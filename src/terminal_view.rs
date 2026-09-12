//! The terminal dock's view: a running backend grid drawn row by row.
//!
//! The backend (`folio::terminal`) owns the pty and the parser; this side
//! owns nothing but presentation. Each frame takes a snapshot and turns it
//! into one shaped line per row — runs of same-styled cells become a
//! [`TextRun`], and the cursor restyles the cell it sits on — while keys
//! and wheel turns go back to the child as bytes. A background task drains
//! the backend's events every few frames so output arrives while the user
//! works elsewhere, and a dock-wide `sync` keeps the grid sized to the
//! panel the dock gives it.

use std::{cell::RefCell, ops::Range, path::Path, rc::Rc, time::Duration};

use folio::terminal::{self, Color, GridPoint, SelectionKind, Terminal, TerminalEvent};
use folio::terminal_keys::{Key, Modifiers, encode};
use folio::terminal_mouse::{self, Button};
use gpui::{
    AnyElement, App, Bounds, ClipboardItem, Context, Element, ElementId, FocusHandle, Focusable,
    Font, FontStyle, FontWeight, GlobalElementId, Hsla, InputHandler, InspectorElementId,
    IntoElement, KeyDownEvent, Keystroke, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, Pixels, Render, ScrollWheelEvent, Style, Styled, StyledText,
    TextRun, UTF16Selection, UnderlineStyle, Window, div, point, prelude::*, px, rgb, size,
};
use gpui_component::ActiveTheme;

/// The terminal font. One family, the code font; the size follows settings.
const FONT: &str = "JetBrains Mono";

/// Sixteen colors the grid's palette entries resolve to, one set per
/// appearance. Calm defaults tuned to sit beside the editor's own colors;
/// nothing in the theme derives from these, and retuning them is a matter
/// of editing the tables.
const DARK_PALETTE: [u32; 16] = [
    0x282C34, 0xE06C75, 0x98C379, 0xE5C07B, 0x61AFEF, 0xC678DD, 0x56B6C2, 0xABB2BF, 0x5C6370,
    0xEF7D85, 0xA5CE8B, 0xEED49F, 0x74BEF0, 0xD18ADF, 0x66C3CC, 0xC7CDD8,
];
const LIGHT_PALETTE: [u32; 16] = [
    0x383A42, 0xD2453C, 0x417B39, 0x96691E, 0x0B6E8F, 0x8E1F8D, 0x06748A, 0x5C6370, 0x4F525E,
    0xE45649, 0x50A14F, 0xC18401, 0x0184BC, 0xA626A4, 0x0997B3, 0x828997,
];

/// The colors one frame of the grid is drawn with, resolved against the
/// theme and the appearance.
struct TermColors {
    foreground: Hsla,
    background: Hsla,
    cursor: Hsla,
    cursor_text: Hsla,
    selection: Hsla,
    palette: [Hsla; 16],
}

impl TermColors {
    fn new(cx: &App) -> Self {
        let theme = cx.theme();
        let table = if theme.is_dark() {
            DARK_PALETTE
        } else {
            LIGHT_PALETTE
        };
        Self {
            foreground: theme.foreground,
            background: theme.background,
            cursor: theme.accent_foreground,
            cursor_text: theme.background,
            selection: theme.selection,
            palette: table.map(|c| rgb(c).into()),
        }
    }

    fn resolve(&self, color: Color) -> Hsla {
        match color {
            Color::DefaultForeground => self.foreground,
            Color::DefaultBackground => self.background,
            Color::Palette(index) => self.palette[index as usize % 16],
            Color::Rgb(r, g, b) => rgb((r as u32) << 16 | (g as u32) << 8 | b as u32).into(),
        }
    }
}

/// One terminal, as the dock renders it. Dropping the view drops the child.
pub struct TerminalView {
    terminal: Option<Terminal>,
    /// Why there is no terminal, when spawning failed.
    error: Option<String>,
    title: Option<String>,
    exited: bool,
    exit_code: Option<i32>,
    focus: FocusHandle,
    /// Wheel turns not yet worth a whole line, so trackpads scroll smoothly.
    scroll_px: Pixels,
    /// The cell metrics the last sync measured, for the wheel and for the
    /// dock's own arithmetic.
    metrics: (Pixels, Pixels),
    /// The selection a drag is building, as its kind; the anchor lives in
    /// the grid and the head follows the mouse.
    selecting: Option<SelectionKind>,
    /// Where the drag started, so the head's side is known.
    selection_anchor: Option<GridPoint>,
    /// Where the grid painted, shared with the IME element that measures
    /// it; mouse positions become cells against this.
    grid_bounds: Rc<RefCell<Option<Bounds<Pixels>>>>,
    /// Text the IME has marked but not yet committed.
    marked_text: Option<String>,
}

