//! The terminal dock's view: a running backend grid painted like Zed's.
//!
//! The backend (`folio::terminal`) owns the pty and the parser; this side
//! owns presentation. Each frame a custom element sizes the pty to the
//! pixels it actually received, then paints cells with the text system —
//! `shape_line` at a cell origin, the way Zed's `TerminalElement` does —
//! instead of flowing `StyledText` through flex. Keys and wheel turns go
//! back to the child as bytes. A background task drains the backend's
//! events every few frames so output arrives while the user works
//! elsewhere.

use std::{cell::RefCell, ops::Range, path::Path, rc::Rc, time::Duration};

use folio::terminal::{self, Color, GridPoint, SelectionKind, Terminal, TerminalEvent};
use folio::terminal_keys::{Key, Modifiers, encode};
use folio::terminal_mouse::{self, Button};
use gpui::{
    App, Bounds, ClipboardItem, ContentMask, Context, Element, ElementId, Entity, FocusHandle,
    Focusable, Font, FontStyle, FontWeight, GlobalElementId, Hsla, InputHandler,
    InspectorElementId, IntoElement, KeyDownEvent, Keystroke, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Overflow, ParentElement, Pixels, Point, Render,
    ScrollWheelEvent, SharedString, Style, Styled, TextAlign, TextRun, UTF16Selection,
    UnderlineStyle, Window, div, fill, point, prelude::*, px, relative, rgb, size,
};
use gpui_component::ActiveTheme;

/// Fallback when the theme has not set a mono family.
const FONT: &str = "JetBrains Mono";

