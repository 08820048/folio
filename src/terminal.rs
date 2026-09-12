//! Terminal backend: a pty child speaking the VT language, plus the grid
//! its output lands in.
//!
//! The pty and the parser are alacritty_terminal's: its `EventLoop` owns a
//! reader thread that feeds everything the child writes into the grid, and
//! a `Notifier` writes back what the editor sends. What this module adds is
//! the seam the UI works against — spawn with the project's shell, `write`
//! keystrokes, `resize` with real cell metrics, and read an owned
//! [`Snapshot`] of the visible grid that no longer touches alacritty's
//! types. Events that answer the child (a cursor-position query, say) are
//! forwarded to the pty here; the ones a human cares about — output, title,
//! bell, exit — come out as [`TerminalEvent`]s.

use std::{
    collections::HashMap,
    io,
    path::Path,
    sync::{
        Arc,
        mpsc::{self, Receiver, Sender, TryRecvError},
    },
};

use alacritty_terminal::{
    event::{Event, EventListener, Notify, WindowSize},
    event_loop::{EventLoop, EventLoopSender, Msg, Notifier},
    grid::{Dimensions, Scroll},
    index::{Column, Line, Point as AlacPoint, Side as AlacSide},
    selection::{Selection as AlacSelection, SelectionType as AlacSelectionType},
    sync::FairMutex,
    term::{Config, Term, TermMode, cell::Flags},
    tty::{self, Shell as TtyShell},
    vte::ansi::{Color as AlacColor, CursorShape as AlacCursorShape, NamedColor, Rgb},
};

/// The grid holds this many lines of history at most, whatever a settings
/// file says.
const MAX_SCROLLBACK: usize = 100_000;

/// A running terminal: the pty, the grid its output lands in, and the
/// thread moving bytes between them. Dropping it shuts the child down.
pub struct Terminal {
    term: Arc<FairMutex<Term<Listener>>>,
    channel: EventLoopSender,
    events: Receiver<Event>,
    size: GridSize,
    cell_size: (u16, u16),
    exited: bool,
    exit_code: Option<i32>,
}

/// What a terminal is spawned with. A `None` shell leaves the choice to the
/// pty, which starts the account's own shell as a login shell.
pub struct Spawn<'a> {
    pub working_directory: Option<&'a Path>,
    pub shell: Option<Shell>,
    pub columns: usize,
    pub rows: usize,
    pub scrollback: usize,
}

/// A program and its arguments. See [`Shell::user_default`] for the common
/// case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shell {
    pub program: String,
    pub args: Vec<String>,
}

impl Shell {
    /// The account's shell, as a login — and, for the shells that read
    /// their rc files only when interactive, interactive as well. The
    /// user's own PATH is what the terminal starts with, which is what
    /// puts `cargo` and `npm` there at all.
    pub fn user_default() -> Option<Self> {
        let program = std::env::var("SHELL").ok()?;
        let args: &[&str] = match Path::new(&program).file_name().and_then(|n| n.to_str()) {
            Some("zsh") | Some("bash") => &["-l", "-i"],
            _ => &[],
        };
        Some(Self {
            program,
            args: args.iter().map(|arg| arg.to_string()).collect(),
        })
    }
}

/// The events a terminal emits upward, after [`Terminal::take_events`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalEvent {
    /// The grid changed; the view should redraw.
    Wakeup,
    /// The child set a window title, or reset to none.
    TitleChanged(Option<String>),
    /// The terminal bell rang.
    Bell,
    /// Cursor blinking toggled; the view's blink timer follows.
    BlinkingChanged,
    /// The child process is gone.
    Exited,
}