impl TerminalView {
    /// Start a child and the task that watches it.
    pub fn new(
        working_directory: &Path,
        columns: usize,
        rows: usize,
        cx: &mut Context<Self>,
    ) -> Self {
        let spawn = Terminal::spawn(terminal::Spawn {
            working_directory: Some(working_directory),
            shell: terminal::Shell::user_default(),
            columns,
            rows,
            scrollback: 10_000,
        });
        let (terminal, error) = match spawn {
            Ok(terminal) => (Some(terminal), None),
            Err(error) => (None, Some(error.to_string())),
        };
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(30))
                    .await;
                let Ok(()) = this.update(cx, |view, cx| {
                    if view.poll() {
                        cx.notify();
                    }
                }) else {
                    // The view is gone; so is the poll.
                    return;
                };
            }
        })
        .detach();
        Self {
            terminal,
            error,
            title: None,
            exited: false,
            exit_code: None,
            focus: cx.focus_handle(),
            scroll_px: px(0.),
            metrics: (px(8.), px(16.)),
            selecting: None,
            selection_anchor: None,
            grid_bounds: Rc::new(RefCell::new(None)),
            marked_text: None,
        }
    }

    /// Drain the backend's events into view state; whether anything the
    /// user can see changed.
    fn poll(&mut self) -> bool {
        let Some(terminal) = self.terminal.as_mut() else {
            return false;
        };
        let mut changed = false;
        for event in terminal.take_events() {
            match event {
                TerminalEvent::Wakeup => changed = true,
                TerminalEvent::TitleChanged(title) => {
                    self.title = title;
                    changed = true;
                }
                TerminalEvent::Bell | TerminalEvent::BlinkingChanged => {}
                TerminalEvent::Exited => {
                    self.exited = true;
                    changed = true;
                }
            }
        }
        if terminal.exit_code().is_some() && self.exit_code.is_none() {
            self.exit_code = terminal.exit_code();
            changed = true;
        }
        changed
    }

    /// Size the grid to what the dock computed for it. Nothing happens
    /// when the shape already fits, so calling this every frame is fine.
    pub fn sync(
        &mut self,
        columns: usize,
        rows: usize,
        cell_width: u16,
        line_height: u16,
        cx: &mut Context<Self>,
    ) {
        self.metrics = (px(f32::from(cell_width)), px(f32::from(line_height)));
        let Some(terminal) = self.terminal.as_mut() else {
            return;
        };
        if terminal.size() != (columns, rows) {
            terminal.resize(columns, rows, cell_width, line_height);
            cx.notify();
        }
    }

    /// Where keyboard focus lives while the terminal is in use.
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// The title the child set, or none.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Whether the child is gone, and its exit code if it is known.
    pub fn exit(&self) -> Option<Option<i32>> {
        self.exited.then_some(self.exit_code)
    }

    fn wheel(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(terminal) = self.terminal.as_ref() else {
            return;
        };
        let snapshot = terminal.snapshot();
        let (_, line_height) = self.metrics;
        // A wheel turn up is positive, and positive is back into history.
        self.scroll_px += event.delta.pixel_delta(line_height).y;
        let lines = (f32::from(self.scroll_px) / f32::from(line_height)).floor() as i32;
        if lines == 0 {
            return;
        }
        self.scroll_px -= px(lines as f32 * f32::from(line_height));
        let modes = snapshot.modes;
        if modes.mouse_reporting {
            // The child asked for the wheel itself.
            if let Some((column, row)) = self.cell_at(event.position) {
                terminal.write(&terminal_mouse::wheel(
                    lines > 0,
                    Modifiers::default(),
                    column,
                    row,
                    modes.sgr_mouse,
                ));
            }
            cx.notify();
            return;
        }
        if modes.alt_screen && modes.alternate_scroll && snapshot.display_offset == 0 {
            // Alternate scroll: a full-screen child without mouse reports
            // still wants the wheel, as its own arrow keys.
            let bytes: &[u8] = if lines > 0 { b"\x1b[A" } else { b"\x1b[B" };
            for _ in 0..lines.unsigned_abs() {
                terminal.write(bytes);
            }
            cx.notify();
            return;
        }
        terminal.scroll(lines);
        cx.notify();
    }

    /// The cell a window position lands on, clamped into the grid.
    fn cell_at(&self, position: gpui::Point<Pixels>) -> Option<(usize, usize)> {
        let origin = self.grid_bounds.borrow().as_ref()?.origin;
        let (cell_width, line_height) = self.metrics;
        let snapshot = self.terminal.as_ref()?.snapshot();
        let column = (((position.x - origin.x) / cell_width).floor() as i32)
            .clamp(0, snapshot.columns as i32 - 1) as usize;
        let row = (((position.y - origin.y) / line_height).floor() as i32)
            .clamp(0, snapshot.rows as i32 - 1) as usize;
        Some((column, row))
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
        let Some(terminal) = self.terminal.as_ref() else {
            return;
        };
        let Some((column, row)) = self.cell_at(event.position) else {
            return;
        };
        let modes = terminal.snapshot().modes;
        if modes.mouse_reporting {
            let button = match event.button {
                MouseButton::Middle => Button::Middle,
                MouseButton::Right => Button::Right,
                _ => Button::Left,
            };
            terminal.write(&terminal_mouse::press(
                button,
                mods_of(event.modifiers),
                column,
                row,
                modes.sgr_mouse,
            ));
            return;
        }
        // A click starts a selection: plain drag for one click, the word
        // for two, the whole line for three.
        let kind = match event.click_count {
            2 => SelectionKind::Word,
            3 => SelectionKind::Line,
            _ => SelectionKind::Char,
        };
        let anchor = GridPoint {
            line: row as i32 - terminal.snapshot().display_offset as i32,
            column,
        };
        terminal.clear_selection();
        terminal.start_selection(anchor, kind);
        self.selecting = Some(kind);
        self.selection_anchor = Some(anchor);
        cx.notify();
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(terminal) = self.terminal.as_ref() else {
            return;
        };
        let Some((column, row)) = self.cell_at(event.position) else {
            return;
        };
        if let Some(kind) = self.selecting {
            let _ = kind;
            let snapshot = terminal.snapshot();
            let head = GridPoint {
                line: row as i32 - snapshot.display_offset as i32,
                column,
            };
            let anchor = self.selection_anchor.unwrap_or(head);
            let forward = (head.line, head.column) >= (anchor.line, anchor.column);
            terminal.update_selection(head, forward);
            cx.notify();
            return;
        }
        let modes = terminal.snapshot().modes;
        if modes.mouse_reporting && (event.pressed_button.is_some() || modes.mouse_motion) {
            let button = event.pressed_button.and_then(button_of);
            terminal.write(&terminal_mouse::motion(
                button,
                mods_of(event.modifiers),
                column,
                row,
                modes.sgr_mouse,
            ));
        }
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(terminal) = self.terminal.as_ref() else {
            return;
        };
        let was_dragging = self.selecting.take().is_some();
        let modes = terminal.snapshot().modes;
        if modes.mouse_reporting
            && let Some((column, row)) = self.cell_at(event.position)
            && let Some(bytes) = terminal_mouse::release(
                Button::Left,
                mods_of(event.modifiers),
                column,
                row,
                modes.sgr_mouse,
            )
        {
            terminal.write(&bytes);
        } else if was_dragging {
            cx.notify();
        }
    }

    /// `⌘C`: the selection if there is one, the child's own interrupt if
    /// not. `⌘V`: the clipboard, bracketed or not as the child prefers.
    fn copy(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(terminal) = self.terminal.as_ref() else {
            return;
        };
        if let Some(text) = terminal.selection_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        } else {
            terminal.write(b"\x03");
        }
    }

    fn paste(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(terminal) = self.terminal.as_ref() else {
            return;
        };
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            terminal.paste(&text);
        }
    }

    /// Text the IME committed, into the child as if typed.
    fn commit(&mut self, text: &str) {
        if let Some(terminal) = self.terminal.as_ref() {
            terminal.write(text.as_bytes());
        }
    }

    fn key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(terminal) = self.terminal.as_ref() else {
            return;
        };
        // A keystroke with the command key belongs to the application's
        // bindings, not the child's input.
        if event.keystroke.modifiers.platform {
            return;
        }
        let Some((key, mods)) = key_input(&event.keystroke) else {
            return;
        };
        let app_cursor = terminal.snapshot().modes.app_cursor;
        if let Some(bytes) = encode(key, mods, app_cursor) {
            terminal.write(&bytes);
            cx.stop_propagation();
        }
    }
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// A keystroke as terminal input. Keys the terminal has no byte for come
/// back as `None` and keep their meaning further out.
fn key_input(keystroke: &Keystroke) -> Option<(Key, Modifiers)> {
    let mods = Modifiers {
        shift: keystroke.modifiers.shift,
        ctrl: keystroke.modifiers.control,
        alt: keystroke.modifiers.alt,
    };
    let key = match keystroke.key.as_str() {
        "enter" => Key::Enter,
        "tab" => Key::Tab,
        "backspace" => Key::Backspace,
        "escape" => Key::Escape,
        "delete" => Key::Delete,
        "insert" => Key::Insert,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "space" => Key::Char(' '),
        f if f.starts_with('f') && f[1..].parse::<u8>().is_ok() => Key::F(f[1..].parse().unwrap()),
        other => Key::Char(
            keystroke
                .key_char
                .as_deref()
                .and_then(|c| c.chars().next())
                .or_else(|| other.chars().next().filter(|_| other.chars().count() == 1))?,
        ),
    };
    Some((key, mods))
}

