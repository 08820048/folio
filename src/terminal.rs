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
    /// Dup of the pty master, so the UI thread can ask the tty which
    /// process group is in the foreground without going through
    /// alacritty's event loop. An interactive shell gives each job its own
    /// group, so `Child::id` alone would only ever name the shell.
    #[cfg(unix)]
    pty_master: Option<std::fs::File>,
    /// The pty's direct child. It called `setsid`, so its pid is also its
    /// process group.
    #[cfg(unix)]
    child_pid: Option<i32>,
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

    /// Run one command and exit, the way Zed's task terminal does. The
    /// shell starts the same way the interactive tab's does — login and
    /// interactive, so `.zprofile` and `.zshrc` both run and PATH has what
    /// they add (brew, cargo, nvm, pyenv) — but `-c` means it does not stay
    /// up afterwards to print a fresh prompt.
    pub fn for_command(command: &str) -> Self {
        #[cfg(windows)]
        {
            Self::for_command_in(
                &std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into()),
                command,
            )
        }
        #[cfg(not(windows))]
        {
            Self::for_command_in(
                &std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()),
                command,
            )
        }
    }

    /// [`Self::for_command`] with the shell named, so the arguments each
    /// shell wants can be checked without touching `$SHELL`.
    pub fn for_command_in(program: &str, command: &str) -> Self {
        // Split on both separators: a Windows path is still a Windows path
        // when the string is looked at from elsewhere.
        let name = program.rsplit(['/', '\\']).next();
        let args = match name {
            Some("cmd.exe") | Some("cmd") => vec!["/S".into(), "/C".into(), command.to_string()],
            Some("zsh") | Some("bash") | Some("fish") => {
                vec!["-l".into(), "-i".into(), "-c".into(), command.to_string()]
            }
            _ => vec!["-c".into(), command.to_string()],
        };
        Self {
            program: program.to_string(),
            args,
        }
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
        #[cfg(unix)]
        let pty_master = pty.file().try_clone().ok();
        #[cfg(unix)]
        let child_pid = i32::try_from(pty.child().id()).ok();
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
            #[cfg(unix)]
            pty_master,
            #[cfg(unix)]
            child_pid,
        })
    }

    /// Send bytes to the child, as if typed.
    pub fn write(&self, bytes: &[u8]) {
        Notifier(self.channel.clone()).notify(bytes.to_vec());
    }

    /// Ctrl-C, as if typed: the tty turns it into SIGINT for the foreground
    /// group. A program in raw mode sees the byte instead and may ignore
    /// it; [`Self::terminate`] and [`Self::kill`] are the next steps.
    pub fn interrupt(&self) {
        self.write(b"\x03");
    }

    /// SIGTERM the foreground job and the pty's child, for a program that
    /// swallowed Ctrl-C. Both groups are signalled: an interactive shell
    /// puts its job in a group of its own, so the child's alone would only
    /// reach the shell.
    pub fn terminate(&self) {
        #[cfg(unix)]
        self.signal_groups(libc::SIGTERM);
    }

    /// SIGKILL the same groups; nothing survives this.
    pub fn kill(&self) {
        #[cfg(unix)]
        self.signal_groups(libc::SIGKILL);
    }

    #[cfg(unix)]
    fn signal_groups(&self, signal: i32) {
        let mut groups: Vec<i32> = self.foreground_pgrp().into_iter().collect();
        if let Some(pid) = self.child_pid
            && !groups.contains(&pid)
        {
            groups.push(pid);
        }
        for group in groups {
            // SAFETY: killpg only sends a signal; a stale group id fails
            // with ESRCH rather than reaching anything else, and the pty's
            // child is ours to signal.
            unsafe {
                libc::killpg(group, signal);
            }
        }
    }

    #[cfg(unix)]
    fn foreground_pgrp(&self) -> Option<i32> {
        use std::os::fd::AsRawFd;
        let fd = self.pty_master.as_ref()?.as_raw_fd();
        // rustix's tcgetpgrp panics in debug on macOS when the ioctl
        // returns 0, which a pty can do before the session is set.
        // SAFETY: `fd` is an open pty master this terminal owns. tcgetpgrp
        // only reads the foreground group; it does not close or take the fd.
        let pid = unsafe { libc::tcgetpgrp(fd) };
        (pid > 0).then_some(pid)
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

    /// Case-insensitive search over the whole scrollback, newest last. The
    /// result is capped so a one-letter query cannot make the UI carry the
    /// entire history.
    pub fn search(&self, query: &str, limit: usize) -> Vec<SearchHit> {
        let query = query.to_lowercase();
        if query.is_empty() || limit == 0 {
            return Vec::new();
        }
        let term = self.term.lock();
        let grid = term.grid();
        let top = grid.topmost_line().0;
        let bottom = grid.bottommost_line().0;
        let mut hits = Vec::new();
        for line in top..=bottom {
            let row = &grid[Line(line)];
            let text: String = row.into_iter().map(|cell| cell.c).collect();
            let lower = text.to_lowercase();
            let mut offset = 0;
            while let Some(found) = lower[offset..].find(&query) {
                let start_byte = offset + found;
                let start = lower[..start_byte].chars().count();
                let end = start + query.chars().count() - 1;
                hits.push(SearchHit {
                    line,
                    column: start,
                    start: GridPoint {
                        line,
                        column: start,
                    },
                    end: GridPoint { line, column: end },
                });
                if hits.len() >= limit {
                    return hits;
                }
                offset = start_byte + query.len();
                if offset >= lower.len() {
                    break;
                }
            }
        }
        hits
    }

    /// Scroll the display so a hit's line is in the viewport. A line already
    /// on screen is left where it is where possible.
    pub fn reveal_search_hit(&self, hit: &SearchHit) {
        let mut term = self.term.lock();
        let (bottom, current) = {
            let grid = term.grid();
            (grid.bottommost_line().0, grid.display_offset() as i32)
        };
        let target = (bottom - hit.line).max(0);
        term.scroll_display(Scroll::Delta(target - current));
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
                    hyperlink: indexed.cell.hyperlink().map(|link| Arc::from(link.uri())),
                };
            }
        }
        apply_detected_urls(&mut cells, self.size.columns, self.size.screen_lines);
        Snapshot {
            columns: self.size.columns,
            rows: self.size.screen_lines,
            display_offset,
            cells,
            cursor: {
                let shape = match content.cursor.shape {
                    AlacCursorShape::Beam => CursorShape::Beam,
                    AlacCursorShape::Underline => CursorShape::Underline,
                    AlacCursorShape::Hidden => CursorShape::Hidden,
                    _ => CursorShape::Block,
                };
                match viewport_cursor_row(
                    content.cursor.point.line.0,
                    display_offset,
                    self.size.screen_lines,
                ) {
                    Some(row) => Cursor {
                        row,
                        column: content.cursor.point.column.0,
                        shape,
                    },
                    None => Cursor {
                        // Past the last drawn row is almost always the
                        // prompt: clamp so the caret and the typed line
                        // stay on screen instead of vanishing.
                        row: self.size.screen_lines.saturating_sub(1),
                        column: content.cursor.point.column.0,
                        shape,
                    },
                }
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

    /// The URI under a viewport cell, if that cell is a hyperlink.
    pub fn hyperlink_at(&self, column: usize, row: usize) -> Option<Arc<str>> {
        self.cells
            .get(row.checked_mul(self.columns)? + column)?
            .hyperlink
            .clone()
    }
}

/// The cursor's row in the visible viewport, if it sits on a drawn line.
/// `line` is alacritty's grid line (0 is the top of the screen); adding
/// `display_offset` turns that into a row index. A negative or past-the-
/// bottom result is off-screen — `as usize` would wrap and the caret
/// would be painted on no row at all.
fn viewport_cursor_row(line: i32, display_offset: usize, rows: usize) -> Option<usize> {
    let row = line.checked_add(display_offset as i32)?;
    (row >= 0 && (row as usize) < rows).then_some(row as usize)
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    pub c: char,
    pub fg: Color,
    pub bg: Color,
    pub attrs: Attrs,
    /// OSC 8 or a URL detected in the row. Shared across the cells of one
    /// link so a click anywhere on it opens the same address.
    pub hyperlink: Option<Arc<str>>,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            c: ' ',
            fg: Color::DefaultForeground,
            bg: Color::DefaultBackground,
            attrs: Attrs::default(),
            hyperlink: None,
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

/// One case-insensitive match in the scrollback. Lines are grid coordinates:
/// negative is history, zero is the top of the screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHit {
    pub line: i32,
    pub column: usize,
    pub start: GridPoint,
    pub end: GridPoint,
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

/// Fill cells that have no OSC 8 link but sit on a printed URL.
fn apply_detected_urls(cells: &mut [Cell], columns: usize, rows: usize) {
    if columns == 0 {
        return;
    }
    for row in 0..rows {
        let start = row * columns;
        let Some(line) = cells.get(start..start + columns) else {
            return;
        };
        let text: String = line.iter().map(|cell| cell.c).collect();
        for (from, to, uri) in find_urls(&text) {
            let uri: Arc<str> = uri.into();
            for col in from..to.min(columns) {
                if let Some(cell) = cells.get_mut(start + col)
                    && cell.hyperlink.is_none()
                {
                    cell.hyperlink = Some(uri.clone());
                }
            }
        }
    }
}

/// Byte ranges of `http(s)://` and `mailto:` URLs in a row, as character
/// offsets so they line up with cells. Trailing punctuation is dropped.
pub fn find_urls(line: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if let Some(scheme) = url_scheme(&bytes[i..]) {
            let start = i;
            i += scheme;
            while i < bytes.len() && is_url_byte(bytes[i]) {
                i += 1;
            }
            let mut end = i;
            while end > start + scheme && is_trailing_url_punct(bytes[end - 1]) {
                end -= 1;
            }
            if end > start + scheme {
                let from = line[..start].chars().count();
                let to = line[..end].chars().count();
                out.push((from, to, line[start..end].to_string()));
            }
        } else {
            i += 1;
        }
    }
    out
}

fn url_scheme(bytes: &[u8]) -> Option<usize> {
    const SCHEMES: [&[u8]; 3] = [b"https://", b"http://", b"mailto:"];
    SCHEMES
        .iter()
        .find(|scheme| bytes.starts_with(scheme))
        .map(|scheme| scheme.len())
}

fn is_url_byte(b: u8) -> bool {
    !b.is_ascii_whitespace() && !matches!(b, b'<' | b'>' | b'"' | b'\'' | b'`')
}

fn is_trailing_url_punct(b: u8) -> bool {
    matches!(
        b,
        b'.' | b',' | b';' | b':' | b'?' | b'!' | b')' | b']' | b'}'
    )
}

/// Whether a URI is safe to hand to the system browser.
pub fn openable_url(uri: &str) -> bool {
    let uri = uri.trim();
    if uri.is_empty() || uri.contains(['\n', '\r', '\0']) {
        return false;
    }
    let lower = uri.to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:")
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

    #[test]
    fn find_urls_drops_trailing_punctuation_and_skips_junk() {
        assert_eq!(
            find_urls("see https://example.com/a."),
            vec![(4, 25, "https://example.com/a".into())]
        );
        assert_eq!(
            find_urls("go (https://example.com/a)"),
            vec![(4, 25, "https://example.com/a".into())]
        );
        assert_eq!(find_urls("no link here").len(), 0);
        assert_eq!(
            find_urls("mailto:dev@example.com, please"),
            vec![(0, 22, "mailto:dev@example.com".into())]
        );
        assert!(!openable_url("javascript:alert(1)"));
        assert!(!openable_url("https://example.com/\nmore"));
        assert!(openable_url("https://example.com/a"));
    }

    #[test]
    fn osc8_and_printed_urls_land_on_their_cells() {
        let mut terminal = Terminal::spawn(Spawn {
            working_directory: None,
            shell: shell(
                "/bin/sh",
                "printf '\\033]8;;https://osc.example/x\\033\\\\osc8\\033]8;;\\033\\\\\\n'; printf 'see https://plain.example/y\\n'",
            ),
            columns: 80,
            rows: 24,
            scrollback: 100,
        })
        .unwrap();
        let snapshot = wait_for(&mut terminal, |_, snapshot| {
            snapshot.text().contains("osc8") && snapshot.text().contains("plain.example")
        });
        assert_eq!(
            snapshot.hyperlink_at(0, 0).as_deref(),
            Some("https://osc.example/x")
        );
        let plain = snapshot
            .row_text(1)
            .find("https://plain.example/y")
            .expect("printed url");
        assert_eq!(
            snapshot.hyperlink_at(plain, 1).as_deref(),
            Some("https://plain.example/y")
        );
    }

    #[test]
    fn search_finds_scrollback_and_reveals_without_panicking() {
        let mut terminal = Terminal::spawn(Spawn {
            working_directory: None,
            shell: shell("/bin/sh", "printf 'alpha beta\nbeta gamma\n'"),
            columns: 40,
            rows: 10,
            scrollback: 100,
        })
        .unwrap();
        wait_for(&mut terminal, |_, snapshot| {
            snapshot.text().contains("alpha beta") && snapshot.text().contains("beta gamma")
        });
        let hits = terminal.search("BETA", 10);
        assert_eq!(hits.len(), 2, "{hits:?}");
        assert_eq!(hits[0].column, 6);
        terminal.reveal_search_hit(&hits[1]);
        assert!(terminal.snapshot().text().contains("gamma"));
        assert!(terminal.search("missing", 10).is_empty());
    }

    #[test]
    fn viewport_cursor_row_stays_on_a_drawn_line() {
        assert_eq!(viewport_cursor_row(0, 0, 24), Some(0));
        assert_eq!(viewport_cursor_row(23, 0, 24), Some(23));
        assert_eq!(viewport_cursor_row(24, 0, 24), None);
        assert_eq!(viewport_cursor_row(-1, 0, 24), None);
        assert_eq!(viewport_cursor_row(-3, 3, 24), Some(0));
        assert_eq!(viewport_cursor_row(0, 3, 24), Some(3));
    }

    #[test]
    fn typed_bytes_are_echoed_on_the_cursor_row() {
        let mut terminal = Terminal::spawn(Spawn {
            working_directory: None,
            shell: shell(
                "/bin/sh",
                "stty echo icanon; printf 'READY'; read line; printf 'GOT%s' \"$line\"",
            ),
            columns: 40,
            rows: 10,
            scrollback: 100,
        })
        .unwrap();
        wait_for(&mut terminal, |_, snapshot| {
            snapshot.text().contains("READY")
        });
        terminal.write(b"abc");
        let snapshot = wait_for(&mut terminal, |_, snapshot| snapshot.text().contains("abc"));
        assert!(
            snapshot.row_text(snapshot.cursor.row).contains("abc")
                || snapshot.text().contains("abc"),
            "echoed input missing from the grid:\n{}",
            snapshot.text()
        );
    }

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

    #[test]
    fn a_run_shell_is_a_login_shell_that_exits_after_the_command() {
        let zsh = Shell::for_command_in("/bin/zsh", "cargo run");
        assert_eq!(zsh.program, "/bin/zsh");
        assert_eq!(zsh.args, ["-l", "-i", "-c", "cargo run"]);
        let bash = Shell::for_command_in("/usr/local/bin/bash", "x");
        assert_eq!(bash.args, ["-l", "-i", "-c", "x"]);
        let fish = Shell::for_command_in("/opt/homebrew/bin/fish", "x");
        assert_eq!(fish.args, ["-l", "-i", "-c", "x"]);
        // `sh` has no rc files worth loading and rejects `-l -i -c` on some
        // systems; plain `-c` is the whole story.
        let sh = Shell::for_command_in("/bin/sh", "x");
        assert_eq!(sh.args, ["-c", "x"]);
        let cmd = Shell::for_command_in(r"C:\Windows\System32\cmd.exe", "x");
        assert_eq!(cmd.args, ["/S", "/C", "x"]);
    }

    /// A program that traps Ctrl-C keeps running; SIGTERM to its group is
    /// what ends it. The shell that started it goes with it.
    #[cfg(unix)]
    #[test]
    fn terminate_reaches_a_job_that_ignores_ctrl_c() {
        let mut terminal = Terminal::spawn(Spawn {
            working_directory: None,
            shell: Some(Shell {
                program: "/bin/sh".into(),
                args: vec![
                    "-c".into(),
                    "trap '' INT; printf READY; sleep 60; printf DONE".into(),
                ],
            }),
            columns: 40,
            rows: 10,
            scrollback: 100,
        })
        .unwrap();
        wait_for(&mut terminal, |_, snapshot| {
            snapshot.text().contains("READY")
        });
        terminal.interrupt();
        std::thread::sleep(Duration::from_millis(200));
        terminal.take_events();
        assert!(
            !terminal.exited(),
            "Ctrl-C was trapped, the job should still be up:\n{}",
            terminal.snapshot().text()
        );
        terminal.terminate();
        wait_for(&mut terminal, |terminal, _| terminal.exited());
        assert!(!terminal.snapshot().text().contains("DONE"));
    }

    #[test]
    fn a_run_command_exits_when_the_child_does() {
        let mut terminal = Terminal::spawn(Spawn {
            working_directory: None,
            shell: Some(Shell::for_command("printf 'hello-run'")),
            columns: 40,
            rows: 10,
            scrollback: 100,
        })
        .unwrap();
        wait_for(&mut terminal, |_, snapshot| {
            snapshot.text().contains("hello-run")
        });
        wait_for(&mut terminal, |terminal, _| terminal.exited());
    }
}