impl Terminal {
    /// Start a child on a fresh pty. The reader thread outlives this call;
    /// the grid starts empty and fills as the child writes.
    pub fn spawn(options: Spawn) -> io::Result<Terminal> {
        let size = GridSize {
            columns: options.columns,
            screen_lines: options.rows,
        };
        let (sender, receiver) = mpsc::channel();
        let listener = Listener(sender);
        let config = Config {
            scrolling_history: options.scrollback.min(MAX_SCROLLBACK),
            ..Config::default()
        };
        let term = Arc::new(FairMutex::new(Term::new(config, &size, listener.clone())));
        let mut env = HashMap::new();
        env.insert("TERM".to_string(), "xterm-256color".to_string());
        env.insert("COLORTERM".to_string(), "truecolor".to_string());
        env.insert("FOLIO_TERM".to_string(), "1".to_string());
        let tty_options = tty::Options {
            shell: options.shell.map(|s| TtyShell::new(s.program, s.args)),
            working_directory: options.working_directory.map(|p| p.to_owned()),
            drain_on_exit: true,
            env,
            #[cfg(target_os = "windows")]
            escape_args: true,
        };
        // The cell metrics are placeholders until the view resizes with the
        // real ones; nothing the child asks for before that cares.
        let pty = tty::new(
            &tty_options,
            WindowSize {
                num_cols: size.columns as u16,
                num_lines: size.screen_lines as u16,
                cell_width: 8,
                cell_height: 16,
            },
            0,
        )?;
        let event_loop = EventLoop::new(term.clone(), listener, pty, true, false)?;
        let channel = event_loop.channel();
        event_loop.spawn();
        Ok(Terminal {
            term,
            channel,
            events: receiver,
            size,
            cell_size: (8, 16),
            exited: false,
            exit_code: None,
        })
    }

    /// Send bytes to the child, as if typed.
    pub fn write(&self, bytes: &[u8]) {
        Notifier(self.channel.clone()).notify(bytes.to_vec());
    }

    /// Resize the grid and the pty. The cell metrics are what the child
    /// sees for pixel-size queries; the view passes the real ones.
    pub fn resize(&mut self, columns: usize, rows: usize, cell_width: u16, cell_height: u16) {
        self.cell_size = (cell_width, cell_height);
        if self.size.columns != columns || self.size.screen_lines != rows {
            self.size = GridSize {
                columns,
                screen_lines: rows,
            };
            self.term.lock().resize(self.size);
        }
        let _ = self.channel.send(Msg::Resize(WindowSize {
            num_cols: columns as u16,
            num_lines: rows as u16,
            cell_width,
            cell_height,
        }));
    }

    /// Scroll the view back into history (positive) or down toward the
    /// present (negative), in whole lines.
    pub fn scroll(&self, lines: i32) {
        self.term.lock().scroll_display(Scroll::Delta(lines));
    }

    /// Begin a selection of the given kind at a grid point. The line is in
    /// grid coordinates: zero is the top of the screen, negative is history.
    pub fn start_selection(&self, point: GridPoint, kind: SelectionKind) {
        let mut term = self.term.lock();
        let mut selection = AlacSelection::new(
            match kind {
                SelectionKind::Char => AlacSelectionType::Simple,
                // A double-click is the word under the cursor, which
                // alacritty calls semantic after the characters that end one.
                SelectionKind::Word => AlacSelectionType::Semantic,
                SelectionKind::Line => AlacSelectionType::Lines,
            },
            alac_point(point),
            AlacSide::Left,
        );
        selection.update(alac_point(point), AlacSide::Right);
        term.selection = Some(selection);
    }

    /// Drag the selection's head to a grid point. `forward` says whether
    /// the head is now after the anchor, which decides which side of the
    /// head cell belongs to the selection.
    pub fn update_selection(&self, point: GridPoint, forward: bool) {
        let mut term = self.term.lock();
        if let Some(mut selection) = term.selection.take() {
            selection.update(
                alac_point(point),
                if forward {
                    AlacSide::Right
                } else {
                    AlacSide::Left
                },
            );
            term.selection = Some(selection);
        }
    }

    pub fn clear_selection(&self) {
        self.term.lock().selection = None;
    }

    /// The selection's text, if any. Lines the selection covers come back
    /// whole, joined the way a copy into an editor wants them.
    pub fn selection_text(&self) -> Option<String> {
        self.term.lock().selection_to_string()
    }

    /// Put text into the child as if pasted. Bracketed paste is decided by
    /// the mode the child itself switched on.
    pub fn paste(&self, text: &str) {
        let bracketed = self.term.lock().mode().contains(TermMode::BRACKETED_PASTE);
        self.write(&crate::terminal_keys::paste(text, bracketed));
    }

    /// Whether the child is gone. The events already said so, but a view
    /// joining late reads this instead of replaying history.
    pub fn exited(&self) -> bool {
        self.exited
    }