/// The code font's cell: width from a shaped row of zeros, height from the
/// font's own ascent and descent.
pub(crate) fn measure(font_size: f32, window: &Window) -> (Pixels, Pixels) {
    let run = TextRun {
        len: 10,
        font: plain_font(),
        color: Default::default(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let layout = window
        .text_system()
        .layout_line("0000000000", px(font_size), &[run], None);
    (
        (f32::from(layout.width) / 10.).max(1.).into(),
        (f32::from(layout.ascent + layout.descent))
            .ceil()
            .max(font_size)
            .into(),
    )
}

fn plain_font() -> Font {
    Font {
        family: FONT.into(),
        ..Default::default()
    }
}

/// What one cell paints as. The cursor's colors are part of the style, and
/// the `cursor` flag keeps that one cell a run of its own even when its
/// colors coincide with a neighbor's.
#[derive(PartialEq)]
struct RunStyle {
    fg: Hsla,
    bg: Hsla,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    cursor: bool,
    selected: bool,
}

impl RunStyle {
    fn font(&self) -> Font {
        Font {
            family: FONT.into(),
            weight: if self.bold {
                FontWeight::BOLD
            } else {
                FontWeight::NORMAL
            },
            style: if self.italic {
                FontStyle::Italic
            } else {
                FontStyle::Normal
            },
            ..Default::default()
        }
    }
}

/// One row of the grid as styled text: runs of same-styled cells, with the
/// cursor's cell restyled in place and a beam cursor drawn as a bar beside
/// the text.
fn row_element(
    row: usize,
    snapshot: &terminal::Snapshot,
    colors: &TermColors,
    cursor_visible: bool,
    cell_width: Pixels,
    line_height: Pixels,
) -> AnyElement {
    let start = row * snapshot.columns;
    let cells = &snapshot.cells[start..start + snapshot.columns];
    // The cursor is part of the grid only when the viewport is at the
    // present; scrolled back, the child's cursor is somewhere else.
    let cursor = (cursor_visible && snapshot.display_offset == 0).then_some(&snapshot.cursor);
    // The columns of this row the selection covers, if any.
    let selected = snapshot.selection.and_then(|span| {
        let line = row as i32 - snapshot.display_offset as i32;
        if line < span.start.line || line > span.end.line {
            return None;
        }
        let start = if line == span.start.line {
            span.start.column
        } else {
            0
        };
        let end = if line == span.end.line {
            span.end.column
        } else {
            snapshot.columns - 1
        };
        Some((start.min(end), end.max(start)))
    });

    let mut text = String::with_capacity(snapshot.columns);
    let mut runs: Vec<(RunStyle, usize)> = Vec::new();
    for (column, cell) in cells.iter().enumerate() {
        let mut fg = colors.resolve(cell.fg);
        let mut bg = colors.resolve(cell.bg);
        if cell.attrs.contains(terminal::Attrs::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }
        if cell.attrs.contains(terminal::Attrs::HIDDEN) {
            fg = bg;
        }
        let on_cursor = cursor.is_some_and(|c| c.row == row && c.column == column);
        let is_selected = selected.is_some_and(|(start, end)| column >= start && column <= end);
        // The selection color takes the cell's background; the character
        // keeps its own, which is how an editor draws a selection too.
        if is_selected {
            bg = colors.selection;
        }
        let mut underline = cell.attrs.contains(terminal::Attrs::UNDERLINE);
        if on_cursor {
            // A block fills the cell with the cursor color and paints the
            // character in the background's place; an underline draws under
            // the character as-is; a beam is the bar below.
            match cursor.unwrap().shape {
                terminal::CursorShape::Block => {
                    fg = colors.cursor_text;
                    bg = colors.cursor;
                }
                terminal::CursorShape::Underline => underline = true,
                _ => {}
            }
        }
        let style = RunStyle {
            fg,
            bg,
            bold: cell.attrs.contains(terminal::Attrs::BOLD),
            italic: cell.attrs.contains(terminal::Attrs::ITALIC),
            underline,
            strike: cell.attrs.contains(terminal::Attrs::STRIKETHROUGH),
            cursor: on_cursor,
            selected: is_selected,
        };
        text.push(cell.c);
        let len = cell.c.len_utf8();
        match runs.last_mut() {
            Some((last, count)) if *last == style => *count += len,
            _ => runs.push((style, len)),
        }
    }

    let beam = cursor
        .filter(|c| c.row == row && c.shape == terminal::CursorShape::Beam)
        .map(|c| c.column);
    div()
        .relative()
        .h(line_height)
        .overflow_hidden()
        .when(!text.is_empty(), |el| {
            el.child(
                StyledText::new(text).with_runs(
                    runs.into_iter()
                        .map(|(style, len)| TextRun {
                            len,
                            font: style.font(),
                            color: style.fg,
                            background_color: (style.bg != colors.background).then_some(style.bg),
                            underline: style.underline.then(UnderlineStyle::default),
                            strikethrough: style.strike.then(Default::default),
                        })
                        .collect(),
                ),
            )
        })
        .when_some(beam, |el, column| {
            el.child(
                div()
                    .absolute()
                    .left(px(column as f32 * f32::from(cell_width)))
                    .top_0()
                    .w(px(1.5))
                    .h_full()
                    .bg(colors.cursor),
            )
        })
        .into_any_element()
}

impl Render for TerminalView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let grid = self.terminal.as_ref().map(|terminal| {
            let snapshot = terminal.snapshot();
            let colors = TermColors::new(cx);
            let (cell_width, line_height) = self.metrics;
            let cursor_visible = !self.exited;
            let ime = ImeRegion {
                view: cx.entity(),
                focus: self.focus.clone(),
                bounds: self.grid_bounds.clone(),
            };
            div()
                .flex()
                .flex_col()
                .child(ime)
                .children((0..snapshot.rows).map(|row| {
                    row_element(
                        row,
                        &snapshot,
                        &colors,
                        cursor_visible,
                        cell_width,
                        line_height,
                    )
                }))
        });
        div()
            .id("terminal-view")
            .size_full()
            .key_context("FolioTerminal")
            .track_focus(&self.focus)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_scroll_wheel(cx.listener(Self::wheel))
            .on_key_down(cx.listener(Self::key))
            .on_action(
                cx.listener(|this, _: &crate::TerminalCopy, window, cx| this.copy(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &crate::TerminalPaste, window, cx| this.paste(window, cx)),
            )
            .when_some(self.error.clone(), |el, error| {
                el.child(
                    div()
                        .p_3()
                        .text_size(px(11.))
                        .text_color(cx.theme().muted_foreground)
                        .child(error),
                )
            })
            .when_some(grid, |el, grid| el.child(grid))
    }
}

/// The IME seam. A zero-size element that does nothing but paint: its
/// paint registers the terminal's input handler with the platform — the
/// only phase that may happen in — and notes where the grid sits, which is
/// what turns mouse positions into cells and anchors the candidate window.
struct ImeRegion {
    view: gpui::Entity<TerminalView>,
    focus: FocusHandle,
    bounds: Rc<RefCell<Option<Bounds<Pixels>>>>,
}

impl IntoElement for ImeRegion {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ImeRegion {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(ElementId::Name("terminal-ime".into()))
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (window.request_layout(Style::default(), [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        *self.bounds.borrow_mut() = Some(bounds);
        window.handle_input(
            &self.focus,
            TerminalInputHandler {
                view: self.view.clone(),
            },
            cx,
        );
    }
}

/// The platform's view of the terminal as a text target. Composition
/// updates are held as marked text; a commit goes to the child as if
/// typed. There is no document to edit, so range queries answer nothing
/// and the selection is always the empty range at zero — enough for the
/// candidate window to anchor at the cursor.
struct TerminalInputHandler {
    view: gpui::Entity<TerminalView>,
}

impl InputHandler for TerminalInputHandler {
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut App,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(&mut self, _: &mut Window, cx: &mut App) -> Option<Range<usize>> {
        let marked = self.view.read(cx).marked_text.as_deref()?;
        Some(0..marked.encode_utf16().count())
    }

    fn text_for_range(
        &mut self,
        _: Range<usize>,
        _: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut App,
    ) -> Option<String> {
        None
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut App,
    ) {
        self.view.update(cx, |view, cx| {
            view.marked_text = None;
            view.commit(text);
            cx.notify();
        });
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut App,
    ) {
        self.view.update(cx, |view, cx| {
            view.marked_text = Some(text.to_string());
            cx.notify();
        });
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut App) {
        self.view.update(cx, |view, cx| {
            view.marked_text = None;
            cx.notify();
        });
    }

    /// The candidate window sits where the child's cursor is, as far as
    /// the view knows it: the grid's origin plus the cursor's cell. While
    /// scrolled into history there is no cursor to anchor to.
    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: &mut Window,
        cx: &mut App,
    ) -> Option<Bounds<Pixels>> {
        let view = self.view.read(cx);
        let origin = view.grid_bounds.borrow().as_ref()?.origin;
        let (cell_width, line_height) = view.metrics;
        let snapshot = view.terminal.as_ref()?.snapshot();
        if snapshot.display_offset != 0 {
            return None;
        }
        Some(Bounds {
            origin: origin
                + point(
                    px(snapshot.cursor.column as f32 * f32::from(cell_width)),
                    px(snapshot.cursor.row as f32 * f32::from(line_height)),
                ),
            size: size(cell_width, line_height),
        })
    }

    fn character_index_for_point(
        &mut self,
        _: gpui::Point<Pixels>,
        _: &mut Window,
        _: &mut App,
    ) -> Option<usize> {
        None
    }

    fn apple_press_and_hold_enabled(&mut self) -> bool {
        false
    }
}

fn mods_of(mods: gpui::Modifiers) -> Modifiers {
    Modifiers {
        shift: mods.shift,
        ctrl: mods.control,
        alt: mods.alt,
    }
}

fn button_of(button: MouseButton) -> Option<Button> {
    match button {
        MouseButton::Left => Some(Button::Left),
        MouseButton::Middle => Some(Button::Middle),
        MouseButton::Right => Some(Button::Right),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keystroke(spec: &str, key_char: Option<&str>) -> Keystroke {
        let mut keystroke = Keystroke::parse(spec).unwrap();
        keystroke.key_char = key_char.map(String::from);
        keystroke
    }

    #[test]
    fn named_keys_map_to_their_terminal_keys() {
        let (key, mods) = key_input(&keystroke("up", None)).unwrap();
        assert_eq!(key, Key::Up);
        assert!(!mods.shift && !mods.ctrl && !mods.alt);
        assert_eq!(key_input(&keystroke("enter", None)).unwrap().0, Key::Enter);
        assert_eq!(
            key_input(&keystroke("delete", None)).unwrap().0,
            Key::Delete
        );
        assert_eq!(
            key_input(&keystroke("pageup", None)).unwrap().0,
            Key::PageUp
        );
        assert_eq!(key_input(&keystroke("f5", None)).unwrap().0, Key::F(5));
        assert_eq!(key_input(&keystroke("f12", None)).unwrap().0, Key::F(12));
    }

    #[test]
    fn characters_prefer_the_typed_char() {
        let (key, _) = key_input(&keystroke("s", Some("s"))).unwrap();
        assert_eq!(key, Key::Char('s'));
        // option-s types ß; the terminal wants the ß, not the s.
        let (key, mods) = key_input(&keystroke("alt-s", Some("ß"))).unwrap();
        assert_eq!(key, Key::Char('ß'));
        assert!(mods.alt);
        // A single-character key with no typed char still types itself.
        let (key, _) = key_input(&keystroke("é", None)).unwrap();
        assert_eq!(key, Key::Char('é'));
    }

    #[test]
    fn keys_without_terminal_meaning_are_none() {
        // A multi-character key with no typed char behind it is not the
        // terminal's to take; the named keys above are the whole list.
        assert!(key_input(&keystroke("menu", None)).is_none());
    }
}