/// Zed's default terminal line height: font size times this.
const LINE_HEIGHT: f32 = 1.3;

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
    /// The cell metrics the last paint measured, for the wheel and for
    /// turning a mouse position into a cell.
    metrics: (Pixels, Pixels),
    /// The selection a drag is building, as its kind; the anchor lives in
    /// the grid and the head follows the mouse.
    selecting: Option<SelectionKind>,
    /// Where the drag started, so the head's side is known.
    selection_anchor: Option<GridPoint>,
    /// Where the grid painted; mouse positions become cells against this.
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

    /// The cell the child's cursor sits in, in window coordinates. IME
    /// candidate windows park here — the whole grid would put them on
    /// the first line, away from what the user types.
    fn cursor_cell_bounds(&self) -> Option<Bounds<Pixels>> {
        let origin = self.grid_bounds.borrow().as_ref()?.origin;
        let (cell_width, line_height) = self.metrics;
        let snapshot = self.terminal.as_ref()?.snapshot();
        if snapshot.display_offset != 0 {
            return None;
        }
        let marked = self
            .marked_text
            .as_deref()
            .filter(|text| !text.is_empty())
            .map(|text| text.chars().count())
            .unwrap_or(1)
            .max(1);
        Some(Bounds {
            origin: origin
                + point(
                    px(snapshot.cursor.column as f32 * f32::from(cell_width)),
                    px(snapshot.cursor.row as f32 * f32::from(line_height)),
                ),
            size: size(cell_width * marked as f32, line_height),
        })
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
        if self.selecting.is_some() {
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

    /// Whether the IME is composing. An empty mark is not composing —
    /// leaving it set would keep macOS sending every later key, so
    /// nothing reaches the child and the caret has nowhere to sit.
    fn composing(&self) -> bool {
        marked_is_composing(self.marked_text.as_deref())
    }

    fn key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(terminal) = self.terminal.as_ref() else {
            return;
        };
        // A keystroke with the command key belongs to the application's
        // bindings, not the child's input. While the IME is composing,
        // the platform already has the key.
        if event.keystroke.modifiers.platform {
            return;
        }
        if self.composing() {
            cx.stop_propagation();
            return;
        }
        let Some((key, mods)) = key_input(&event.keystroke) else {
            return;
        };
        let app_cursor = terminal.snapshot().modes.app_cursor;
        if let Some(bytes) = encode(key, mods, app_cursor) {
            terminal.write(&bytes);
            cx.stop_propagation();
            cx.notify();
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

fn grid_cells(span: Pixels, cell: Pixels) -> usize {
    let cell = f32::from(cell).max(1.);
    ((f32::from(span) / cell).floor() as usize).clamp(2, 500)
}

fn cell_font(cx: &App) -> Font {
    let family = cx.theme().mono_font_family.clone();
    Font {
        family: if family.is_empty() {
            FONT.into()
        } else {
            family
        },
        ..Default::default()
    }
}

/// Cell width from the advance of `m`, line height from the font size —
/// the same arithmetic Zed's `TerminalElement` uses.
fn cell_metrics(cx: &App) -> (Pixels, Pixels, Pixels) {
    let font_size = cx.theme().mono_font_size;
    let font_id = cx.text_system().resolve_font(&cell_font(cx));
    let cell_width = cx
        .text_system()
        .advance(font_id, font_size, 'm')
        .map(|advance| advance.width)
        .unwrap_or(px(8.))
        .max(px(1.));
    let line_height = px((f32::from(font_size) * LINE_HEIGHT)
        .ceil()
        .max(f32::from(font_size)));
    (cell_width, line_height, font_size)
}

impl Render for TerminalView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("terminal-view")
            .size_full()
            .overflow_hidden()
            .bg(cx.theme().background)
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
            .when(self.terminal.is_some(), |el| {
                el.child(TerminalGrid {
                    view: cx.entity(),
                    focus: self.focus.clone(),
                })
            })
    }
}

/// The grid itself, painted the way Zed paints a terminal: one shaped
/// run per stretch of same-styled cells, placed on the cell grid, with
/// the IME handler registered in `paint` so the platform has a place
/// for the caret.
struct TerminalGrid {
    view: Entity<TerminalView>,
    focus: FocusHandle,
}

struct GridPaint {
    origin: Point<Pixels>,
    cell_width: Pixels,
    line_height: Pixels,
    font_size: Pixels,
}

impl IntoElement for TerminalGrid {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TerminalGrid {
    type RequestLayoutState = ();
    type PrepaintState = Option<GridPaint>;

    fn id(&self) -> Option<ElementId> {
        Some(ElementId::Name("terminal-grid".into()))
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
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        style.overflow.x = Overflow::Hidden;
        style.overflow.y = Overflow::Hidden;
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        if bounds.size.width < px(1.) || bounds.size.height < px(1.) {
            return None;
        }
        let (cell_width, line_height, font_size) = cell_metrics(cx);
        let columns = grid_cells(bounds.size.width, cell_width);
        let rows = grid_cells(bounds.size.height, line_height);
        self.view.update(cx, |view, cx| {
            *view.grid_bounds.borrow_mut() = Some(bounds);
            view.metrics = (cell_width, line_height);
            let Some(terminal) = view.terminal.as_mut() else {
                return;
            };
            if terminal.size() != (columns, rows) {
                terminal.resize(
                    columns,
                    rows,
                    f32::from(cell_width) as u16,
                    f32::from(line_height) as u16,
                );
                cx.notify();
            }
        });
        Some(GridPaint {
            origin: bounds.origin,
            cell_width,
            line_height,
            font_size,
        })
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        layout: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(layout) = layout.as_ref() else {
            return;
        };
        let colors = TermColors::new(cx);
        let (snapshot, marked, cursor_visible) = {
            let view = self.view.read(cx);
            let Some(terminal) = view.terminal.as_ref() else {
                return;
            };
            (
                terminal.snapshot(),
                view.marked_text
                    .as_deref()
                    .filter(|text| !text.is_empty())
                    .map(str::to_string),
                !view.exited,
            )
        };

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_quad(fill(bounds, colors.background));
            paint_grid(
                layout,
                &snapshot,
                &colors,
                cell_font(cx),
                cursor_visible,
                marked.as_deref(),
                window,
                cx,
            );
        });

        window.handle_input(
            &self.focus,
            TerminalInputHandler {
                view: self.view.clone(),
            },
            cx,
        );
    }
}

/// One stretch of same-styled cells, painted with `shape_line` at a
/// cell origin the way Zed's `BatchedTextRun` is.
struct TextBatch {
    row: usize,
    column: usize,
    text: String,
    run: TextRun,
}

/// A run of cells that share a background other than the grid's.
struct BgRect {
    row: usize,
    column: usize,
    cells: usize,
    color: Hsla,
}