    /// The child's exit code, once it is gone.
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    /// The grid's current shape: columns first, then rows.
    pub fn size(&self) -> (usize, usize) {
        (self.size.columns, self.size.screen_lines)
    }

    /// The cell metrics last set by [`Terminal::resize`].
    pub fn cell_size(&self) -> (u16, u16) {
        self.cell_size
    }

    /// Drain the events since last called. Responses the child is waiting
    /// on go straight to the pty on the way through.
    pub fn take_events(&mut self) -> Vec<TerminalEvent> {
        let mut out = Vec::new();
        loop {
            match self.events.try_recv() {
                Ok(event) => match event {
                    Event::Wakeup => out.push(TerminalEvent::Wakeup),
                    Event::Title(title) => out.push(TerminalEvent::TitleChanged(Some(title))),
                    Event::ResetTitle => out.push(TerminalEvent::TitleChanged(None)),
                    Event::Bell => out.push(TerminalEvent::Bell),
                    Event::CursorBlinkingChange => out.push(TerminalEvent::BlinkingChanged),
                    Event::ChildExit(status) => self.exit_code = status.code(),
                    Event::Exit if !self.exited => {
                        self.exited = true;
                        out.push(TerminalEvent::Exited);
                    }
                    Event::PtyWrite(text) => {
                        Notifier(self.channel.clone()).notify(text.into_bytes());
                    }
                    // Mouse-cursor shape hints, OSC 52 (off by default) and
                    // the color/size queries nothing in the project asks
                    // for yet have nowhere to go.
                    _ => {}
                },
                Err(TryRecvError::Empty) => break,
                // The reader thread ended; everything it had to say is read.
                Err(TryRecvError::Disconnected) => break,
            }
        }
        out
    }

    /// What the grid holds right now, owned. The UI renders from this and
    /// never touches alacritty's types.
    pub fn snapshot(&self) -> Snapshot {
        let term = self.term.lock();
        let content = term.renderable_content();
        let display_offset = content.display_offset;
        let capacity = self.size.columns * self.size.screen_lines;
        let mut cells = vec![Cell::default(); capacity];
        for indexed in content.display_iter {
            let row = (indexed.point.line.0 + display_offset as i32) as usize;
            let index = row * self.size.columns + indexed.point.column.0;
            if let Some(slot) = cells.get_mut(index) {
                *slot = Cell {
                    c: indexed.c,
                    fg: color(indexed.fg),
                    bg: color(indexed.bg),
                    attrs: attrs(indexed.flags),
                };
            }
        }
        Snapshot {
            columns: self.size.columns,
            rows: self.size.screen_lines,
            display_offset,
            cells,
            cursor: Cursor {
                row: content.cursor.point.line.0 as usize,
                column: content.cursor.point.column.0,
                shape: match content.cursor.shape {
                    AlacCursorShape::Beam => CursorShape::Beam,
                    AlacCursorShape::Underline => CursorShape::Underline,
                    AlacCursorShape::Hidden => CursorShape::Hidden,
                    _ => CursorShape::Block,
                },
            },
            selection: content.selection.map(|range| SelectionSpan {
                start: grid_point(range.start),
                end: grid_point(range.end),
            }),
            modes: Modes {
                app_cursor: content.mode.contains(TermMode::APP_CURSOR),
                alt_screen: content.mode.contains(TermMode::ALT_SCREEN),
                bracketed_paste: content.mode.contains(TermMode::BRACKETED_PASTE),
                alternate_scroll: content.mode.contains(TermMode::ALTERNATE_SCROLL),
                sgr_mouse: content.mode.contains(TermMode::SGR_MOUSE),
                mouse_reporting: content.mode.intersects(
                    TermMode::MOUSE_REPORT_CLICK | TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION,
                ),
                mouse_motion: content.mode.contains(TermMode::MOUSE_MOTION),
            },
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.channel.send(Msg::Shutdown);
    }
}

/// Forwards the grid's events upward. The spawned term and the event loop
/// both speak to this one channel.
#[derive(Clone)]
struct Listener(Sender<Event>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let _ = self.0.send(event);
    }
}