fn paint_grid(
    layout: &GridPaint,
    snapshot: &terminal::Snapshot,
    colors: &TermColors,
    font: Font,
    cursor_visible: bool,
    marked: Option<&str>,
    window: &mut Window,
    cx: &mut App,
) {
    let (rects, batches) =
        layout_cells(snapshot, colors, &font, cursor_visible && marked.is_none());
    for rect in &rects {
        let origin = cell_origin(layout, rect.row, rect.column);
        window.paint_quad(fill(
            Bounds {
                origin,
                size: size(layout.cell_width * rect.cells as f32, layout.line_height),
            },
            rect.color,
        ));
    }
    for batch in &batches {
        let origin = cell_origin(layout, batch.row, batch.column);
        let line = window.text_system().shape_line(
            SharedString::from(batch.text.clone()),
            layout.font_size,
            std::slice::from_ref(&batch.run),
            Some(layout.cell_width),
        );
        let _ = line.paint(
            origin,
            layout.line_height,
            TextAlign::Left,
            None,
            window,
            cx,
        );
    }

    // IME preedit sits on the cursor, covering whatever the grid has
    // there, the way Zed paints `marked_text` after the cell runs.
    if let Some(text) = marked.filter(|text| !text.is_empty())
        && snapshot.display_offset == 0
    {
        let origin = cell_origin(layout, snapshot.cursor.row, snapshot.cursor.column);
        let underline = Some(UnderlineStyle {
            color: Some(colors.foreground),
            thickness: px(1.),
            wavy: false,
        });
        let line = window.text_system().shape_line(
            SharedString::from(text.to_string()),
            layout.font_size,
            &[TextRun {
                len: text.len(),
                font: font.clone(),
                color: colors.foreground,
                background_color: Some(colors.background),
                underline,
                strikethrough: None,
            }],
            Some(layout.cell_width),
        );
        window.paint_quad(fill(
            Bounds {
                origin,
                size: size(line.width.max(layout.cell_width), layout.line_height),
            },
            colors.background,
        ));
        let _ = line.paint(
            origin,
            layout.line_height,
            TextAlign::Left,
            None,
            window,
            cx,
        );
        return;
    }

    if cursor_visible && snapshot.display_offset == 0 {
        paint_cursor(layout, snapshot, colors, window);
    }
}

fn paint_cursor(
    layout: &GridPaint,
    snapshot: &terminal::Snapshot,
    colors: &TermColors,
    window: &mut Window,
) {
    if snapshot.cursor.shape == terminal::CursorShape::Hidden {
        return;
    }
    let origin = cell_origin(layout, snapshot.cursor.row, snapshot.cursor.column);
    match snapshot.cursor.shape {
        // A block is already the cell's background and inverse glyph,
        // painted with the other runs. Drawing it again here would cover
        // the character the user just typed.
        terminal::CursorShape::Block | terminal::CursorShape::Hidden => {}
        terminal::CursorShape::Beam => {
            window.paint_quad(fill(
                Bounds {
                    origin,
                    size: size(px(1.5), layout.line_height),
                },
                colors.cursor,
            ));
        }
        terminal::CursorShape::Underline => {
            window.paint_quad(fill(
                Bounds {
                    origin: origin + point(px(0.), layout.line_height - px(2.)),
                    size: size(layout.cell_width, px(2.)),
                },
                colors.cursor,
            ));
        }
    }
}

fn cell_origin(layout: &GridPaint, row: usize, column: usize) -> Point<Pixels> {
    layout.origin
        + point(
            px(column as f32 * f32::from(layout.cell_width)),
            px(row as f32 * f32::from(layout.line_height)),
        )
}