/// The grid's shape, in the terms `Term::resize` asks for: history lives in
/// the grid already, so the total is the viewport.
#[derive(Clone, Copy, Debug)]
struct GridSize {
    columns: usize,
    screen_lines: usize,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.screen_lines()
    }

    fn screen_lines(&self) -> usize {
        self.screen_lines
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

/// What the grid holds right now: the visible viewport, wherever it is
/// scrolled to, plus the cursor and the modes input has to respect.
pub struct Snapshot {
    pub columns: usize,
    pub rows: usize,
    /// How far the view is scrolled back into history, in lines. Zero is
    /// the present.
    pub display_offset: usize,
    /// `rows * columns` cells, top row first.
    pub cells: Vec<Cell>,
    pub cursor: Cursor,
    /// The live selection in grid coordinates, if any.
    pub selection: Option<SelectionSpan>,
    pub modes: Modes,
}

impl Snapshot {
    /// One row's text, trailing blanks trimmed.
    pub fn row_text(&self, row: usize) -> String {
        let start = row * self.columns;
        self.cells[start..start + self.columns]
            .iter()
            .map(|cell| cell.c)
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    /// The whole viewport as text, one row per line.
    pub fn text(&self) -> String {
        (0..self.rows)
            .map(|row| self.row_text(row))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Where the cursor is and what it would look like.
pub struct Cursor {
    pub row: usize,
    pub column: usize,
    pub shape: CursorShape,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    Beam,
    Underline,
    Hidden,
}

/// Content and attributes of one cell in the grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub c: char,
    pub fg: Color,
    pub bg: Color,
    pub attrs: Attrs,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            c: ' ',
            fg: Color::DefaultForeground,
            bg: Color::DefaultBackground,
            attrs: Attrs::default(),
        }
    }
}

/// A cell color: the theme's default, one of the sixteen the palette
/// names, a 256-color index, or a direct truecolor value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    DefaultForeground,
    DefaultBackground,
    Palette(u8),
    Rgb(u8, u8, u8),
}

/// The attributes a cell carries, as the renderer needs them. The four
/// underline styles arrive as one, since the view draws a line regardless.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Attrs(u16);

impl Attrs {
    pub const BOLD: Self = Self(1 << 0);
    pub const DIM: Self = Self(1 << 1);
    pub const ITALIC: Self = Self(1 << 2);
    pub const UNDERLINE: Self = Self(1 << 3);
    pub const STRIKETHROUGH: Self = Self(1 << 4);
    pub const INVERSE: Self = Self(1 << 5);
    pub const HIDDEN: Self = Self(1 << 6);
    pub const WIDE_CHAR: Self = Self(1 << 7);
    pub const WIDE_CHAR_SPACER: Self = Self(1 << 8);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// The modes the child switched on, as input and scrolling need to know.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modes {
    pub app_cursor: bool,
    pub alt_screen: bool,
    pub bracketed_paste: bool,
    /// Alternate-screen wheel turns are the child's arrow keys.
    pub alternate_scroll: bool,
    /// Any of the mouse reporting protocols is on; wheel and click events
    /// belong to the child, not the scrollback.
    pub mouse_reporting: bool,
    /// Reports are SGR's decimal form rather than the old three bytes.
    pub sgr_mouse: bool,
    /// Hover motion is reported, not just drags.
    pub mouse_motion: bool,
}

/// A point in the grid: the line in grid coordinates — zero is the top of
/// the screen, negative is history — and the column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridPoint {
    pub line: i32,
    pub column: usize,
}

/// What a drag or a click count is building.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionKind {
    /// Exactly the cells between anchor and head.
    Char,
    /// The words under anchor and head, and everything between.
    Word,
    /// Whole lines from the anchor's to the head's.
    Line,
}

/// A selection in grid coordinates, anchors already normalized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionSpan {
    pub start: GridPoint,
    pub end: GridPoint,
}

fn alac_point(point: GridPoint) -> AlacPoint<Line> {
    AlacPoint::new(Line(point.line), Column(point.column))
}

fn grid_point(point: AlacPoint<Line>) -> GridPoint {
    GridPoint {
        line: point.line.0,
        column: point.column.0,
    }
}

fn color(c: AlacColor) -> Color {
    match c {
        AlacColor::Named(NamedColor::Foreground) => Color::DefaultForeground,
        AlacColor::Named(NamedColor::Background) => Color::DefaultBackground,
        AlacColor::Named(named) => Color::Palette(palette_index(named)),
        AlacColor::Spec(Rgb { r, g, b }) => Color::Rgb(r, g, b),
        AlacColor::Indexed(index) => Color::Palette(index),
    }
}

/// The named colors become palette entries; the dim ones land on the same
/// entries as their plain forms, since the attribute carries the faintness.
fn palette_index(named: NamedColor) -> u8 {
    match named {
        NamedColor::Black | NamedColor::DimBlack => 0,
        NamedColor::Red | NamedColor::DimRed => 1,
        NamedColor::Green | NamedColor::DimGreen => 2,
        NamedColor::Yellow | NamedColor::DimYellow => 3,
        NamedColor::Blue | NamedColor::DimBlue => 4,
        NamedColor::Magenta | NamedColor::DimMagenta => 5,
        NamedColor::Cyan | NamedColor::DimCyan => 6,
        NamedColor::White | NamedColor::DimWhite => 7,
        NamedColor::BrightBlack => 8,
        NamedColor::BrightRed => 9,
        NamedColor::BrightGreen => 10,
        NamedColor::BrightYellow => 11,
        NamedColor::BrightBlue => 12,
        NamedColor::BrightMagenta => 13,
        NamedColor::BrightCyan => 14,
        NamedColor::BrightWhite => 15,
        _ => 0,
    }
}