fn layout_cells(
    snapshot: &terminal::Snapshot,
    colors: &TermColors,
    font: &Font,
    cursor_visible: bool,
) -> (Vec<BgRect>, Vec<TextBatch>) {
    let cursor = (cursor_visible && snapshot.display_offset == 0).then_some(&snapshot.cursor);
    let mut rects: Vec<BgRect> = Vec::new();
    let mut batches: Vec<TextBatch> = Vec::new();
    for row in 0..snapshot.rows {
        let start = row * snapshot.columns;
        let cells = &snapshot.cells[start..start + snapshot.columns];
        let selected = selection_on_row(snapshot, row);
        for (column, cell) in cells.iter().enumerate() {
            if cell.attrs.contains(terminal::Attrs::WIDE_CHAR_SPACER) {
                continue;
            }
            let mut fg = colors.resolve(cell.fg);
            let mut bg = colors.resolve(cell.bg);
            if cell.attrs.contains(terminal::Attrs::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cell.attrs.contains(terminal::Attrs::HIDDEN) {
                fg = bg;
            }
            let on_cursor = cursor.is_some_and(|c| {
                c.shape == terminal::CursorShape::Block && c.row == row && c.column == column
            });
            let is_selected = selected.is_some_and(|(start, end)| column >= start && column <= end);
            if is_selected {
                bg = colors.selection;
            }
            if on_cursor {
                fg = colors.cursor_text;
                bg = colors.cursor;
            }
            if bg != colors.background {
                match rects.last_mut() {
                    Some(last)
                        if last.row == row
                            && last.column + last.cells == column
                            && last.color == bg =>
                    {
                        last.cells += 1;
                    }
                    _ => rects.push(BgRect {
                        row,
                        column,
                        cells: 1,
                        color: bg,
                    }),
                }
            }
            if cell.c == ' '
                && !cell.attrs.contains(terminal::Attrs::UNDERLINE)
                && !cell.attrs.contains(terminal::Attrs::STRIKETHROUGH)
                && !on_cursor
            {
                continue;
            }
            let font = Font {
                family: font.family.clone(),
                weight: if cell.attrs.contains(terminal::Attrs::BOLD) {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                },
                style: if cell.attrs.contains(terminal::Attrs::ITALIC) {
                    FontStyle::Italic
                } else {
                    FontStyle::Normal
                },
                ..Default::default()
            };
            let run = TextRun {
                len: cell.c.len_utf8(),
                font,
                color: if on_cursor { colors.cursor_text } else { fg },
                background_color: None,
                underline: cell
                    .attrs
                    .contains(terminal::Attrs::UNDERLINE)
                    .then(UnderlineStyle::default),
                strikethrough: cell
                    .attrs
                    .contains(terminal::Attrs::STRIKETHROUGH)
                    .then(Default::default),
            };
            match batches.last_mut() {
                Some(last)
                    if last.row == row
                        && last.column + last.text.chars().count() == column
                        && same_run(&last.run, &run) =>
                {
                    last.text.push(cell.c);
                    last.run.len += cell.c.len_utf8();
                }
                _ => batches.push(TextBatch {
                    row,
                    column,
                    text: cell.c.to_string(),
                    run,
                }),
            }
        }
    }
    (rects, batches)
}

fn same_run(left: &TextRun, right: &TextRun) -> bool {
    left.font == right.font
        && left.color == right.color
        && left.background_color == right.background_color
        && left.underline == right.underline
        && left.strikethrough == right.strikethrough
}

fn selection_on_row(snapshot: &terminal::Snapshot, row: usize) -> Option<(usize, usize)> {
    let span = snapshot.selection?;
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
}

/// The platform's view of the terminal as a text target. Composition
/// updates are held as marked text; a commit goes to the child as if
/// typed. There is no document to edit, so range queries answer nothing
/// and the selection is always the empty range at zero — enough for the
/// candidate window to anchor at the cursor.
struct TerminalInputHandler {
    view: Entity<TerminalView>,
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
        let marked = self
            .view
            .read(cx)
            .marked_text
            .as_deref()
            .filter(|text| !text.is_empty())?;
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
        window: &mut Window,
        cx: &mut App,
    ) {
        self.view.update(cx, |view, cx| {
            view.marked_text = None;
            view.commit(text);
            cx.notify();
        });
        window.invalidate_character_coordinates();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.view.update(cx, |view, cx| {
            view.marked_text = (!text.is_empty()).then(|| text.to_string());
            cx.notify();
        });
        window.invalidate_character_coordinates();
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut App) {
        self.view.update(cx, |view, cx| {
            view.marked_text = None;
            cx.notify();
        });
        window.invalidate_character_coordinates();
    }

    fn element_bounds(&mut self, _: &mut Window, cx: &mut App) -> Option<Bounds<Pixels>> {
        self.view.read(cx).cursor_cell_bounds()
    }

    fn prefers_ime_for_printable_keys(&mut self, _: &mut Window, _: &mut App) -> bool {
        // Zed's terminal keeps the default: printable keys reach the
        // child so the shell echoes them. Composition still arrives
        // through `replace_and_mark_text_in_range` when an IME is
        // actually composing.
        false
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: &mut Window,
        cx: &mut App,
    ) -> Option<Bounds<Pixels>> {
        self.view.read(cx).cursor_cell_bounds()
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

    #[test]
    fn empty_ime_mark_is_not_composing() {
        // macOS ends a composition by marking the empty string. Treating
        // that as still composing would swallow every later key.
        assert!(!marked_is_composing(None));
        assert!(!marked_is_composing(Some("")));
        assert!(marked_is_composing(Some("ni")));
    }

    #[test]
    fn grid_cells_never_overrun_the_span() {
        assert_eq!(grid_cells(px(100.), px(16.)), 6);
        assert_eq!(grid_cells(px(16.), px(16.)), 2);
        assert_eq!(grid_cells(px(15.), px(16.)), 2);
    }
}

fn marked_is_composing(marked: Option<&str>) -> bool {
    marked.is_some_and(|text| !text.is_empty())
}