fn attrs(flags: Flags) -> Attrs {
    let mut bits = 0;
    for (set, attr) in [
        (Flags::BOLD, Attrs::BOLD),
        (Flags::DIM, Attrs::DIM),
        (Flags::ITALIC, Attrs::ITALIC),
        (
            Flags::UNDERLINE
                | Flags::DOUBLE_UNDERLINE
                | Flags::UNDERCURL
                | Flags::DOTTED_UNDERLINE
                | Flags::DASHED_UNDERLINE,
            Attrs::UNDERLINE,
        ),
        (Flags::STRIKEOUT, Attrs::STRIKETHROUGH),
        (Flags::INVERSE, Attrs::INVERSE),
        (Flags::HIDDEN, Attrs::HIDDEN),
        (Flags::WIDE_CHAR, Attrs::WIDE_CHAR),
        (Flags::WIDE_CHAR_SPACER, Attrs::WIDE_CHAR_SPACER),
    ] {
        if flags.contains(set) {
            bits |= attr.0;
        }
    }
    Attrs(bits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    const DEADLINE: Duration = Duration::from_secs(10);

    fn shell(program: &str, script: &str) -> Option<Shell> {
        Some(Shell {
            program: program.to_string(),
            args: vec!["-c".to_string(), script.to_string()],
        })
    }

    /// Drain events until `until` holds or the deadline passes.
    fn wait_for(terminal: &mut Terminal, until: impl Fn(&Terminal, &Snapshot) -> bool) -> Snapshot {
        let deadline = Instant::now() + DEADLINE;
        loop {
            terminal.take_events();
            let snapshot = terminal.snapshot();
            if until(terminal, &snapshot) {
                return snapshot;
            }
            assert!(Instant::now() < deadline, "the child never got there");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn a_childs_output_lands_in_the_grid_and_its_exit_is_seen() {
        let mut terminal = Terminal::spawn(Spawn {
            working_directory: None,
            shell: shell("/bin/sh", "printf 'hello world'"),
            columns: 80,
            rows: 24,
            scrollback: 100,
        })
        .unwrap();
        let snapshot = wait_for(&mut terminal, |_, snapshot| {
            snapshot.text().contains("hello world")
        });
        assert_eq!(snapshot.row_text(0), "hello world");
        assert_eq!(snapshot.display_offset, 0);
        assert_eq!(snapshot.cells.len(), 24 * 80);

        wait_for(&mut terminal, |terminal, _| terminal.exited());
        assert_eq!(terminal.exit_code(), Some(0));
    }

    #[test]
    fn ansi_attributes_come_through_the_grid() {
        let mut terminal = Terminal::spawn(Spawn {
            working_directory: None,
            shell: shell("/bin/sh", "printf '\\033[31mR\\033[0m\\033[1mB'"),
            columns: 80,
            rows: 24,
            scrollback: 100,
        })
        .unwrap();
        let snapshot = wait_for(&mut terminal, |_, snapshot| snapshot.text().contains("RB"));

        let red = snapshot.cells.iter().find(|c| c.c == 'R').unwrap();
        assert_eq!(red.fg, Color::Palette(1));
        assert!(red.attrs.is_empty());
        let bold = snapshot.cells.iter().find(|c| c.c == 'B').unwrap();
        assert_eq!(bold.fg, Color::DefaultForeground);
        assert!(bold.attrs.contains(Attrs::BOLD));
    }

    #[test]
    fn resize_reaches_the_child() {
        let mut terminal = Terminal::spawn(Spawn {
            working_directory: None,
            shell: shell("/bin/sh", "stty size; read line; stty size"),
            columns: 40,
            rows: 10,
            scrollback: 100,
        })
        .unwrap();
        wait_for(&mut terminal, |_, snapshot| {
            snapshot.text().contains("10 40")
        });

        terminal.resize(80, 24, 8, 16);
        assert_eq!(terminal.size(), (80, 24));
        assert_eq!(terminal.cell_size(), (8, 16));
        terminal.write(b"go\r");
        wait_for(&mut terminal, |_, snapshot| {
            snapshot.text().contains("24 80")
        });
    }

    #[test]
    fn scrolling_reaches_the_history() {
        let mut terminal = Terminal::spawn(Spawn {
            working_directory: None,
            shell: shell(
                "/bin/sh",
                "i=1; while [ $i -le 30 ]; do echo \"line$i\"; i=$((i+1)); done",
            ),
            columns: 40,
            rows: 10,
            scrollback: 1000,
        })
        .unwrap();
        let snapshot = wait_for(&mut terminal, |_, snapshot| {
            snapshot.text().contains("line30")
        });
        assert_eq!(snapshot.display_offset, 0);

        // Back as far as the buffer goes shows the first line at the top.
        terminal.scroll(1000);
        let snapshot = terminal.snapshot();
        assert!(snapshot.display_offset > 0);
        assert!(snapshot.row_text(0).starts_with("line1"));

        // Forward again lands on the present: the trailing newline leaves
        // the cursor on the empty row after line30.
        terminal.scroll(-1000);
        let snapshot = terminal.snapshot();
        assert_eq!(snapshot.display_offset, 0);
        assert!(snapshot.row_text(snapshot.rows - 2).starts_with("line30"));
    }

    #[test]
    fn a_drag_selects_and_copies() {
        let mut terminal = Terminal::spawn(Spawn {
            working_directory: None,
            shell: shell("/bin/sh", "printf 'hello world'"),
            columns: 80,
            rows: 24,
            scrollback: 100,
        })
        .unwrap();
        wait_for(&mut terminal, |_, snapshot| {
            snapshot.text().contains("hello world")
        });

        // A drag from the first to the fifth character takes those five.
        terminal.start_selection(GridPoint { line: 0, column: 0 }, SelectionKind::Char);
        terminal.update_selection(GridPoint { line: 0, column: 4 }, true);
        assert_eq!(terminal.selection_text().as_deref(), Some("hello"));

        // A word click takes the word under it, not just the cell.
        terminal.clear_selection();
        terminal.start_selection(GridPoint { line: 0, column: 1 }, SelectionKind::Word);
        assert_eq!(terminal.selection_text().as_deref(), Some("hello"));

        // Clearing leaves nothing behind.
        terminal.clear_selection();
        assert_eq!(terminal.selection_text(), None);
    }

    #[test]
    fn paste_reaches_the_child() {
        let mut terminal = Terminal::spawn(Spawn {
            working_directory: None,
            shell: shell("/bin/sh", "read line; echo \"got:$line\""),
            columns: 80,
            rows: 24,
            scrollback: 100,
        })
        .unwrap();
        wait_for(&mut terminal, |_, _| {
            // The prompt may or may not print; the read is what waits, and
            // pasting is what unblocks it.
            true
        });
        terminal.paste("hi\n");
        let snapshot = wait_for(&mut terminal, |_, snapshot| {
            snapshot.text().contains("got:hi")
        });
        assert!(snapshot.text().contains("got:hi"));
    }
}
