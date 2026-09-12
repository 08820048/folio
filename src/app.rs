use crate::assets::FolioIcon;
use crate::preview::{self, Content};
use crate::terminal_view::TerminalView;
use folio::{
    blame, buffer, diff, editorconfig, fs_op, git,
    recent::{self, RecentProject},
    search, session,
    settings::{self, Settings},
    tree::{self, Entry, EntryKind},
    watch,
    workspace::Workspace,
};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, Root, Sizable, Theme, TitleBar,
    button::{Button, ButtonVariants},
    input::{self, EditorState, Input, InputBaseState, InputEvent, InputState, Position, TabSize},
    native_menu::NativeMenu,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Where the versions of this application are listed.
const RELEASES: &str = "https://github.com/08820048/folio/releases";

/// Fonts tried for the glyphs the code font has no coverage for. GPUI's own
/// fallback stack names no CJK family at all, so without this a Chinese
/// character in a file is drawn in whatever the platform happens to pick —
/// usually a proportional face, which breaks the character grid. The mono CJK
/// families come first for that reason; the rest are platform defaults.
const CJK_FALLBACKS: &[&str] = &[
    "Sarasa Mono SC",
    "Sarasa Term SC",
    "Source Han Mono SC",
    "Noto Sans Mono CJK SC",
    "Noto Sans Mono CJK TC",
    "PingFang SC",
    "Hiragino Sans GB",
    "Microsoft YaHei",
    "Noto Sans CJK SC",
];

/// How tall the tab strip is. Thin on purpose: it takes its row from the code
/// area, so every pixel it keeps is a pixel the code does not get.
const TAB_HEIGHT: f32 = 26.;

/// How wide a text field in the settings is. Wide enough for a font name or a
/// short list of ignored folders, and the same everywhere so the fields line
/// up down the right-hand side.
const FIELD_WIDTH: f32 = 240.;

/// What the editor's right-click menu needs to know about the editor, copied
/// out of the component's capabilities one frame before the menu is asked for.
/// The component's own type is not nameable from here, and this is the whole
/// of what the menu reads from it.
#[derive(Clone, Copy)]
struct EditorMenuState {
    enabled: bool,
    /// Enabled and not read-only: a read-only editor can still be navigated
    /// and copied out of, it only rejects what would change the text.
    editable: bool,
    code_editor: bool,
    has_selection: bool,
    can_go_to_definition: bool,
    has_code_actions: bool,
}

/// The editor's right-click menu: the standard text actions, then the way into
/// the changes view.
///
/// This replaces the component's own menu rather than adding to it. The
/// component sets its handler on every render and offers no way to extend it,
/// so the standard entries are rebuilt here, disabled under the same
/// conditions. Dropping them would cost Cut, Copy and Paste on right-click,
/// which is a poor trade for one added row.
///
/// That state arrives as an argument rather than being read here, because this
/// runs inside the editor's own update — that is how the component schedules
/// it — and reading the entity from there is a re-entrant borrow. It is read
/// one frame earlier instead, and anything that moves it repaints.
fn editor_context_menu(
    editor: EditorMenuState,
    mut menu: NativeMenu,
    _: &mut Window,
    cx: &mut App,
) -> NativeMenu {
    if editor.code_editor {
        menu = menu
            .menu_with_disabled(
                "Go to Definition",
                !(editor.enabled && editor.can_go_to_definition),
                Box::new(input::GoToDefinition),
            )
            .menu_with_disabled(
                "Show Code Actions",
                !(editor.editable && editor.has_code_actions),
                Box::new(input::ToggleCodeActions),
            )
            .separator();
    }
    menu.menu_with_disabled(
        "Cut",
        !(editor.editable && editor.has_selection),
        Box::new(input::Cut),
    )
    .menu_with_disabled("Copy", !editor.has_selection, Box::new(input::Copy))
    .menu_with_disabled(
        "Paste",
        !(editor.editable && cx.read_from_clipboard().is_some()),
        Box::new(input::Paste),
    )
    .separator()
    .menu("Select All", Box::new(input::SelectAll))
    .separator()
    // The editor is only on screen while the changes view is off, so this
    // only ever shows one way.
    .menu("Show File Changes", Box::new(ToggleDiff))
}

/// The far-left icon rail. Narrow on purpose: icons only, no labels.
/// The rail plus the gap before the card must stay at or under 40px.
const ACTIVITY_BAR_WIDTH: f32 = 36.;
/// Space between the rail and the rounded card. `ACTIVITY_BAR_WIDTH` +
/// this is the whole left chrome strip.
const WORKSPACE_RAIL_GAP: f32 = 4.;
const _: () = assert!((ACTIVITY_BAR_WIDTH + WORKSPACE_RAIL_GAP) as i32 <= 40);
/// Space between the window chrome and the rounded workspace card.
const WORKSPACE_INSET: f32 = 8.;
/// Corner radius of the workspace card. Children that paint their own
/// background have to use the same number, or GPUI leaves the parent's
/// cut-out as a grey triangle — overflow clip is a rectangle, not an arc.
const WORKSPACE_RADIUS: f32 = 12.;
/// File-tree row height. `uniform_list` needs a fixed size.
const TREE_ROW_HEIGHT: f32 = 27.;
/// Indent of a tree row, matching gpui-component's Tree: `16 * depth + 12`.
const TREE_ROW_INDENT: f32 = 16.;
const TREE_ROW_PAD: f32 = 12.;

/// The frame the rounded workspace sits on, so the white card reads as a
/// surface rather than disappearing into the window.
fn workspace_chrome(cx: &App) -> Hsla {
    rgb(if cx.theme().is_dark() {
        0x141516
    } else {
        0xFDFDFD
    })
    .into()
}

/// The rule between the file tree and the editor. One pixel, and paler
/// than the card's own border, so it does not read as a second frame.
fn sidebar_rule(cx: &App) -> Hsla {
    rgb(if cx.theme().is_dark() {
        0x222426
    } else {
        0xF0F0F0
    })
    .into()
}

/// Left padding of a file-tree row. Same formula as gpui-component's Tree.
fn tree_row_indent(depth: usize) -> Pixels {
    px(TREE_ROW_PAD + depth as f32 * TREE_ROW_INDENT)
}

/// Horizontal position of the indent guide for nesting `level`.
/// Centered in that level's gutter so the line sits under the parent's icon.
fn tree_guide_x(level: usize) -> Pixels {
    px(TREE_ROW_PAD + TREE_ROW_INDENT / 2. + level as f32 * TREE_ROW_INDENT)
}

/// The guide that marks the folder containing `index`: first child row,
/// end (exclusive), and indent level. The parent row itself does not
/// carry this line — same as Zed.
fn tree_active_guide(rows: &[Row], index: usize) -> Option<(usize, usize, usize)> {
    let depth = rows.get(index)?.depth;
    if depth == 0 {
        return None;
    }
    let level = depth - 1;
    let parent = (0..=index).rev().find(|&i| rows[i].depth == level)?;
    let mut end = parent + 1;
    while end < rows.len() && rows[end].depth > level {
        end += 1;
    }
    Some((parent + 1, end, level))
}

fn tree_guide_elements(
    rows: &[Row],
    index: usize,
    depth: usize,
    focus: Option<usize>,
    cx: &App,
) -> Vec<AnyElement> {
    let active = focus.and_then(|ix| tree_active_guide(rows, ix));
    (0..depth)
        .map(|level| {
            let is_active = active
                .is_some_and(|(start, end, guide)| guide == level && index >= start && index < end);
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(tree_guide_x(level))
                .w(px(1.))
                .bg(cx
                    .theme()
                    .muted_foreground
                    .opacity(if is_active { 0.5 } else { 0.22 }))
                .into_any_element()
        })
        .collect()
}

#[cfg(test)]
mod tree_guide_tests {
    use super::*;

    fn row_at(depth: usize) -> Row {
        Row {
            entry: Entry {
                path: PathBuf::from(format!("/{depth}")),
                name: depth.to_string(),
                kind: EntryKind::File,
            },
            depth,
        }
    }

    #[core::prelude::v1::test]
    fn tree_active_guide_follows_the_containing_folder() {
        // src/          0
        //   app.rs      1
        //   tree/       1
        //     mod.rs    2
        //     walk.rs   2
        //   lib.rs      1
        // README        0
        let rows: Vec<Row> = [0, 1, 1, 2, 2, 1, 0].into_iter().map(row_at).collect();
        assert_eq!(tree_active_guide(&rows, 0), None);
        assert_eq!(tree_active_guide(&rows, 6), None);
        assert_eq!(tree_active_guide(&rows, 1), Some((1, 6, 0)));
        assert_eq!(tree_active_guide(&rows, 2), Some((1, 6, 0)));
        assert_eq!(tree_active_guide(&rows, 3), Some((3, 5, 1)));
        assert_eq!(tree_active_guide(&rows, 4), Some((3, 5, 1)));
        assert_eq!(tree_active_guide(&rows, 5), Some((1, 6, 0)));
        assert_eq!(tree_guide_x(0), px(TREE_ROW_PAD + TREE_ROW_INDENT / 2.));
        assert_eq!(
            tree_guide_x(1),
            px(TREE_ROW_PAD + TREE_ROW_INDENT / 2. + TREE_ROW_INDENT)
        );
    }
}

/// A type size in the interface scale. The design is drawn at 13px, so
/// `ui(11.)` is the 11px label from the spec, and it grows with the interface
/// size setting instead of staying pinned.
fn ui(size: f32) -> Rems {
    // The argument is a pixel size, not a ratio. Passing one here silently
    // divides it twice and renders the whole interface at a fraction of a
    // pixel, which is hard to spot and easy to do.
    debug_assert!(
        (8. ..=64.).contains(&size),
        "ui({size}) is not a size in the 13px design scale"
    );
    rems(size / 13.)
}

/// Apply the theme. `window` is the window being refreshed, when the caller
/// has one — the settings window changes the theme for every window, so it
/// passes `None` and refreshes them itself.
fn sync_appearance(
    appearance: WindowAppearance,
    settings: &Settings,
    mut window: Option<&mut Window>,
    cx: &mut App,
) {
    Theme::change(appearance, window.as_deref_mut(), cx);
    let theme = Theme::global_mut(cx);
    let dark = theme.is_dark();
    theme.mono_font_family = settings
        .code_font_family
        .clone()
        .unwrap_or_else(|| "JetBrains Mono".to_string())
        .into();
    theme.mono_font_size = px(settings.code_font_size);
    theme.font_size = px(settings.font_size);
    // `Theme::change` has already put its own family back, so leaving this
    // alone is what "system default" means.
    if let Some(family) = settings.font_family.clone() {
        theme.font_family = family.into();
    }
    theme.background = rgb(if dark { 0x181A1C } else { 0xFFFFFF }).into();
    theme.foreground = rgb(if dark { 0xDCDDD8 } else { 0x282D2B }).into();
    theme.sidebar = rgb(if dark { 0x1D1F21 } else { 0xFFFFFF }).into();
    theme.popover = theme.sidebar;
    theme.muted_foreground = rgb(if dark { 0x929792 } else { 0x626A64 }).into();
    theme.border = rgb(if dark { 0x2B2E30 } else { 0xE8E8E8 }).into();
    theme.accent_foreground = rgb(if dark { 0xBECBAD } else { 0x4C6341 }).into();
    theme.list_active = rgb(if dark { 0x2B3031 } else { 0xEFEFEF }).into();
    theme.list_active_border = rgb(if dark { 0x3A4042 } else { 0xE0E0E0 }).into();
    theme.list_hover = rgb(if dark { 0x25292B } else { 0xF5F5F5 }).into();
    theme.title_bar = theme.background;
    theme.title_bar_border = theme.border;
    let background = theme.background;
    let foreground = theme.foreground;
    let muted = theme.muted_foreground;
    let highlight = std::sync::Arc::make_mut(&mut theme.highlight_theme);
    highlight.style.editor_background = Some(background);
    highlight.style.editor_foreground = Some(foreground);
    highlight.style.editor_gutter_background = Some(background);
    highlight.style.editor_active_line = Some(rgb(if dark { 0x212628 } else { 0xF5F5F5 }).into());
    highlight.style.editor_line_number = Some(muted);
    if let Some(window) = window {
        window.refresh();
    }
}

actions!(
    folio,
    [
        OpenProject,
        CloseProject,
        Save,
        QuickOpen,
        ProjectSearch,
        ProjectReplace,
        GoToLine,
        ToggleSidebar,
        ToggleActivityBar,
        ToggleTerminal,
        TerminalCopy,
        TerminalPaste,
        CloseTerminal,
        OpenSettings,
        CloseWindow,
        OpenAbout,
        ToggleDiff,
        ToggleBlame,
        CheckForUpdates,
        ToggleComment,
        ToggleBlockComment,
        DuplicateLines,
        MoveLinesUp,
        MoveLinesDown,
        Fold,
        Unfold,
        SelectNextOccurrence,
        SelectAllOccurrences,
        AddCursorAbove,
        AddCursorBelow,
        SelectColumnUp,
        SelectColumnDown,
        PairParen,
        PairBracket,
        PairBrace,
        PairQuote,
        PairApostrophe,
        PairBacktick,
        SkipParen,
        SkipBracket,
        SkipBrace,
        Quit
    ]
);

#[derive(Clone)]
enum Next {
    Picker,
    Open(PathBuf),
    Close,
    /// Close these open files. One for a single tab; several for Close
    /// Others and its neighbours.
    CloseTabs(Vec<PathBuf>),
    Quit,
}
enum RecentAction {
    Load,
    Open(PathBuf),
    Remove(PathBuf),
}

struct Document {
    editor: Entity<EditorState>,
    saved: SharedString,
    dirty: bool,
    large: bool,
    _subscription: Subscription,
}
#[derive(Clone)]
struct Row {
    entry: Entry,
    depth: usize,
}

/// Which lookup the overlay panel is running.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Panel {
    /// `⌘P`: fuzzy file-name search.
    Files,
    /// `⇧⌘F`: project-wide content search, optionally with replace.
    Search,
}

/// Every entry a right-click menu can offer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuItem {
    // The project tree.
    NewFile,
    NewFolder,
    Reveal,
    OpenDefault,
    OpenTerminal,
    FindInFolder,
    Cut,
    Copy,
    Duplicate,
    Paste,
    Rename,
    Trash,
    Delete,
    // The changes view.
    HideChanges,
    // The tab strip.
    CloseTab,
    CloseOthers,
    CloseLeft,
    CloseRight,
    CloseClean,
    CloseAll,
    ToggleReadOnly,
    CopyPath,
    CopyRelativePath,
    PinTab,
    RevealInTree,
}

/// What a right-click opened over. Which surface it is decides which entries
/// the menu offers, and what each entry acts on.
#[derive(Clone, Debug, PartialEq, Eq)]
enum MenuTarget {
    /// A folder in the project tree.
    Tree { path: PathBuf, root: bool },
    /// A file shown as its changes against HEAD.
    Changes { path: PathBuf },
    /// A tab in the strip, which is not necessarily the active one.
    Tab { path: PathBuf, index: usize },
}

impl MenuTarget {
    /// The file the menu acts on.
    fn path(&self) -> &Path {
        match self {
            MenuTarget::Tree { path, .. }
            | MenuTarget::Changes { path }
            | MenuTarget::Tab { path, .. } => path,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Surface {
    Tree,
    Changes,
    Tab,
}

/// `(item, label, shortcut)`. One table per surface, so each one's separators
/// are its own and an entry can be named for what it does where it appears.
/// The shortcut is spelled the way macOS writes it and rewritten for other
/// platforms by [`shortcut_label`]; it is empty where the action has no
/// single-key form, which is most of the tab strip — Zed reaches several of
/// those through a `⌘K` prefix this menu cannot express.
const TREE_MENU: &[(MenuItem, &str, &str)] = &[
    (MenuItem::NewFile, "New File", "⌘N"),
    (MenuItem::NewFolder, "New Folder", "⌘⇧N"),
    (MenuItem::Reveal, "Reveal in Finder", "⌘⇧R"),
    (MenuItem::OpenDefault, "Open in Default App", ""),
    (MenuItem::OpenTerminal, "Open in Terminal", ""),
    (MenuItem::FindInFolder, "Find in Folder…", "⌘⇧F"),
    (MenuItem::Cut, "Cut", "⌘X"),
    (MenuItem::Copy, "Copy", "⌘C"),
    (MenuItem::Duplicate, "Duplicate", "⌘D"),
    (MenuItem::Paste, "Paste", "⌘V"),
    (MenuItem::Rename, "Rename", "⇧R"),
    // The two Finder bindings, so the destructive one carries the extra key.
    (MenuItem::Trash, "Move to Trash", "⌘⌫"),
    (MenuItem::Delete, "Delete Immediately", "⌥⌘⌫"),
];
const TREE_SEPARATORS: &[usize] = &[2, 5, 6, 10, 11];

const CHANGES_MENU: &[(MenuItem, &str, &str)] =
    &[(MenuItem::HideChanges, "Hide File Changes", "⌘⇧D")];
const CHANGES_SEPARATORS: &[usize] = &[];

const TAB_MENU: &[(MenuItem, &str, &str)] = &[
    (MenuItem::CloseTab, "Close", ""),
    (MenuItem::CloseOthers, "Close Others", "⌥⌘T"),
    (MenuItem::CloseLeft, "Close Left", ""),
    (MenuItem::CloseRight, "Close Right", ""),
    (MenuItem::CloseClean, "Close Clean", ""),
    (MenuItem::CloseAll, "Close All", ""),
    (MenuItem::ToggleReadOnly, "Make Tab Read-Only", ""),
    (MenuItem::CopyPath, "Copy Path", "⌥⌘C"),
    (MenuItem::CopyRelativePath, "Copy Relative Path", "⌥⌘⇧C"),
    (MenuItem::Reveal, "Reveal in Finder", "⌘⇧R"),
    (MenuItem::PinTab, "Pin Tab", ""),
    (MenuItem::RevealInTree, "Reveal In Project Panel", ""),
    (MenuItem::OpenTerminal, "Open in Terminal", ""),
];
const TAB_SEPARATORS: &[usize] = &[2, 4, 6, 7, 9, 10];

impl Surface {
    /// This surface's entries, and where the rules between them go.
    fn menu(
        self,
    ) -> (
        &'static [(MenuItem, &'static str, &'static str)],
        &'static [usize],
    ) {
        match self {
            Surface::Tree => (TREE_MENU, TREE_SEPARATORS),
            Surface::Changes => (CHANGES_MENU, CHANGES_SEPARATORS),
            Surface::Tab => (TAB_MENU, TAB_SEPARATORS),
        }
    }
}

fn surface_of(target: &MenuTarget) -> Surface {
    match target {
        MenuTarget::Tree { .. } => Surface::Tree,
        MenuTarget::Changes { .. } => Surface::Changes,
        MenuTarget::Tab { .. } => Surface::Tab,
    }
}

impl MenuItem {
    /// The project root is the workspace's identity: `project_order`, the
    /// recent list and every cached path key off it, so the tree offers no way
    /// to rename or remove it.
    fn applies_to_root(self) -> bool {
        !matches!(self, MenuItem::Rename | MenuItem::Trash | MenuItem::Delete)
    }
}

/// The menu is a fixed grid so its height can be measured before it is built.
const MENU_WIDTH: f32 = 228.;
const MENU_ROW: f32 = 26.;

/// The indices into the target's own menu that it shows. The project root
/// drops the entries that would rename or remove it.
fn visible_menu_items(target: &MenuTarget) -> Vec<usize> {
    let root = matches!(target, MenuTarget::Tree { root: true, .. });
    surface_of(target)
        .menu()
        .0
        .iter()
        .enumerate()
        .filter(|(_, (item, _, _))| !root || item.applies_to_root())
        .map(|(index, _)| index)
        .collect()
}

fn shortcut_label(shortcut: &str) -> String {
    if cfg!(target_os = "macos") || shortcut.is_empty() {
        shortcut.to_string()
    } else {
        shortcut
            .replace('⌘', "Ctrl+")
            .replace('⇧', "Shift+")
            .replace('⌥', "Alt+")
            .replace('⌫', "Backspace")
            .replace('⌦', "Delete")
    }
}

/// The `KeyDownEvent` key name for the glyph a shortcut label ends with.
fn shortcut_key(shortcut: &str) -> Option<String> {
    let key = shortcut.chars().last()?;
    Some(match key {
        '⌫' => "backspace".into(),
        '⌦' => "delete".into(),
        '↩' => "enter".into(),
        '⎋' => "escape".into(),
        key => key.to_lowercase().to_string(),
    })
}

/// Whether a keystroke is the platform's form of one of the shortcuts the menu
/// prints, so those labels are real bindings while the menu is open.
fn matches_shortcut(keystroke: &Keystroke, shortcut: &str) -> bool {
    let Some(key) = shortcut_key(shortcut) else {
        return false;
    };
    if keystroke.key.to_lowercase() != key {
        return false;
    }
    let modifiers = keystroke.modifiers;
    let command = if cfg!(target_os = "macos") {
        modifiers.platform
    } else {
        modifiers.control
    };
    command == shortcut.contains('⌘')
        && modifiers.shift == shortcut.contains('⇧')
        && modifiers.alt == shortcut.contains('⌥')
}

/// Total menu height including separators and the 4px inner padding.
fn menu_height(target: &MenuTarget) -> f32 {
    let visible = visible_menu_items(target);
    let rules = surface_of(target).menu().1;
    let separators = visible
        .iter()
        .enumerate()
        .filter(|(position, index)| *position > 0 && rules.contains(index))
        .count();
    visible.len() as f32 * MENU_ROW + separators as f32 * 9. + 8.
}

/// Whether a closing bracket may be inserted in front of this character.
///
/// Zed's rule, and the reason it exists: a pair typed in front of a word would
/// swallow it — `foo` becomes `()foo` — when the opener was meant to go before
/// it. Whitespace and an existing closer are the only things a pair may be
/// opened against.
fn allows_autoclose(next: Option<char>) -> bool {
    next.is_none_or(|next| next.is_whitespace() || ")]}".contains(next))
}

/// Whether a character is part of a word as far as pairing is concerned. Zed
/// asks the language's character classifier; a letter, a digit or an
/// underscore is the whole of what that resolves to here.
fn is_word_char(previous: Option<char>) -> bool {
    previous.is_some_and(|previous| previous.is_alphanumeric() || previous == '_')
}

/// An open right-click menu, anchored where the pointer was.
struct ContextMenu {
    target: MenuTarget,
    position: Point<Pixels>,
    /// Index into the target surface's menu, for the keyboard and the
    /// highlight.
    selected: usize,
}

/// What a tab drag carries. GPUI drags a value plus a view to draw under the
/// pointer, so this is both.
#[derive(Clone)]
struct TabDrag {
    path: PathBuf,
    label: SharedString,
}

/// The little tab that follows the pointer while one is being dragged.
struct TabDragPreview {
    label: SharedString,
}

impl Render for TabDragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_sm()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .shadow_md()
            .text_size(ui(11.))
            .text_color(cx.theme().foreground)
            .child(self.label.clone())
    }
}

/// A file-tree row being dragged onto a folder.
#[derive(Clone)]
struct TreeDrag {
    path: PathBuf,
    name: SharedString,
}

struct TreeDragPreview {
    name: SharedString,
}

impl Render for TreeDragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_sm()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .shadow_md()
            .text_size(ui(11.))
            .text_color(cx.theme().foreground)
            .child(self.name.clone())
    }
}

/// The in-app file clipboard behind Cut / Copy / Paste.
#[derive(Clone)]
struct FileClipboard {
    path: PathBuf,
    /// Cut moves the entry on paste; copy leaves the source alone.
    cut: bool,
}

/// A tree row currently being typed into: a new entry being named, or an
/// existing one being renamed. The filesystem is only touched on Enter.
struct InlineEdit {
    /// Folder that will receive a new entry, or that holds the renamed one.
    parent: PathBuf,
    kind: EntryKind,
    /// The entry being renamed, or `None` when this is a new entry.
    renaming: Option<PathBuf>,
    /// Row the text field occupies. `rebuild_rows` keeps this in step with the
    /// list it splices into.
    row: usize,
    input: Entity<InputState>,
    _subscription: Subscription,
}

/// One line of the project-search results list. File headers and matches share
/// a height so the list can stay a `uniform_list`.
#[derive(Clone, Copy)]
enum SearchRow {
    File(usize),
    Hit { file: usize, hit: usize },
}

/// Project-search panel state, grouped so `Folio` stays readable.
#[derive(Default)]
struct SearchState {
    options: search::Options,
    /// The query the current results were produced from.
    query: String,
    /// Set by "Find in Folder", which narrows the scan to one subtree.
    scope: Option<PathBuf>,
    results: Vec<search::FileHits>,
    rows: Vec<SearchRow>,
    /// Index into `rows`; always a `SearchRow::Hit` when the list is not empty.
    selected: usize,
    running: bool,
    truncated: bool,
    error: Option<String>,
    show_replace: bool,
    scroll: UniformListScrollHandle,
}

/// Wait for typing to settle before scanning. Long enough to avoid scanning on
/// every keystroke, short enough to still feel live.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(140);

/// Flatten grouped results into the single list the panel renders.
fn search_rows(results: &[search::FileHits]) -> Vec<SearchRow> {
    let mut rows = Vec::with_capacity(results.len() * 2);
    for (file, hits) in results.iter().enumerate() {
        rows.push(SearchRow::File(file));
        rows.extend((0..hits.hits.len()).map(|hit| SearchRow::Hit { file, hit }));
    }
    rows
}

/// The active file shown as a diff against HEAD, in place of the editor.
#[derive(Default)]
struct DiffView {
    path: PathBuf,
    lines: Vec<diff::Line>,
    untracked: bool,
    /// `None` once git has answered. Loading and failed are different from
    /// "no changes" and have to say so rather than showing an empty list.
    error: Option<String>,
    loading: bool,
    scroll: UniformListScrollHandle,
    /// Bumped per request, so a slow `git` for a file the user has moved on
    /// from does not land on top of the newer one.
    request: u64,
}

#[derive(Default)]
struct Project {
    id: u64,
    image: Option<(PathBuf, std::sync::Arc<RenderImage>)>,
    workspace: Option<Workspace>,
    directories: HashMap<PathBuf, Vec<Entry>>,
    expanded: HashSet<PathBuf>,
    rows: Vec<Row>,
    selected_row: usize,
    tree_scroll: UniformListScrollHandle,
    documents: HashMap<PathBuf, Document>,
    /// The open files, in strip order. `documents` holds the buffers; this
    /// holds which of them are showing and in what order.
    tabs: Vec<PathBuf>,
    /// Tabs the user has pinned, and buffers they have locked. Both are view
    /// state — neither reaches the disk — and both are per project, so
    /// switching projects keeps them.
    pinned: HashSet<PathBuf>,
    read_only: HashSet<PathBuf>,
    active: Option<PathBuf>,
    git_status: HashMap<String, char>,
    files: Vec<PathBuf>,
    indexing: bool,
    /// Set while the active file is being shown as changes rather than code.
    diff: Option<DiffView>,
    /// Open buffers whose disk copy no longer matches `saved`. Clean buffers
    /// reload on their own; these are the dirty ones waiting for Reload / Keep.
    disk_notice: HashMap<PathBuf, DiskNotice>,
}

/// What the disk did to a dirty buffer while Folio still holds edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiskNotice {
    Changed,
    Removed,
}

/// One project's terminal tabs: the running terminals and which of them
/// the drawer shows.
struct TerminalTabs {
    views: Vec<Entity<TerminalView>>,
    active: usize,
}

pub struct Folio {
    project: Project,
    parked: Vec<Project>,
    project_order: Vec<PathBuf>,
    recent: Vec<RecentProject>,
    recent_file: PathBuf,
    recent_task: Option<Task<()>>,
    tree_focus: FocusHandle,
    /// Row the pointer is over, so the indent guide for that folder can
    /// brighten the way Zed's does. Cleared when the tree is rebuilt.
    tree_hover_row: Option<usize>,
    quick_scroll: UniformListScrollHandle,
    sidebar: bool,
    /// The far-left icon rail. Independent of the file tree: hiding the
    /// rail does not close the tree, and `⌘B` does not hide the rail.
    activity_bar: bool,
    sidebar_width: f32,
    resizing: bool,
    /// The terminal drawer: whether it shows, how tall, whether its top
    /// edge is being dragged, and each project's terminal tabs.
    terminals: HashMap<PathBuf, TerminalTabs>,
    terminal_open: bool,
    terminal_height: f32,
    resizing_terminal: bool,
    generation: u64,
    open_request: u64,
    query: Entity<InputState>,
    panel: Option<Panel>,
    matches: Vec<PathBuf>,
    match_selected: usize,
    /// The open right-click menu, if any.
    menu: Option<ContextMenu>,
    /// Where a dragged tab would land, while one is in flight. Drawn as a
    /// caret between the tabs.
    tab_drop: Option<usize>,
    /// Where Cut / Copy put the entry, and whether it was a cut.
    clipboard: Option<FileClipboard>,
    /// The tree row being named, if any.
    editing: Option<InlineEdit>,
    search_query: Entity<InputState>,
    replace_query: Entity<InputState>,
    search: SearchState,
    /// Loaded once at startup and written back whenever the panel changes it.
    settings: Settings,
    /// Last wrap mode pushed into open editors, so a settings change can
    /// catch up on the next frame when we have a window.
    applied_wrap: bool,
    /// The ignore rules the tree's cached listings were built with. Tracked
    /// separately because callers edit `settings` before applying it, so
    /// comparing against that field would never see a change.
    applied_ignored: Vec<String>,
    settings_file: PathBuf,
    /// Held so the settings window can be focused instead of opened twice.
    settings_window: Option<WindowHandle<Root>>,
    about_window: Option<WindowHandle<Root>>,
    settings_view: Option<Entity<SettingsView>>,
    /// The system appearance, kept so a theme change can be applied without
    /// borrowing a particular window.
    appearance: WindowAppearance,
    /// The main window's bounds. Recorded here because `⌘Q` can arrive from
    /// the settings window, and the geometry to persist is never its own.
    main_bounds: Bounds<Pixels>,
    /// Where the settings window was, as last saved. `None` means it has never
    /// been placed, so it opens centred.
    settings_bounds: Option<[f32; 4]>,
    window_file: PathBuf,
    /// Bumped for every new search; a running scan compares it to stop early.
    search_request: Arc<AtomicU64>,
    /// Set when a search result is opened, so the cursor lands on the match once
    /// the file's editor exists.
    goto: Option<(PathBuf, Position)>,
    message: Option<String>,
    loading: bool,
    project_loading: bool,
    saving: bool,
    prompting: bool,
    /// Where the last session and the unsaved buffers are written.
    session_file: PathBuf,
    unsaved_file: PathBuf,
    /// What is left of a session being put back. One step at a time, because
    /// opening a project and opening a file are both asynchronous and neither
    /// can be started while the other is in flight.
    restore: VecDeque<Restore>,
    /// The blame being shown for the file being read, when it is on.
    blame: Option<BlameView>,
    /// Which blame read is the current one: a read of one file can land after
    /// the user has moved to another.
    blame_request: u64,
    /// The file a recovered buffer belongs to, waiting for its own open to
    /// land. Keyed by path so that an open the user started in the meantime
    /// cannot pick it up.
    restoring: Option<(PathBuf, String)>,
    /// How many buffers were recovered, for the message that says so.
    recovered: usize,
    /// What was last written to the unsaved file, so that the timer only
    /// rewrites it when there is something new to write.
    written_unsaved: session::UnsavedBuffers,
    written_session: session::Session,
    disk_watch: watch::DiskWatch,
    /// Paths Folio just wrote, and when their own filesystem events expire.
    quiet_writes: HashMap<PathBuf, Instant>,
    _subscriptions: Vec<Subscription>,
}

/// How often the app writes down what is open and what is unsaved. Often
/// enough that a crash costs seconds of work, rarely enough that it is not
/// doing file work while the user types.
const UNSAVED_EVERY: Duration = Duration::from_secs(5);

/// What the caret's line was last written by, for the file being read.
struct BlameView {
    path: PathBuf,
    blame: blame::Blame,
}

/// The two files a session lives in, and what has changed in them.
struct Writes {
    session_file: PathBuf,
    unsaved_file: PathBuf,
    session: Option<session::Session>,
    unsaved: Option<session::UnsavedBuffers>,
}

impl Writes {
    fn is_empty(&self) -> bool {
        self.session.is_none() && self.unsaved.is_none()
    }

    /// Write both, and hand them back so the caller can remember what landed.
    ///
    /// Errors are dropped rather than reported: a config directory that cannot
    /// be written to is not worth a message every few seconds, and the next
    /// tick tries again.
    fn write(self) -> Writes {
        if let Some(session) = &self.session {
            let _ = session::Session::save(session, &self.session_file);
        }
        if let Some(unsaved) = &self.unsaved {
            let _ = session::save_unsaved(&self.unsaved_file, unsaved);
        }
        self
    }
}

/// One step of putting a session back.
enum Restore {
    /// A project that was open.
    Project(PathBuf),
    /// A file of one, in the order the strip had them.
    File(PathBuf),
    /// The file the project was showing, after its tabs — it is open by then,
    /// so this only makes it the one on screen.
    Activate(PathBuf),
    /// A file that had edits which never reached the disk: opened, and then
    /// given them back.
    Unsaved(PathBuf, String),
}

fn remap_project_paths(project: &mut Project, from: &Path, to: &Path) {
    fn moved(path: &Path, from: &Path, to: &Path) -> Option<PathBuf> {
        let rest = path.strip_prefix(from).ok()?;
        Some(if rest.as_os_str().is_empty() {
            to.to_path_buf()
        } else {
            to.join(rest)
        })
    }
    let documents = std::mem::take(&mut project.documents);
    project.documents = documents
        .into_iter()
        .map(|(path, document)| (moved(&path, from, to).unwrap_or(path), document))
        .collect();
    if let Some(active) = project.active.take() {
        project.active = Some(moved(&active, from, to).unwrap_or(active));
    }
    if let Some((image, render)) = project.image.take() {
        project.image = Some((moved(&image, from, to).unwrap_or(image), render));
    }
    for tab in &mut project.tabs {
        if let Some(renamed) = moved(tab, from, to) {
            *tab = renamed;
        }
    }
    let remap_set = |set: HashSet<PathBuf>| {
        set.into_iter()
            .map(|path| moved(&path, from, to).unwrap_or(path))
            .collect()
    };
    project.expanded = remap_set(std::mem::take(&mut project.expanded));
    project.pinned = remap_set(std::mem::take(&mut project.pinned));
    project.read_only = remap_set(std::mem::take(&mut project.read_only));
    let notice = std::mem::take(&mut project.disk_notice);
    project.disk_notice = notice
        .into_iter()
        .map(|(path, notice)| (moved(&path, from, to).unwrap_or(path), notice))
        .collect();
    let directories = std::mem::take(&mut project.directories);
    project.directories = directories
        .into_iter()
        .map(|(path, entries)| (moved(&path, from, to).unwrap_or(path), entries))
        .collect();
}

impl Folio {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // A corrupt settings file falls back to the defaults rather than
        // stopping the app, but the user is told instead of it being silently
        // replaced on the next write.
        let settings_file = config_dir().join("settings.json");
        let (settings, settings_error) = match settings::load(&settings_file) {
            Ok(settings) => (settings, None),
            Err(error) => (
                Settings::default(),
                Some(format!("Could not read settings: {error}")),
            ),
        };
        let appearance = window.appearance();
        let main_bounds = window.window_bounds().get_bounds();
        sync_appearance(appearance, &settings, Some(window), cx);
        let appearance_subscription =
            cx.observe_window_appearance(window, |this: &mut Self, window, cx| {
                this.appearance = window.appearance();
                sync_appearance(this.appearance, &this.settings, Some(window), cx);
                cx.notify();
            });
        let query = cx
            .new(|cx| InputState::new(window, cx).placeholder("Search file names, or type :line"));
        let subscription = cx.subscribe_in(
            &query,
            window,
            |this: &mut Self, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    this.filter(cx);
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.accept_match(window, cx),
                _ => {}
            },
        );
        let bounds_subscription =
            cx.observe_window_bounds(window, |this: &mut Self, window, cx| {
                this.main_bounds = window.window_bounds().get_bounds();
                this.sidebar_width = this
                    .sidebar_width
                    .min((f32::from(window.viewport_size().width) * 0.4).max(160.));
                cx.notify();
            });
        let search_query = cx.new(|cx| InputState::new(window, cx).placeholder("Search contents"));
        let search_subscription = cx.subscribe_in(
            &search_query,
            window,
            |this: &mut Self, _, event: &InputEvent, window, cx| match event {
                InputEvent::Change => {
                    this.start_search(cx);
                    cx.notify();
                }
                InputEvent::PressEnter { .. } => this.open_selected_hit(window, cx),
                _ => {}
            },
        );
        let replace_query = cx.new(|cx| InputState::new(window, cx).placeholder("Replace with"));
        let replace_subscription = cx.subscribe_in(
            &replace_query,
            window,
            |this: &mut Self, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.replace_project(window, cx);
                    cx.notify();
                }
            },
        );
        let recent_file = config_dir().join("recent.json");
        let window_file = config_dir().join("window.json");
        let settings_bounds = WindowState::load(&window_file).settings;
        let settings_ignored = settings.ignored.clone();
        let mut this = Self {
            project: Project::default(),
            parked: vec![],
            project_order: vec![],
            recent: vec![],
            recent_file,
            recent_task: None,
            tree_focus: cx.focus_handle(),
            tree_hover_row: None,
            quick_scroll: UniformListScrollHandle::new(),
            sidebar: settings.sidebar,
            activity_bar: settings.activity_bar,
            sidebar_width: 240.,
            resizing: false,
            terminals: HashMap::new(),
            terminal_open: false,
            terminal_height: 260.,
            resizing_terminal: false,
            generation: 0,
            open_request: 0,
            query,
            panel: None,
            matches: vec![],
            match_selected: 0,
            menu: None,
            tab_drop: None,
            clipboard: None,
            editing: None,
            search_query,
            replace_query,
            search: SearchState::default(),
            settings,
            applied_wrap: false,
            applied_ignored: settings_ignored,
            settings_file,
            settings_window: None,
            about_window: None,
            settings_view: None,
            appearance,
            main_bounds,
            settings_bounds,
            window_file,
            search_request: Arc::new(AtomicU64::new(0)),
            goto: None,
            message: settings_error,
            loading: false,
            project_loading: false,
            saving: false,
            prompting: false,
            session_file: config_dir().join("session.json"),
            unsaved_file: config_dir().join("unsaved.json"),
            restore: VecDeque::new(),
            blame: None,
            blame_request: 0,
            restoring: None,
            recovered: 0,
            written_unsaved: session::UnsavedBuffers::new(),
            written_session: session::Session::default(),
            disk_watch: watch::DiskWatch::default(),
            quiet_writes: HashMap::new(),
            _subscriptions: vec![
                subscription,
                search_subscription,
                replace_subscription,
                bounds_subscription,
                appearance_subscription,
            ],
        };
        this.refresh_recent(RecentAction::Load, cx);
        this.tree_focus.focus(window, cx);
        this
    }

    /// Start keeping the session: put back what was open last time, and write
    /// down what is open from here on.
    ///
    /// Not done in the constructor because it reads the config directory, and a
    /// window built without one — a test's — should not be reading the machine's
    /// session and opening its projects.
    pub fn start_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.start_restore(window, cx);
        self.watch_unsaved(cx);
        self.watch_disk(window, cx);
    }

    fn request(&mut self, next: Next, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || self.prompting || self.project_loading {
            return;
        }
        // Invalidate a pending file read before changing project or opening a modal.
        self.open_request += 1;
        self.loading = false;
        // By reference: one arm needs the path, and `next` is still wanted
        // for the prompt and the follow-up.
        let dirty = match &next {
            Next::Picker | Next::Open(_) => 0,
            // Only the files being closed are at stake, so only their own
            // edits are counted.
            Next::CloseTabs(paths) => paths.iter().filter(|path| self.is_dirty(path)).count(),
            Next::Close => self.project.documents.values().filter(|d| d.dirty).count(),
            Next::Quit => std::iter::once(&self.project)
                .chain(self.parked.iter())
                .flat_map(|p| p.documents.values())
                .filter(|d| d.dirty)
                .count(),
        };
        if dirty == 0 {
            self.perform(next, window, cx);
            return;
        }
        // Closing one tab asks about that file alone; closing a project or
        // quitting asks about everything it would take with it.
        if let Next::CloseTabs(paths) = &next {
            self.prompt_close_tabs(paths.clone(), window, cx);
            return;
        }
        self.prompting = true;
        cx.notify();
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("{dirty} files have unsaved changes"),
            Some(if matches!(next, Next::Quit) {
                "Save changes in every project before quitting?"
            } else {
                "Save changes before closing this project?"
            }),
            &["Save All", "Don't Save", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = answer.await.unwrap_or(2);
            let _ = this.update_in(cx, |this, window, cx| {
                this.prompting = false;
                cx.notify();
                match answer {
                    0 => this.save_documents(Some(next), window, cx),
                    1 => this.perform(next, window, cx),
                    _ => {}
                }
            });
        })
        .detach();
    }

    /// Ask about unsaved changes before these tabs go away. One file is named;
    /// several are counted, because the answer is the same either way.
    fn prompt_close_tabs(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prompting = true;
        cx.notify();
        let (title, body) = match paths.as_slice() {
            [only] => (name(only), "This file has unsaved changes.".to_string()),
            many => (
                format!("{} files", many.len()),
                "These files have unsaved changes.".to_string(),
            ),
        };
        let answer = window.prompt(
            PromptLevel::Warning,
            &title,
            Some(&body),
            &["Save", "Don't Save", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = answer.await.unwrap_or(2);
            let _ = this.update_in(cx, |this, window, cx| {
                this.prompting = false;
                match answer {
                    0 => this.save_documents(Some(Next::CloseTabs(paths)), window, cx),
                    1 => this.close_tabs(paths, window, cx),
                    _ => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Take these files out of the strip and release their buffers. Only ever
    /// called once their unsaved changes have been settled.
    fn close_tabs(&mut self, closing: Vec<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        let active_survives = self
            .project
            .active
            .as_ref()
            .is_some_and(|active| !closing.contains(active));
        // Where the view lands if the active tab is one of them: the first tab
        // after the set that survives, else the last one before it.
        let replacement = self.project.active.as_ref().and_then(|active| {
            let index = self.project.tabs.iter().position(|tab| tab == active)?;
            self.project.tabs[index..]
                .iter()
                .find(|tab| !closing.contains(tab))
                .or_else(|| {
                    self.project.tabs[..index]
                        .iter()
                        .rev()
                        .find(|tab| !closing.contains(tab))
                })
                .cloned()
        });
        for path in &closing {
            self.project.tabs.retain(|tab| tab != path);
            self.project.documents.remove(path);
            self.project.pinned.remove(path);
            self.project.read_only.remove(path);
            if self
                .project
                .image
                .as_ref()
                .is_some_and(|(image, _)| image == path)
            {
                self.project.image = None;
            }
            if self.goto.as_ref().is_some_and(|(target, _)| target == path) {
                self.goto = None;
            }
        }
        if active_survives {
            cx.notify();
            return;
        }
        match replacement {
            Some(next) => self.open_file(next, window, cx),
            None => {
                self.project.active = None;
                self.open_request += 1;
                self.loading = false;
                self.focus_editor(window, cx);
                self.update_title(window);
                cx.notify();
            }
        }
    }

    fn perform(&mut self, next: Next, window: &mut Window, cx: &mut Context<Self>) {
        match next {
            Next::CloseTabs(paths) => self.close_tabs(paths, window, cx),
            Next::Quit => {
                // A settings window still open is the authority on its own
                // geometry; otherwise whatever it last reported stands.
                let open = self.settings_window.and_then(|window| {
                    window
                        .update(cx, |_, window, _| {
                            bounds_values(window.window_bounds().get_bounds())
                        })
                        .ok()
                });
                if open.is_some() {
                    self.settings_bounds = open;
                }
                self.save_window_state();
                self.finish_session();
                cx.quit();
            }
            Next::Close => {
                // The closing project takes its terminal with it; dropping
                // the view shuts the child down.
                if let Some(workspace) = &self.project.workspace {
                    self.terminals.remove(&workspace.root);
                }
                self.reset();
                if let Some(workspace) = &self.project.workspace {
                    self.project_order.retain(|root| root != &workspace.root);
                }
                self.project = Project::default();
                if let Some(root) = self.project_order.first().cloned() {
                    self.switch_project(&root, window, cx);
                } else {
                    window.set_window_title("Folio");
                }
                self.sync_disk_watch();
                cx.notify();
            }
            Next::Open(path) => self.open_project(path, window, cx),
            Next::Picker => {
                let picker = cx.prompt_for_paths(PathPromptOptions {
                    files: false,
                    directories: true,
                    multiple: false,
                    prompt: Some("Open Project".into()),
                });
                self.prompting = true;
                cx.notify();
                cx.spawn_in(window, async move |this, cx| {
                    let result = picker.await;
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.prompting = false;
                        match result {
                            Ok(Ok(Some(paths))) => {
                                if let Some(path) = paths.into_iter().next() {
                                    this.open_project(path, window, cx);
                                }
                            }
                            Ok(Err(e)) => this.error(e.to_string(), cx),
                            _ => {}
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
        }
    }

    fn reset(&mut self) {
        self.generation += 1;
        self.open_request += 1;
        self.panel = None;
        self.matches.clear();
        self.quick_scroll = UniformListScrollHandle::new();
        self.cancel_search();
        self.goto = None;
        self.loading = false;
        self.project_loading = false;
        self.resizing = false;
        self.message = None;
    }

    /// Supersede any running scan and drop the results it produced.
    fn cancel_search(&mut self) {
        self.search_request.fetch_add(1, Ordering::Relaxed);
        self.search.running = false;
        self.search.truncated = false;
        self.search.error = None;
        self.search.results.clear();
        self.search.rows.clear();
        self.search.selected = 0;
    }

    /// Put the cursor on a match once its file has an editor.
    ///
    /// `Position`'s column counts characters, which is what `search::Hit`
    /// records, so this is a direct hand-off. Cleared whether or not it applied.
    fn apply_goto(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        let Some((target, position)) = self.goto.clone() else {
            return;
        };
        if target != path {
            return;
        }
        self.goto = None;
        let Some(document) = self.project.documents.get(&target) else {
            return;
        };
        let editor = document.editor.clone();
        editor.update(cx, |state, cx| {
            state.base_state().clone().update(cx, |base, cx| {
                base.set_cursor_position(position, window, cx)
            });
        });
    }

    fn project_ref(&self, id: u64) -> Option<&Project> {
        if self.project.id == id {
            Some(&self.project)
        } else {
            self.parked.iter().find(|project| project.id == id)
        }
    }

    fn project_mut(&mut self, id: u64) -> Option<&mut Project> {
        if self.project.id == id {
            Some(&mut self.project)
        } else {
            self.parked.iter_mut().find(|project| project.id == id)
        }
    }

    fn switch_project(&mut self, root: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || self.prompting || self.project_loading {
            return;
        }
        if self
            .project
            .workspace
            .as_ref()
            .is_some_and(|w| w.root == root)
        {
            return;
        }
        let Some(index) = self
            .parked
            .iter()
            .position(|p| p.workspace.as_ref().is_some_and(|w| w.root == root))
        else {
            return;
        };
        self.reset();
        let project = self.parked.remove(index);
        let previous = std::mem::replace(&mut self.project, project);
        if previous.workspace.is_some() {
            self.parked.push(previous);
        }
        self.rebuild_rows();
        self.update_title(window);
        // The dock follows the project it is open on.
        if self.terminal_open {
            self.ensure_terminal(cx);
        }
        if let Some(doc) = self
            .project
            .active
            .as_ref()
            .and_then(|p| self.project.documents.get(p))
        {
            doc.editor.focus_handle(cx).focus(window, cx);
        } else {
            self.tree_focus.focus(window, cx);
        }
        self.refresh_git(cx);
        cx.notify();
    }

    fn open_project(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.project_loading || self.saving || self.prompting {
            return;
        }
        self.open_request += 1;
        self.project_loading = true;
        self.loading = true;
        self.message = None;
        cx.notify();
        let generation = self.generation;
        let ignored = self.settings.ignored.clone();
        let task = cx.background_executor().spawn(async move {
            let workspace = Workspace::open(&path)?;
            let children = tree::children(&workspace.root, &ignored)?;
            Ok::<_, std::io::Error>((workspace, children))
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.generation != generation {
                    return;
                }
                this.loading = false;
                this.project_loading = false;
                cx.notify();
                match result {
                    Ok((workspace, children)) => {
                        if this
                            .project
                            .workspace
                            .as_ref()
                            .is_some_and(|w| w.root == workspace.root)
                        {
                            return;
                        }
                        if this.parked.iter().any(|p| {
                            p.workspace
                                .as_ref()
                                .is_some_and(|w| w.root == workspace.root)
                        }) {
                            this.switch_project(&workspace.root, window, cx);
                            return;
                        }
                        this.reset();
                        let previous = std::mem::replace(
                            &mut this.project,
                            Project {
                                id: this.generation,
                                ..Default::default()
                            },
                        );
                        if previous.workspace.is_some() {
                            this.parked.push(previous);
                        }
                        this.project_order.push(workspace.root.clone());
                        this.project
                            .directories
                            .insert(workspace.root.clone(), children);
                        this.project.expanded.insert(workspace.root.clone());
                        window.set_window_title(&name(&workspace.root));
                        this.project.workspace = Some(workspace);
                        this.rebuild_rows();
                        this.refresh_project(cx);
                        if this.terminal_open {
                            this.ensure_terminal(cx);
                        }
                        this.tree_focus.focus(window, cx);
                        this.sync_disk_watch();
                    }
                    Err(e) => this.error(format!("Could not open the project: {e}"), cx),
                }
                cx.notify();
                // Nothing left to put back is the common case: this is the same
                // path a project the user opened takes.
                this.next_restore(window, cx);
            });
        })
        .detach();
    }

    fn refresh_project(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.project.workspace.as_ref().map(|w| w.root.clone()) else {
            return;
        };
        self.refresh_recent(RecentAction::Open(root), cx);
        self.reindex(cx);
        self.refresh_git(cx);
    }

    /// Rebuild the flat index that quick-open and project search read.
    fn reindex(&mut self, cx: &mut Context<Self>) {
        self.reindex_project(self.project.id, cx);
    }

    fn reindex_project(&mut self, project_id: u64, cx: &mut Context<Self>) {
        let Some(root) = self.project_ref(project_id).and_then(|project| {
            project
                .workspace
                .as_ref()
                .map(|workspace| workspace.root.clone())
        }) else {
            return;
        };
        if let Some(project) = self.project_mut(project_id) {
            project.indexing = true;
        }
        let ignored = self.settings.ignored.clone();
        let task = cx
            .background_executor()
            .spawn(async move { tree::index(&root, &ignored) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                let Some(project) = this.project_mut(project_id) else {
                    return;
                };
                project.indexing = false;
                match result {
                    Ok(files) => {
                        project.files = files;
                        if this.project.id == project_id {
                            this.filter(cx);
                        }
                    }
                    Err(e) => this.error(e.to_string(), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn refresh_git(&mut self, cx: &mut Context<Self>) {
        self.refresh_git_project(self.project.id, cx);
    }

    fn refresh_git_project(&mut self, project_id: u64, cx: &mut Context<Self>) {
        let Some(root) = self.project_ref(project_id).and_then(|project| {
            project
                .workspace
                .as_ref()
                .map(|workspace| workspace.root.clone())
        }) else {
            return;
        };
        let task = cx
            .background_executor()
            .spawn(async move { git::status(&root) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                let Some(project) = this.project_mut(project_id) else {
                    return;
                };
                match result {
                    Ok(status) => project.git_status = status,
                    Err(e) => this.error(format!("Could not read git status: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn rebuild_rows(&mut self) {
        self.tree_hover_row = None;
        fn flatten(
            dir: &Path,
            depth: usize,
            cache: &HashMap<PathBuf, Vec<Entry>>,
            expanded: &HashSet<PathBuf>,
            rows: &mut Vec<Row>,
        ) {
            if let Some(entries) = cache.get(dir) {
                for entry in entries {
                    rows.push(Row {
                        entry: entry.clone(),
                        depth,
                    });
                    if expanded.contains(&entry.path) {
                        flatten(&entry.path, depth + 1, cache, expanded, rows);
                    }
                }
            }
        }
        self.project.rows.clear();
        if let Some(workspace) = &self.project.workspace
            && self.project.expanded.contains(&workspace.root)
        {
            flatten(
                &workspace.root,
                0,
                &self.project.directories,
                &self.project.expanded,
                &mut self.project.rows,
            );
        }
        self.project.selected_row = self
            .project
            .selected_row
            .min(self.project.rows.len().saturating_sub(1));

        // The row being named is not on disk under its new name yet, so it is
        // spliced in after the flattening pass: renaming takes over the entry's
        // own row, creating adds one in front of the folder's children.
        let edit = self.editing.as_ref().map(|edit| {
            (
                edit.parent.clone(),
                edit.kind,
                edit.renaming.clone(),
                edit.renaming.as_ref().and_then(|path| {
                    self.project
                        .rows
                        .iter()
                        .position(|row| &row.entry.path == path)
                }),
            )
        });
        if let Some((parent, kind, renaming, taken)) = edit {
            // Collapsing the folder took the renamed entry's row away; there is
            // nothing left to type into.
            if renaming.is_some() && taken.is_none() {
                self.editing = None;
                return;
            }
            let (index, depth) = match taken {
                Some(index) => (index, self.project.rows[index].depth),
                None => {
                    let parent_row = self
                        .project
                        .rows
                        .iter()
                        .position(|row| row.entry.path == parent);
                    match (parent_row, &self.project.workspace) {
                        (Some(index), _) => (index + 1, self.project.rows[index].depth + 1),
                        (None, Some(workspace)) if parent == workspace.root => (0, 0),
                        (None, _) => (self.project.rows.len(), 0),
                    }
                }
            };
            // A placeholder path that can never collide with a real entry: NUL
            // is illegal in a path on every platform we build for.
            let entry = Entry {
                path: renaming.unwrap_or_else(|| parent.join("\0")),
                name: String::new(),
                kind,
            };
            let row = Row { entry, depth };
            if taken.is_some() {
                self.project.rows[index] = row;
            } else {
                self.project.rows.insert(index, row);
            }
            if let Some(edit) = self.editing.as_mut() {
                edit.row = index;
            }
        }
    }

    fn toggle_directory(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self.project.expanded.remove(&path) {
            if self.project.directories.contains_key(&path) {
                self.project.expanded.insert(path);
            } else {
                // Keep expansion intent while the read is pending; a late result only fills the cache.
                self.project.expanded.insert(path.clone());
                self.project.directories.insert(path.clone(), vec![]);
                let project_id = self.project.id;
                let dir = path.clone();
                let ignored = self.settings.ignored.clone();
                let task = cx
                    .background_executor()
                    .spawn(async move { tree::children(&dir, &ignored) });
                cx.spawn(async move |this, cx| {
                    let result = task.await;
                    let _ = this.update(cx, |this, cx| {
                        let Some(project) = this.project_mut(project_id) else {
                            return;
                        };
                        match result {
                            Ok(children) => {
                                project.directories.insert(path.clone(), children);
                            }
                            Err(e) => {
                                project.directories.remove(&path);
                                project.expanded.remove(&path);

                                this.error(e.to_string(), cx);
                            }
                        }
                        if this.project.id == project_id {
                            this.rebuild_rows();
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
        }
        self.rebuild_rows();
        cx.notify();
    }

    /// Re-read one folder into the tree cache. Unlike `toggle_directory` this
    /// forces a fresh read, which is what a filesystem change needs.
    fn reload_directory(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        let Some(project_id) = std::iter::once(&self.project)
            .chain(self.parked.iter())
            .find(|project| project.directories.contains_key(&dir))
            .map(|project| project.id)
        else {
            return;
        };
        let read = dir.clone();
        let ignored = self.settings.ignored.clone();
        let task = cx
            .background_executor()
            .spawn(async move { tree::children(&read, &ignored) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                let error = match result {
                    Ok(children) => {
                        if let Some(project) = this.project_mut(project_id) {
                            project.directories.insert(dir.clone(), children);
                        }
                        None
                    }
                    Err(e) => Some(e.to_string()),
                };
                if this.project.id == project_id {
                    this.rebuild_rows();
                }
                if let Some(error) = error {
                    this.error(error, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Refresh everything a filesystem change invalidates: the folder that
    /// changed, the quick-open index, and the git dots.
    fn rescan(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.reload_directory(dir, cx);
        self.reindex(cx);
        self.refresh_git(cx);
    }

    /// Run a launcher — Reveal, Open, Terminal — off the UI thread.
    fn spawn_launch<F>(&mut self, work: F, cx: &mut Context<Self>)
    where
        F: FnOnce() -> io::Result<()> + Send + 'static,
    {
        let task = cx.background_executor().spawn(async move { work() });
        cx.spawn(async move |this, cx| {
            if let Err(error) = task.await {
                let _ = this.update(cx, |this, cx| this.error(error.to_string(), cx));
            }
        })
        .detach();
    }

    /// Run a change to the filesystem off the UI thread. On success the folder
    /// that changed is re-read; on failure the tree is left alone. A newly
    /// created file is opened once it exists on disk.
    fn spawn_change<F>(
        &mut self,
        dir: PathBuf,
        open_created: bool,
        work: F,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) where
        F: FnOnce() -> io::Result<PathBuf> + Send + 'static,
    {
        let task = cx.background_executor().spawn(async move { work() });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(created) => {
                        this.quiet_write(&created);
                        this.rescan(dir, cx);
                        if open_created {
                            this.open_file(created, window, cx);
                        }
                    }
                    Err(error) => this.error(error.to_string(), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Rename off the UI thread. `spawn_change`'s shape is not enough here:
    /// the caches that key off the old path have to move with the entry.
    fn spawn_rename(
        &mut self,
        from: PathBuf,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source = from.clone();
        let task = cx
            .background_executor()
            .spawn(async move { fs_op::rename(&source, &name) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(renamed) => {
                        this.quiet_write(&renamed);
                        this.remap_paths(&from, &renamed);
                        this.rebuild_rows();
                        let dir = renamed
                            .parent()
                            .map(Path::to_path_buf)
                            .unwrap_or_else(|| renamed.clone());
                        this.rescan(dir, cx);
                        this.update_title(window);
                    }
                    Err(error) => this.error(error.to_string(), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// How many open buffers under `path` hold edits that are not on disk.
    fn unsaved_under(&self, path: &Path) -> usize {
        self.project
            .documents
            .iter()
            .filter(|(key, document)| key.starts_with(path) && document.dirty)
            .count()
    }

    /// Deleting is irreversible, so it always asks first. Trashing is
    /// recoverable and only asks when it would drop unsaved edits.
    fn confirm_removal(
        &mut self,
        path: PathBuf,
        trashed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.saving || self.prompting || self.project_loading {
            return;
        }
        let name = name(&path);
        let unsaved = self.unsaved_under(&path);
        let detail = match (trashed, unsaved) {
            (true, unsaved) => {
                format!(
                    "{name} and its contents will move to the Trash. {unsaved} files have unsaved changes."
                )
            }
            (false, 0) => format!(
                "{name} and its contents will be deleted permanently. This cannot be undone."
            ),
            (false, unsaved) => format!(
                "{name} and its contents will be deleted permanently. This cannot be undone. {unsaved} files have unsaved changes."
            ),
        };
        let confirm = if trashed { "Move to Trash" } else { "Delete" };
        self.prompting = true;
        cx.notify();
        let answer = window.prompt(
            PromptLevel::Warning,
            confirm,
            Some(&detail),
            &[confirm, "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = answer.await.unwrap_or(1);
            let _ = this.update_in(cx, |this, window, cx| {
                this.prompting = false;
                if answer == 0 {
                    this.spawn_removal(path, trashed, window, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Remove an entry off the UI thread, then drop everything that pointed at
    /// it. A buffer for a file that no longer exists cannot stay open.
    fn spawn_removal(
        &mut self,
        path: PathBuf,
        trashed: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = path.clone();
        let task = cx.background_executor().spawn(async move {
            if trashed {
                fs_op::trash(&target)
            } else {
                fs_op::delete(&target)
            }
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(()) => {
                        let dir = path
                            .parent()
                            .map(Path::to_path_buf)
                            .unwrap_or_else(|| path.clone());
                        this.forget_paths(&path);
                        this.rescan(dir, cx);
                        this.update_title(window);
                        let verb = if trashed { "Moved to Trash" } else { "Deleted" };
                        this.toast(&format!("{verb} {}", name(&path)), cx);
                    }
                    Err(error) => this.error(error.to_string(), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Forget everything that lived under `path`, which is no longer on disk.
    fn forget_paths(&mut self, path: &Path) {
        self.project
            .documents
            .retain(|key, _| !key.starts_with(path));
        self.project.tabs.retain(|tab| !tab.starts_with(path));
        if self
            .project
            .active
            .as_ref()
            .is_some_and(|key| key.starts_with(path))
        {
            self.project.active = None;
        }
        if self
            .project
            .image
            .as_ref()
            .is_some_and(|(key, _)| key.starts_with(path))
        {
            self.project.image = None;
        }
        self.project.expanded.retain(|key| !key.starts_with(path));
        self.project
            .directories
            .retain(|key, _| !key.starts_with(path));
    }

    /// Note where the settings window was and persist it alongside the main
    /// window's geometry.
    fn remember_settings_bounds(&mut self, bounds: Bounds<Pixels>) {
        self.settings_bounds = Some(bounds_values(bounds));
        self.save_window_state();
    }

    /// Persist both windows' geometry. Written whenever either window goes
    /// away, so the app never depends on quitting being the last thing to
    /// happen.
    fn save_window_state(&self) {
        WindowState {
            main: Some(bounds_values(self.main_bounds)),
            settings: self.settings_bounds,
        }
        .save(&self.window_file);
    }

    /// Re-key the state that pointed at `from`, now that it lives at `to`. A
    /// renamed folder takes its whole subtree with it.
    fn remap_paths(&mut self, from: &Path, to: &Path) {
        remap_project_paths(&mut self.project, from, to);
        for project in &mut self.parked {
            remap_project_paths(project, from, to);
        }
        self.settle_documents();
    }

    /// After a move, a buffer may now live under another project's root.
    fn settle_documents(&mut self) {
        let mut orphans = Vec::new();
        for project in std::iter::once(&mut self.project).chain(self.parked.iter_mut()) {
            let Some(root) = project.workspace.as_ref().map(|workspace| workspace.root.clone())
            else {
                continue;
            };
            let documents = std::mem::take(&mut project.documents);
            for (path, document) in documents {
                if path.starts_with(&root) {
                    project.documents.insert(path, document);
                } else {
                    project.tabs.retain(|tab| tab != &path);
                    project.pinned.remove(&path);
                    project.read_only.remove(&path);
                    project.disk_notice.remove(&path);
                    if project.active.as_ref() == Some(&path) {
                        project.active = None;
                    }
                    orphans.push((path, document));
                }
            }
        }
        for (path, document) in orphans {
            let home = std::iter::once(&mut self.project)
                .chain(self.parked.iter_mut())
                .find(|project| {
                    project
                        .workspace
                        .as_ref()
                        .is_some_and(|workspace| path.starts_with(&workspace.root))
                });
            if let Some(project) = home {
                if !project.tabs.contains(&path) {
                    project.tabs.push(path.clone());
                }
                project.documents.insert(path, document);
            }
        }
    }

    fn drop_tree_entry(
        &mut self,
        source: PathBuf,
        onto: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if source == onto {
            return;
        }
        let into = if self.is_tree_directory(&onto) {
            onto
        } else {
            match onto.parent() {
                Some(parent) => parent.to_path_buf(),
                None => return,
            }
        };
        if source.parent() == Some(into.as_path()) {
            return;
        }
        if source.is_dir() && into.starts_with(&source) {
            self.error("Cannot move a folder into itself".into(), cx);
            return;
        }
        let from_parent = source.parent().map(Path::to_path_buf);
        let dest_dir = into.clone();
        let moving = source.clone();
        let task = cx
            .background_executor()
            .spawn(async move { fs_op::paste(&moving, &into, true) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(dest) => {
                        this.quiet_write(&dest);
                        this.remap_paths(&source, &dest);
                        this.rebuild_rows();
                        if let Some(parent) = from_parent {
                            this.reload_directory(parent, cx);
                        }
                        this.rescan(dest_dir, cx);
                        this.update_title(window);
                        this.toast(&format!("Moved {}", name(&dest)), cx);
                    }
                    Err(error) => this.error(error.to_string(), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn is_tree_directory(&self, path: &Path) -> bool {
        std::iter::once(&self.project)
            .chain(self.parked.iter())
            .any(|project| {
                project.directories.contains_key(path)
                    || project.rows.iter().any(|row| {
                        row.entry.path == path && row.entry.kind == EntryKind::Directory
                    })
                    || project
                        .workspace
                        .as_ref()
                        .is_some_and(|workspace| workspace.root == path)
            })
    }

    /// Open the context menu over `target`, flipped when it would overhang the
    /// window's bottom or right edge.
    fn open_menu(
        &mut self,
        target: MenuTarget,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let size = window.viewport_size();
        let (x, y) = (f32::from(position.x), f32::from(position.y));
        let height = menu_height(&target);
        let x = if x + MENU_WIDTH + 8. > f32::from(size.width) {
            (x - MENU_WIDTH).max(8.)
        } else {
            x
        };
        let y = if y + height + 8. > f32::from(size.height) {
            (y - height).max(8.)
        } else {
            y
        };
        self.menu = Some(ContextMenu {
            selected: visible_menu_items(&target).first().copied().unwrap_or(0),
            target,
            position: point(px(x), px(y)),
        });
        cx.notify();
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu.take().is_some() {
            cx.notify();
        }
    }

    /// Whether a tab's buffer has edits that are not on disk.
    fn is_dirty(&self, path: &Path) -> bool {
        self.project
            .documents
            .get(path)
            .is_some_and(|doc| doc.dirty)
    }

    fn is_pinned(&self, path: &Path) -> bool {
        self.project.pinned.contains(path)
    }

    fn is_read_only(&self, path: &Path) -> bool {
        self.project.read_only.contains(path)
    }

    /// The tabs an entry would close, which is nothing for anything else. The
    /// bulk entries leave pinned tabs alone — that is what pinning is for.
    fn tabs_to_close(&self, item: MenuItem, path: &Path) -> Vec<PathBuf> {
        let tabs = &self.project.tabs;
        let Some(index) = tabs.iter().position(|tab| tab == path) else {
            return Vec::new();
        };
        let free = |tab: &PathBuf| !self.is_pinned(tab);
        match item {
            MenuItem::CloseTab => vec![path.to_path_buf()],
            MenuItem::CloseOthers => tabs
                .iter()
                .filter(|tab| tab.as_path() != path && free(tab))
                .cloned()
                .collect(),
            MenuItem::CloseLeft => tabs[..index]
                .iter()
                .filter(|tab| free(tab))
                .cloned()
                .collect(),
            MenuItem::CloseRight => tabs[index + 1..]
                .iter()
                .filter(|tab| free(tab))
                .cloned()
                .collect(),
            MenuItem::CloseClean => tabs
                .iter()
                .filter(|tab| !self.is_dirty(tab) && free(tab))
                .cloned()
                .collect(),
            MenuItem::CloseAll => tabs.iter().filter(|tab| free(tab)).cloned().collect(),
            _ => Vec::new(),
        }
    }

    /// Whether an entry can be taken here. Everything is available unless it
    /// would do nothing at all.
    fn menu_item_enabled(&self, item: MenuItem, path: &Path) -> bool {
        match item {
            MenuItem::Paste => self.clipboard.is_some(),
            MenuItem::CloseOthers
            | MenuItem::CloseLeft
            | MenuItem::CloseRight
            | MenuItem::CloseAll
            | MenuItem::CloseClean => !self.tabs_to_close(item, path).is_empty(),
            MenuItem::CopyRelativePath | MenuItem::RevealInTree => self.project.workspace.is_some(),
            _ => true,
        }
    }

    /// The label an entry shows here. Most are what the table says; the two
    /// that flip are the reason this is a call and not a lookup.
    fn menu_label(&self, item: MenuItem, path: &Path, label: &'static str) -> SharedString {
        match item {
            MenuItem::ToggleReadOnly if self.is_read_only(path) => "Make Tab Writable".into(),
            MenuItem::PinTab if self.is_pinned(path) => "Unpin Tab".into(),
            _ => label.into(),
        }
    }

    /// Move a dragged tab into another tab's place: it lands at that tab's
    /// index and everything between shifts one step back towards where it came
    /// from. Every position is reachable this way, including the last, and a
    /// one-slot drag moves in either direction.
    ///
    /// Nothing here reads the pointer. The tab the drop landed on is the one
    /// thing a drop reliably reports, and a rule built on anything finer — which
    /// half of a tab the pointer is over — depends on move events arriving for
    /// every tab on the way, which is exactly what could not be relied on.
    ///
    /// Pinned tabs stay at the front, so a drop can neither strand one among
    /// the unpinned ones nor put an unpinned one ahead of them.
    fn move_tab(&mut self, dragged: &Path, to: usize, cx: &mut Context<Self>) {
        self.tab_drop = None;
        let mut tabs = std::mem::take(&mut self.project.tabs);
        let Some(from) = tabs.iter().position(|tab| tab == dragged) else {
            self.project.tabs = tabs;
            return;
        };
        let tab = tabs.remove(from);
        let to = to.min(tabs.len());
        let pinned = tabs
            .iter()
            .filter(|tab| self.project.pinned.contains(*tab))
            .count();
        let to = if self.project.pinned.contains(&tab) {
            to.min(pinned)
        } else {
            to.max(pinned)
        };
        tabs.insert(to, tab);
        self.project.tabs = tabs;
        cx.notify();
    }

    /// Pinned tabs sit at the front. `sort_by_key` is stable, so each group
    /// keeps the order it already had.
    fn reorder_tabs(&mut self) {
        let pinned = self.project.pinned.clone();
        self.project.tabs.sort_by_key(|tab| !pinned.contains(tab));
    }

    /// Put a string on the system clipboard and say so, because the clipboard
    /// gives no sign of its own.
    fn copy_to_clipboard(&mut self, text: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.toast("Copied", cx);
    }

    /// Move the keyboard highlight to the next or previous visible entry.
    fn move_menu_selection(&mut self, down: bool) {
        let Some(menu) = self.menu.as_ref() else {
            return;
        };
        let visible = visible_menu_items(&menu.target);
        let current = visible.iter().position(|&index| index == menu.selected);
        let next = match (current, down) {
            (Some(current), true) => (current + 1).min(visible.len().saturating_sub(1)),
            (Some(current), false) => current.saturating_sub(1),
            (None, true) => 0,
            (None, false) => visible.len().saturating_sub(1),
        };
        let Some(&index) = visible.get(next) else {
            return;
        };
        if let Some(menu) = self.menu.as_mut() {
            menu.selected = index;
        }
    }

    fn run_menu_item(&mut self, item: MenuItem, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.menu.take() else {
            return;
        };
        if !self.menu_item_enabled(item, menu.target.path()) {
            cx.notify();
            return;
        }
        match menu.target {
            MenuTarget::Tree { path, .. } => self.run_tree_item(item, path, window, cx),
            MenuTarget::Changes { .. } => self.run_changes_item(item, window, cx),
            MenuTarget::Tab { path, .. } => self.run_tab_item(item, path, window, cx),
        }
    }

    /// The entries a tab offers.
    fn run_tab_item(
        &mut self,
        item: MenuItem,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match item {
            MenuItem::CloseTab
            | MenuItem::CloseOthers
            | MenuItem::CloseLeft
            | MenuItem::CloseRight
            | MenuItem::CloseClean
            | MenuItem::CloseAll => {
                let closing = self.tabs_to_close(item, &path);
                if !closing.is_empty() {
                    self.request(Next::CloseTabs(closing), window, cx);
                }
            }
            MenuItem::ToggleReadOnly => {
                if !self.project.read_only.remove(&path) {
                    self.project.read_only.insert(path);
                }
                cx.notify();
            }
            MenuItem::PinTab => {
                if !self.project.pinned.remove(&path) {
                    self.project.pinned.insert(path);
                }
                self.reorder_tabs();
                cx.notify();
            }
            MenuItem::CopyPath => {
                let text = path.to_string_lossy().into_owned();
                self.copy_to_clipboard(text, cx);
            }
            MenuItem::CopyRelativePath => {
                let text = self.relative_path(&path);
                self.copy_to_clipboard(text, cx);
            }
            MenuItem::RevealInTree => self.reveal_in_tree(path, window, cx),
            MenuItem::Reveal => {
                let target = path.clone();
                self.spawn_launch(move || fs_op::reveal(&target), cx);
            }
            MenuItem::OpenTerminal => {
                // A tab is a file; a terminal is useful at the folder holding
                // it rather than at the file.
                let dir = path.parent().map(Path::to_path_buf).unwrap_or(path);
                self.spawn_launch(move || fs_op::open_terminal(&dir), cx);
            }
            // Nothing else is offered on that surface.
            _ => cx.notify(),
        }
    }

    /// The entries a folder in the tree offers.
    fn run_tree_item(
        &mut self,
        item: MenuItem,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match item {
            MenuItem::NewFile => self.begin_create(path, EntryKind::File, window, cx),
            MenuItem::NewFolder => self.begin_create(path, EntryKind::Directory, window, cx),
            MenuItem::FindInFolder => self.show_search_in(path, window, cx),
            MenuItem::Rename => self.begin_rename(path, window, cx),
            MenuItem::Cut | MenuItem::Copy => {
                let cut = item == MenuItem::Cut;
                self.clipboard = Some(FileClipboard {
                    path: path.clone(),
                    cut,
                });
                let verb = if cut { "Cut" } else { "Copied" };
                self.toast(&format!("{verb} {}", name(&path)), cx);
            }
            MenuItem::Duplicate => {
                let source = path.clone();
                self.spawn_change(path, false, move || fs_op::duplicate(&source), window, cx);
            }
            MenuItem::Paste => {
                let Some(clipboard) = self.clipboard.clone() else {
                    return;
                };
                let into = path.clone();
                self.spawn_change(
                    path,
                    false,
                    move || fs_op::paste(&clipboard.path, &into, clipboard.cut),
                    window,
                    cx,
                );
                // A cut is spent once it lands; a copy can be pasted again.
                if clipboard.cut {
                    self.clipboard = None;
                }
            }
            // Trashing is recoverable, so it only stops to warn about edits
            // that are not on disk yet. Deleting never is.
            MenuItem::Trash if self.unsaved_under(&path) > 0 => {
                self.confirm_removal(path, true, window, cx)
            }
            MenuItem::Trash => self.spawn_removal(path, true, window, cx),
            MenuItem::Delete => self.confirm_removal(path, false, window, cx),
            MenuItem::Reveal => {
                let target = path.clone();
                self.spawn_launch(move || fs_op::reveal(&target), cx);
            }
            MenuItem::OpenDefault => {
                let target = path.clone();
                self.spawn_launch(move || fs_op::open_default(&target), cx);
            }
            MenuItem::OpenTerminal => {
                let target = path.clone();
                self.spawn_launch(move || fs_op::open_terminal(&target), cx);
            }
            // The remaining entries belong to other surfaces.
            _ => cx.notify(),
        }
    }

    /// The entries a file's changes offer. Leaving is the only one so far.
    fn run_changes_item(&mut self, item: MenuItem, window: &mut Window, cx: &mut Context<Self>) {
        match item {
            MenuItem::HideChanges => self.toggle_diff(window, cx),
            // Nothing else is offered on that surface.
            _ => cx.notify(),
        }
    }

    /// Splice a text field into the tree for a new entry under `parent`. The
    /// entry is created on Enter; Escape creates nothing.
    fn begin_create(
        &mut self,
        parent: PathBuf,
        kind: EntryKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.project.workspace.is_none() || self.project_loading || self.prompting {
            return;
        }
        self.editing = None;
        // The field needs a row to sit in, so open the folder it will go into.
        // A folder that was never opened has no children cached yet, which is
        // exactly the case `toggle_directory` also reads on demand.
        if !self.project.expanded.contains(&parent) {
            self.toggle_directory(parent.clone(), cx);
        }
        let placeholder = match kind {
            EntryKind::Directory => "Folder name",
            EntryKind::File => "File name",
        };
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let subscription = Self::subscribe_edit(&input, window, cx);
        self.editing = Some(InlineEdit {
            parent,
            kind,
            renaming: None,
            row: 0,
            input: input.clone(),
            _subscription: subscription,
        });
        self.rebuild_rows();
        if let Some(edit) = &self.editing {
            self.project
                .tree_scroll
                .scroll_to_item(edit.row, ScrollStrategy::Nearest);
        }
        input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    /// Turn a tree row into a text field holding the entry's current name. The
    /// row already exists, so unlike creating there is nothing to splice in.
    fn begin_rename(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.project_loading || self.prompting || self.editing.is_some() {
            return;
        }
        let Some(row) = self.project.rows.iter().find(|row| row.entry.path == path) else {
            return;
        };
        let (kind, name) = (row.entry.kind, row.entry.name.clone());
        let Some(parent) = path.parent().map(Path::to_path_buf) else {
            return;
        };
        let input = cx.new(|cx| InputState::new(window, cx));
        // Preselect the whole name, so typing replaces it the way Finder does.
        input.update(cx, |input, cx| {
            input.set_value(name, window, cx);
            input.select_all(window, cx);
        });
        let subscription = Self::subscribe_edit(&input, window, cx);
        self.editing = Some(InlineEdit {
            parent,
            kind,
            renaming: Some(path),
            row: 0,
            input: input.clone(),
            _subscription: subscription,
        });
        self.rebuild_rows();
        if let Some(edit) = &self.editing {
            self.project
                .tree_scroll
                .scroll_to_item(edit.row, ScrollStrategy::Nearest);
        }
        input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    /// Enter and Escape for the field, shared by naming and renaming.
    fn subscribe_edit(
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe_in(
            input,
            window,
            move |this: &mut Self, input, event: &InputEvent, window, cx| {
                if this
                    .editing
                    .as_ref()
                    .is_none_or(|edit| &edit.input != input)
                {
                    return;
                }
                match event {
                    InputEvent::PressEnter { .. } => this.commit_edit(window, cx),
                    // A row that was never named should not linger once the
                    // pointer moves on.
                    InputEvent::Blur => this.cancel_create(window, cx),
                    _ => {}
                }
            },
        )
    }

    /// Keep `Document::dirty` in step with the buffer it belongs to. The
    /// document is found by editor rather than by path, so renaming an open
    /// file does not have to tear the subscription down and build it again —
    /// which would be a trap, because a `Subscription` unsubscribes whatever
    /// currently sits at its key when it is dropped.
    fn subscribe_document(
        editor: &Entity<EditorState>,
        project_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        let editor = editor.clone();
        cx.subscribe_in(
            &editor,
            window,
            move |this, editor, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::Change) {
                    if let Some(doc) = this
                        .project_mut(project_id)
                        .and_then(|p| p.documents.values_mut().find(|doc| &doc.editor == editor))
                    {
                        doc.dirty = editor.read(cx).value() != doc.saved;
                    }
                    this.update_title(window);
                    cx.notify();
                }
            },
        )
    }

    fn cancel_create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing.take().is_none() {
            return;
        }
        self.rebuild_rows();
        self.tree_focus.focus(window, cx);
        cx.notify();
    }

    fn commit_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = &self.editing else {
            return;
        };
        let name = edit.input.read(cx).value().to_string();
        let (parent, kind, renaming) = (edit.parent.clone(), edit.kind, edit.renaming.clone());
        self.editing = None;
        self.rebuild_rows();
        self.tree_focus.focus(window, cx);
        match renaming {
            Some(source) => self.spawn_rename(source, name, window, cx),
            None => {
                let dir = parent.clone();
                let create = move || match kind {
                    EntryKind::Directory => fs_op::create_dir(&dir, &name),
                    EntryKind::File => fs_op::create_file(&dir, &name),
                };
                self.spawn_change(parent, kind == EntryKind::File, create, window, cx);
            }
        }
    }

    /// The font code is drawn in, falling back to the one in the binary.
    fn code_font(&self) -> SharedString {
        self.settings
            .code_font_family
            .clone()
            .unwrap_or_else(|| "JetBrains Mono".to_string())
            .into()
    }

    /// Keep an open changes view pointed at whatever file is active now, so a
    /// review can walk the tree instead of reopening the view per file.
    fn follow_diff(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.project.diff.is_some() {
            self.load_diff(window, cx);
        }
    }

    /// Open the folders above a file in the tree and put the cursor on it, so
    /// the panel shows where the file lives.
    fn reveal_in_tree(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.project.workspace.as_ref().map(|w| w.root.clone()) else {
            return;
        };
        if !path.starts_with(&root) {
            return;
        }
        // Every folder between the root and the file has to be open for its
        // row to exist at all.
        let mut folders: Vec<PathBuf> = path
            .parent()
            .into_iter()
            .flat_map(|parent| parent.ancestors())
            .take_while(|dir| dir.starts_with(&root))
            .map(Path::to_path_buf)
            .collect();
        folders.reverse();
        for folder in &folders {
            self.project.expanded.insert(folder.clone());
        }
        let project_id = self.project.id;
        let ignored = self.settings.ignored.clone();
        // Every folder on the way down is re-read rather than only the ones
        // the tree has never opened: a folder added since its parent was last
        // read is not in that parent's listing, so the row would not exist even
        // though the folder itself would read fine.
        let task = cx.background_executor().spawn(async move {
            let mut read = Vec::new();
            for folder in folders {
                if let Ok(children) = tree::children(&folder, &ignored) {
                    read.push((folder, children));
                }
            }
            read
        });
        cx.spawn_in(window, async move |this, cx| {
            let read = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if let Some(project) = this.project_mut(project_id) {
                    for (folder, children) in read {
                        project.directories.insert(folder, children);
                    }
                }
                if this.project.id == project_id {
                    this.select_row_for(&path, window, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Put the tree's cursor on a row and bring it into view.
    fn select_row_for(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        self.rebuild_rows();
        if let Some(index) = self
            .project
            .rows
            .iter()
            .position(|row| row.entry.path == path)
        {
            self.project.selected_row = index;
            self.project
                .tree_scroll
                .scroll_to_item(index, ScrollStrategy::Center);
        }
        self.tree_focus.focus(window, cx);
        cx.notify();
    }

    /// Put the caret back on the code, or on the tree when nothing is open.
    fn focus_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(doc) = self
            .project
            .active
            .as_ref()
            .and_then(|path| self.project.documents.get(path))
        {
            doc.editor.focus_handle(cx).focus(window, cx);
        } else {
            self.tree_focus.focus(window, cx);
        }
    }

    /// The buffer being edited, if one is. An image preview has none.
    fn active_editor(&self) -> Option<Entity<EditorState>> {
        let path = self.project.active.as_ref()?;
        self.project
            .documents
            .get(path)
            .map(|document| document.editor.clone())
    }

    /// Run a command against the buffer being edited.
    ///
    /// The multi-cursor commands are the editor's own — it holds the
    /// selections, and the edit they make has to go through the same path a
    /// keystroke does — so this is only the reaching and the redrawing.
    fn on_active_buffer(
        &mut self,
        cx: &mut Context<Self>,
        command: impl FnOnce(&mut InputBaseState, &mut Context<InputBaseState>),
    ) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        let base = editor.read(cx).base_state().clone();
        base.update(cx, |base, cx| command(base, cx));
        cx.notify();
    }

    /// What an opening bracket does: a pair around the selection, or an empty
    /// pair with the caret between the two.
    ///
    /// Both characters go in at once, so an opener is one edit and one step on
    /// the undo stack rather than two.
    fn open_pair(
        &mut self,
        open: &'static str,
        close: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        let base = editor.read(cx).base_state().clone();
        if self.type_at_every_cursor(&base, open, window, cx) {
            return;
        }
        let value = base.read(cx).value().to_string();
        let selection = base.read(cx).selected_range();
        let Some(selected) = value.get(selection.clone()) else {
            return;
        };
        // Wrapping a selection is always what was meant. An empty pair is not:
        // in front of a word the closer would land where the rest of the word
        // has to go, so the opener is typed on its own.
        if selected.is_empty() && !allows_autoclose(value[selection.start..].chars().next()) {
            base.update(cx, |base, cx| base.replace(open, window, cx));
            cx.notify();
            return;
        }
        // Wrapping leaves the caret after the pair; an empty pair leaves it
        // between the two.
        let caret = if selected.is_empty() {
            selection.start + open.len()
        } else {
            selection.start + open.len() + selected.len() + close.len()
        };
        let text = format!("{open}{selected}{close}");
        base.update(cx, |base, cx| {
            base.replace(text, window, cx);
            base.set_selected_range(caret..caret, cx);
        });
        cx.notify();
    }

    /// A quote closes itself, so it is its own pair.
    fn pair_quote(&mut self, quote: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        let base = editor.read(cx).base_state().clone();
        if self.type_at_every_cursor(&base, quote, window, cx) {
            return;
        }
        let value = base.read(cx).value().to_string();
        let selection = base.read(cx).selected_range();
        let follows = || {
            value
                .get(selection.start..)
                .is_some_and(|rest| rest.starts_with(quote))
        };
        if selection.is_empty() && follows() {
            self.step_over(&base, selection.start + quote.len(), cx);
            return;
        }
        // A quote after a word is not opening a quote — it is an apostrophe,
        // or a lifetime. Zed asks the language; a word character is what that
        // amounts to for the languages here.
        let after_word =
            selection.is_empty() && is_word_char(value[..selection.start].chars().next_back());
        if after_word {
            base.update(cx, |base, cx| base.replace(quote, window, cx));
            cx.notify();
            return;
        }
        self.open_pair(quote, quote, window, cx);
    }

    /// A closing bracket typed where one already is steps over it, and
    /// anywhere else is typed as itself.
    fn skip_closer(&mut self, close: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.active_editor() else {
            return;
        };
        let base = editor.read(cx).base_state().clone();
        if self.type_at_every_cursor(&base, close, window, cx) {
            return;
        }
        let value = base.read(cx).value().to_string();
        let selection = base.read(cx).selected_range();
        let follows = || {
            value
                .get(selection.start..)
                .is_some_and(|rest| rest.starts_with(close))
        };
        if selection.is_empty() && follows() {
            self.step_over(&base, selection.start + close.len(), cx);
            return;
        }
        base.update(cx, |base, cx| base.replace(close, window, cx));
        cx.notify();
    }

    /// Move the caret past a bracket that is already there.
    fn step_over(&self, base: &Entity<InputBaseState>, caret: usize, cx: &mut Context<Self>) {
        base.update(cx, |base, cx| base.set_selected_range(caret..caret, cx));
        cx.notify();
    }

    /// Type one character at every cursor, when there is more than one.
    ///
    /// A pair is a decision about a single selection: with several there is
    /// nothing to wrap and nothing to step over, so the character goes in at
    /// each cursor like any other. Answering here rather than letting the
    /// single-selection paths below run matters for a second reason — those go
    /// through `replace`, which leaves one selection behind and would take the
    /// other cursors with it.
    ///
    /// Answers whether it typed, so the caller can stop.
    fn type_at_every_cursor(
        &self,
        base: &Entity<InputBaseState>,
        text: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if base.read(cx).selected_ranges().len() <= 1 {
            return false;
        }
        base.update(cx, |base, cx| {
            base.replace_in_every_selection(text, window, cx)
        });
        cx.notify();
        true
    }

    /// `⌘/`: comment out every line the selection touches, or bring them back.
    ///
    /// The editor offers no hook for this and no way to replace an arbitrary
    /// range, so it is done by selecting the lines and replacing the selection
    /// — which is also what puts it on the undo stack as one step.
    fn toggle_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.project.active.clone() else {
            return;
        };
        let Some(marker) = buffer::line_comment(buffer::language(&path)) else {
            // Plenty of languages only have a block form; saying so beats
            // inserting a marker the file cannot use.
            self.toast("This language has only block comments", cx);
            return;
        };
        // An image has no buffer to comment.
        let Some(editor) = self
            .project
            .documents
            .get(&path)
            .map(|document| document.editor.clone())
        else {
            return;
        };
        let base = editor.read(cx).base_state().clone();
        let text = base.read(cx).value().to_string();
        // Every cursor's lines, from the first to the last: commenting is one
        // edit over a block, not one per caret, so the block is what the span
        // of the set covers. A single cursor gives the same range it always
        // did.
        let selections = base.read(cx).selected_ranges();
        let (Some(first), Some(last)) = (selections.first(), selections.last()) else {
            return;
        };
        let lines = buffer::line_range(&text, first.start..last.end);
        let Some(block) = text.get(lines.clone()) else {
            return;
        };
        let toggled = buffer::toggle_comments(block, marker);
        let reselect = lines.start..lines.start + toggled.len();
        base.update(cx, |base, cx| {
            base.set_selected_range(lines, cx);
            base.replace(toggled, window, cx);
            // Left selected, so pressing again undoes what this one did.
            base.set_selected_range(reselect, cx);
        });
        cx.notify();
    }

    fn toggle_block_comment(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.project.active.clone() else {
            return;
        };
        let Some((open, close)) = buffer::block_comment(buffer::language(&path)) else {
            self.toast("This language has no block comments", cx);
            return;
        };
        self.replace_active_lines(window, cx, |block| {
            buffer::toggle_block_comments(block, open, close)
        });
    }

    fn duplicate_lines(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_active_text(window, cx, |text, selection| {
            Some(buffer::duplicate_lines(text, selection))
        });
    }

    fn move_lines(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.replace_active_text(window, cx, |text, selection| {
            buffer::move_lines(text, selection, down)
        });
    }

    fn replace_active_lines(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        change: impl FnOnce(&str) -> String,
    ) {
        let Some(path) = self.project.active.clone() else {
            return;
        };
        let Some(editor) = self
            .project
            .documents
            .get(&path)
            .map(|document| document.editor.clone())
        else {
            return;
        };
        let base = editor.read(cx).base_state().clone();
        let text = base.read(cx).value().to_string();
        let selections = base.read(cx).selected_ranges();
        let (Some(first), Some(last)) = (selections.first(), selections.last()) else {
            return;
        };
        let lines = buffer::line_range(&text, first.start..last.end);
        let Some(block) = text.get(lines.clone()) else {
            return;
        };
        let next = change(block);
        let reselect = lines.start..lines.start + next.len();
        base.update(cx, |base, cx| {
            base.set_selected_range(lines, cx);
            base.replace(next, window, cx);
            base.set_selected_range(reselect, cx);
        });
        cx.notify();
    }

    fn replace_active_text(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        change: impl FnOnce(&str, std::ops::Range<usize>) -> Option<(String, std::ops::Range<usize>)>,
    ) {
        let Some(path) = self.project.active.clone() else {
            return;
        };
        let Some(editor) = self
            .project
            .documents
            .get(&path)
            .map(|document| document.editor.clone())
        else {
            return;
        };
        let base = editor.read(cx).base_state().clone();
        let text = base.read(cx).value().to_string();
        let selections = base.read(cx).selected_ranges();
        let (Some(first), Some(last)) = (selections.first(), selections.last()) else {
            return;
        };
        let Some((next, reselect)) = change(&text, first.start..last.end) else {
            return;
        };
        base.update(cx, |base, cx| {
            base.set_selected_range(0..text.len(), cx);
            base.replace(next, window, cx);
            base.set_selected_range(reselect, cx);
        });
        cx.notify();
    }

    /// `⌥⌘B`: show what the caret's line was last written by, or stop showing
    /// it.
    fn toggle_blame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.blame.take().is_some() {
            cx.notify();
            return;
        }
        self.read_blame(window, cx);
    }

    /// Read the blame of the file being read, against what its buffer holds.
    ///
    /// The buffer rather than the file, because blame is looked up by line and
    /// the lines are the buffer's: a file with an edit in it would otherwise
    /// have everything below that edit attributed to the wrong commit.
    fn read_blame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.project.active.clone() else {
            return;
        };
        let Some(document) = self.project.documents.get(&path) else {
            // An image preview has no lines to blame.
            return;
        };
        let text = document.editor.read(cx).value().to_string();
        self.blame_request += 1;
        let request = self.blame_request;
        let blamed = path.clone();
        let task = cx
            .background_executor()
            .spawn(async move { blame::run(&blamed, &text) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, _, cx| {
                // The user may have moved on to another file while this was
                // being read, in which case its own read is the one that counts.
                if this.blame_request != request {
                    return;
                }
                match result {
                    Ok(blame) => this.blame = Some(BlameView { path, blame }),
                    Err(e) => {
                        this.blame = None;
                        this.error(format!("Could not read blame: {e}"), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// What the blame strip says for the line the caret is on, if it is on and
    /// there is anything to say.
    fn blame_text(&self, cx: &App) -> Option<String> {
        let view = self.blame.as_ref()?;
        let path = self.project.active.as_ref()?;
        if view.path != *path {
            return None;
        }
        let document = self.project.documents.get(path)?;
        let line = document
            .editor
            .read(cx)
            .base_state()
            .read(cx)
            .cursor_position()
            .line as usize;
        let blamed = view.blame.line(line)?;
        if blamed.uncommitted() {
            return Some("Not committed yet".to_string());
        }
        Some(format!(
            "{} · {}, {} — {}",
            blamed.short(),
            blamed.author,
            relative_time(blamed.time),
            blamed.summary
        ))
    }

    /// `⇧⌘D`: show the active file as its changes against HEAD, or go back to
    /// reading it.
    fn toggle_diff(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.project.diff.take().is_some() {
            self.focus_editor(window, cx);
            cx.notify();
            return;
        }
        // The tree keeps the keyboard: the diff is read-only, and picking the
        // next file moves the view on to its changes.
        self.tree_focus.focus(window, cx);
        self.load_diff(window, cx);
    }

    /// Read the active file's changes. Called when the view is turned on and
    /// again whenever the file it is showing changes, so it follows a review
    /// rather than having to be reopened per file.
    fn load_diff(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(path), Some(workspace)) =
            (self.project.active.clone(), self.project.workspace.clone())
        else {
            self.project.diff = None;
            cx.notify();
            return;
        };
        let project_id = self.project.id;
        let request = self.project.diff.as_ref().map_or(0, |view| view.request) + 1;
        self.project.diff = Some(DiffView {
            path: path.clone(),
            loading: true,
            request,
            ..DiffView::default()
        });
        cx.notify();
        let task = cx
            .background_executor()
            .spawn(async move { diff::for_file(&workspace.root, &path) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, _, cx| {
                let Some(project) = this.project_mut(project_id) else {
                    return;
                };
                let Some(view) = project.diff.as_mut() else {
                    return;
                };
                // A slower answer for a file the user has already left is not
                // the answer to show.
                if view.request != request {
                    return;
                }
                view.loading = false;
                match result {
                    Ok(diff) => {
                        view.lines = diff.lines;
                        view.untracked = diff.untracked;
                        view.error = None;
                    }
                    Err(error) => view.error = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The open files, as a strip above the editor. Hidden while only one
    /// file is open, which is the single-file mode the requirements ask to
    /// keep: one file looks exactly as it did before tabs existed.
    fn render_tabs(&self, cx: &Context<Self>) -> AnyElement {
        let end = self.project.tabs.len();
        div()
            .id("tabs")
            .flex_shrink_0()
            .w_full()
            .h(px(TAB_HEIGHT))
            .flex()
            .items_center()
            .children(
                self.project
                    .tabs
                    .iter()
                    .enumerate()
                    .flat_map(|(index, path)| {
                        // A caret sits where the dragged tab would land, so the
                        // drop is not a guess.
                        let caret = (self.tab_drop == Some(index)).then(|| Self::tab_caret(cx));
                        caret
                            .into_iter()
                            .chain(std::iter::once(self.render_tab(index, path, cx)))
                    })
                    .chain((self.tab_drop == Some(end)).then(|| Self::tab_caret(cx))),
            )
            .into_any_element()
    }

    /// The insertion caret drawn between tabs while one is being dragged.
    fn tab_caret(cx: &Context<Self>) -> AnyElement {
        div()
            .w(px(2.))
            .h_full()
            .flex_shrink_0()
            .bg(cx.theme().accent_foreground)
            .into_any_element()
    }

    /// One tab: the file's name, a mark when it has edits on disk to match,
    /// and the way to close it.
    fn render_tab(&self, index: usize, path: &PathBuf, cx: &Context<Self>) -> AnyElement {
        let selected = self.project.active.as_ref() == Some(path);
        let dirty = self
            .project
            .documents
            .get(path)
            .is_some_and(|doc| doc.dirty);
        let open = path.clone();
        let close = path.clone();
        let menu_path = path.clone();
        div()
            .id(("tab", index))
            .role(Role::Button)
            .aria_label(name(path))
            .aria_selected(selected)
            .h_full()
            .min_w_0()
            .max_w(px(200.))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .flex_shrink_0()
            .border_r_1()
            .border_color(cx.theme().border)
            .cursor_pointer()
            .text_size(ui(11.))
            .text_color(if selected {
                cx.theme().foreground
            } else {
                cx.theme().muted_foreground
            })
            .when(selected, |el| el.bg(cx.theme().list_active))
            .hover(|el| el.bg(cx.theme().list_hover))
            .on_click(
                cx.listener(move |this, _, window, cx| this.open_file(open.clone(), window, cx)),
            )
            // Dragging a tab decides where it lands from which half of this
            // one the pointer is on, the way an insertion caret reads.
            // This only drives the caret. Where the tab actually lands comes
            // from the drop itself, so a move event that never arrives costs a
            // caret and nothing else.
            .on_drag_move(cx.listener(move |this, _: &DragMoveEvent<TabDrag>, _, cx| {
                if this.tab_drop != Some(index) {
                    this.tab_drop = Some(index);
                    cx.notify();
                }
            }))
            .on_drag(
                TabDrag {
                    path: path.clone(),
                    label: name(path).into(),
                },
                |drag, _, _, cx| {
                    cx.new(|_| TabDragPreview {
                        label: drag.label.clone(),
                    })
                },
            )
            .on_drop(cx.listener(move |this, drag: &TabDrag, _, cx| {
                this.move_tab(&drag.path, index, cx);
            }))
            // The menu acts on the tab that was clicked, which is not
            // necessarily the one showing.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.open_menu(
                        MenuTarget::Tab {
                            path: menu_path.clone(),
                            index,
                        },
                        event.position,
                        window,
                        cx,
                    );
                    cx.stop_propagation();
                }),
            )
            .child(div().flex_1().min_w_0().truncate().child(name(path)))
            .when(dirty, |el| {
                el.child(
                    Icon::new(IconName::Asterisk)
                        .xsmall()
                        .text_color(cx.theme().warning),
                )
            })
            .child(
                div()
                    .id(("tab-close", index))
                    .role(Role::Button)
                    .aria_label("Close")
                    .size(px(14.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_color(cx.theme().muted_foreground)
                    .hover(|el| el.text_color(cx.theme().foreground))
                    // Without this the tab underneath would take the same
                    // click and reopen the file being closed.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.request(Next::CloseTabs(vec![close.clone()]), window, cx)
                    }))
                    .child(Icon::new(IconName::Close).xsmall()),
            )
            .into_any_element()
    }

    /// The active file's changes, in place of the editor.
    fn render_diff(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(view) = self.project.diff.as_ref() else {
            return div().into_any_element();
        };
        let added = view
            .lines
            .iter()
            .filter(|line| line.change == diff::Change::Added)
            .count();
        let removed = view
            .lines
            .iter()
            .filter(|line| line.change == diff::Change::Removed)
            .count();
        let summary = if view.loading {
            "Reading…".to_string()
        } else if let Some(error) = &view.error {
            error.clone()
        } else if view.lines.is_empty() {
            "No changes against HEAD".to_string()
        } else if view.untracked {
            format!("Not tracked by git yet · {added} added")
        } else {
            format!("+{added} −{removed}")
        };
        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_col()
            // This surface is the app's own, so it gets the app's menu rather
            // than the native one the editor brings with it.
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    let Some(path) = this.project.active.clone() else {
                        return;
                    };
                    this.open_menu(MenuTarget::Changes { path }, event.position, window, cx);
                    cx.stop_propagation();
                }),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .px_4()
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_3()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .text_size(ui(11.))
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(self.relative_path(&view.path)),
                    )
                    .child(summary)
                    // The way out, in the open. Escape and the shortcut do
                    // the same thing, but nothing on screen said so.
                    .child(Self::icon_button(
                        "diff-close",
                        IconName::Close,
                        "Hide file changes",
                        |this, window, cx| this.toggle_diff(window, cx),
                        cx,
                    )),
            )
            .when(!view.lines.is_empty(), |el| {
                el.child(
                    uniform_list(
                        "diff",
                        view.lines.len(),
                        cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                            range.map(|index| this.render_diff_row(index, cx)).collect()
                        }),
                    )
                    .track_scroll(&view.scroll)
                    .flex_1(),
                )
            })
            .into_any_element()
    }

    /// One row of the diff: both line numbers, then the line itself.
    fn render_diff_row(&self, index: usize, cx: &Context<Self>) -> AnyElement {
        let Some(line) = self
            .project
            .diff
            .as_ref()
            .and_then(|view| view.lines.get(index))
        else {
            return div().into_any_element();
        };
        if line.change == diff::Change::Hunk {
            return div()
                .w_full()
                .h(px(24.))
                .px_4()
                .flex()
                .items_center()
                .bg(cx.theme().list_hover)
                .text_size(ui(11.))
                .text_color(cx.theme().muted_foreground)
                .child(div().min_w_0().truncate().child(line.text.clone()))
                .into_any_element();
        }
        let (background, marker) = match line.change {
            diff::Change::Added => (Some(cx.theme().success.opacity(0.16)), "+"),
            diff::Change::Removed => (Some(cx.theme().danger.opacity(0.16)), "−"),
            _ => (None, " "),
        };
        div()
            .w_full()
            .h(px(22.))
            .flex()
            .items_center()
            .font_family(self.code_font())
            .text_size(px(self.settings.code_font_size))
            .when_some(background, |el, background| el.bg(background))
            .child(Self::diff_number(line.old, cx))
            .child(Self::diff_number(line.new, cx))
            .child(
                div()
                    .w(px(18.))
                    .flex_shrink_0()
                    .flex()
                    .justify_center()
                    .text_color(cx.theme().muted_foreground)
                    .child(marker),
            )
            .child(div().flex_1().min_w_0().truncate().child(line.text.clone()))
            .into_any_element()
    }

    /// A gutter cell. Empty on the side a line does not exist on, which is what
    /// keeps the two columns readable.
    fn diff_number(number: Option<u32>, cx: &Context<Self>) -> AnyElement {
        div()
            .w(px(46.))
            .flex_shrink_0()
            .pr_2()
            .flex()
            .justify_end()
            .text_color(cx.theme().muted_foreground)
            .child(number.map(|number| number.to_string()).unwrap_or_default())
            .into_any_element()
    }

    /// `Find in Folder`: the project search narrowed to one subtree.
    fn show_search_in(&mut self, dir: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.show_search(false, window, cx);
        if self.panel != Some(Panel::Search) {
            return;
        }
        self.search.scope = Some(dir);
        self.start_search(cx);
        cx.notify();
    }

    /// `⌘,`: the settings window. Focuses the one already open rather than
    /// opening a second, and reopens it if it was closed.
    fn show_settings(&mut self, cx: &mut Context<Self>) {
        if let Some(window) = self.settings_window
            && window
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return;
        }
        let bounds = self
            .settings_bounds
            .and_then(|values| restore_bounds(values, SETTINGS_WINDOW_MIN))
            .unwrap_or_else(|| WindowBounds::centered(SETTINGS_WINDOW_DEFAULT, cx));
        let folio = cx.entity().downgrade();
        let settings = self.settings.clone();
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(bounds),
                window_min_size: Some(SETTINGS_WINDOW_MIN),
                // The title bar is drawn by the app, exactly as the main
                // window does it. Leaving it to AppKit instead makes it use
                // the system titlebar material, which samples what is behind
                // the window and lands nowhere near the theme.
                titlebar: Some(TitlebarOptions {
                    title: Some("Settings".into()),
                    appears_transparent: true,
                    traffic_light_position: None,
                }),
                app_owns_titlebar_drag: true,
                is_resizable: true,
                is_movable: true,
                ..Default::default()
            },
            move |window, cx| {
                let view = cx.new(|cx| SettingsView::new(folio.clone(), settings, window, cx));
                // Recorded as the window goes away, so the geometry survives
                // even when the app is not quit from this window.
                window.on_window_should_close(cx, move |window, cx| {
                    let bounds = window.window_bounds().get_bounds();
                    if let Some(folio) = folio.upgrade() {
                        folio.update(cx, |folio, _| folio.remember_settings_bounds(bounds));
                    }
                    true
                });
                cx.new(|cx| Root::new(view, window, cx))
            },
        );
        match opened {
            Ok(window) => {
                self.settings_window = Some(window);
                // Kept so `apply_settings` can tell the form to repaint. The
                // window callback runs inside this update, so the view is
                // reached through the opened root rather than registered from
                // within it.
                self.settings_view = window
                    .update(cx, |root, _, _| {
                        root.view().clone().downcast::<SettingsView>().ok()
                    })
                    .ok()
                    .flatten();
            }
            Err(error) => self.error(format!("Could not open the settings: {error}"), cx),
        }
        cx.notify();
    }

    /// The About window. It holds the name and the version and nothing that
    /// needs updating: automatic updates are not part of this application.
    /// Where a new version would be announced. There is no feed behind this and
    /// no update to install from it: the packaging this project has signs
    /// ad-hoc for one machine, so what a version check can honestly do today is
    /// open the page that lists the versions.
    fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        cx.open_url(RELEASES);
    }

    fn show_about(&mut self, cx: &mut Context<Self>) {
        if let Some(window) = self.about_window
            && window
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return;
        }
        let bounds = WindowBounds::centered(size(px(360.), px(220.)), cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(bounds),
                // Drawn by the app like the settings window's, so it carries
                // the theme rather than AppKit's title bar material. Fixed in
                // size, because there is nothing in it to make room for.
                titlebar: Some(TitlebarOptions {
                    title: Some("About Folio".into()),
                    appears_transparent: true,
                    traffic_light_position: None,
                }),
                app_owns_titlebar_drag: true,
                is_resizable: false,
                is_movable: true,
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|_| AboutView);
                cx.new(|cx| Root::new(view, window, cx))
            },
        );
        match opened {
            Ok(window) => self.about_window = Some(window),
            Err(error) => self.error(format!("Could not open the About window: {error}"), cx),
        }
        cx.notify();
    }

    /// Everything the settings window changes lands here: clamp, apply, then
    /// persist. Writing the file is small enough to do on every step.
    fn apply_settings(&mut self, cx: &mut Context<Self>) {
        self.settings = self.settings.clone().clamped();
        // Only the ignore rules invalidate cached directory listings.
        let ignored_changed = self.settings.ignored != self.applied_ignored;
        self.applied_ignored = self.settings.ignored.clone();
        // Every window shares one theme, and the change may have come from
        // either of them, so apply it without borrowing a window and repaint
        // them all.
        sync_appearance(self.appearance, &self.settings, None, cx);
        // These two settings are the stored state the toggles write back.
        self.sidebar = self.settings.sidebar;
        self.activity_bar = self.settings.activity_bar;
        // Editors pick the new wrap up on the next frame of the main window.
        if ignored_changed {
            self.reload_tree(cx);
        }
        // Deferred: the change may have come *from* the settings window, and
        // updating a view that is already mid-update is a re-entrant borrow.
        if let Some(view) = self.settings_view.clone() {
            let settings = self.settings.clone();
            cx.defer(move |cx| {
                view.update(cx, |view, cx| view.set_settings(settings, cx));
            });
        }
        let file = self.settings_file.clone();
        let settings = self.settings.clone();
        let task = cx
            .background_executor()
            .spawn(async move { settings::save(&file, &settings) });
        cx.spawn(async move |this, cx| {
            if let Err(error) = task.await {
                let _ = this.update(cx, |this, cx| {
                    this.error(format!("Could not save settings: {error}"), cx)
                });
            }
        })
        .detach();
        cx.refresh_windows();
        cx.notify();
    }

    fn sync_soft_wrap(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let wrap = self.settings.soft_wrap;
        let editors: Vec<_> = std::iter::once(&self.project)
            .chain(self.parked.iter())
            .flat_map(|project| project.documents.values().map(|document| document.editor.clone()))
            .collect();
        for editor in editors {
            editor.update(cx, |state, cx| {
                state
                    .base_state()
                    .update(cx, |base, cx| base.set_soft_wrap(wrap, window, cx));
            });
        }
        self.applied_wrap = wrap;
    }

    /// The ignore rules changed, so every cached listing is suspect. One
    /// background pass re-reads the folders the tree is currently holding.
    fn reload_tree(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.project.workspace.as_ref().map(|w| w.root.clone()) else {
            return;
        };
        let project_id = self.project.id;
        let mut wanted: Vec<PathBuf> = self.project.expanded.iter().cloned().collect();
        wanted.push(root);
        wanted.sort();
        wanted.dedup();
        let ignored = self.settings.ignored.clone();
        let task = cx.background_executor().spawn(async move {
            let mut cache = HashMap::new();
            for dir in wanted {
                if let Ok(children) = tree::children(&dir, &ignored) {
                    cache.insert(dir, children);
                }
            }
            cache
        });
        cx.spawn(async move |this, cx| {
            let cache = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(project) = this.project_mut(project_id) {
                    project.directories = cache;
                }
                if this.project.id == project_id {
                    this.rebuild_rows();
                    this.reindex(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || self.project_loading || self.prompting {
            return;
        }
        // Search stays up so the next hit is one key away. File-name
        // lookup is a jump, and closes.
        if !matches!(self.panel, Some(Panel::Search)) {
            self.panel = None;
        }
        // A pending jump only belongs to the file it was queued for.
        if self
            .goto
            .as_ref()
            .is_some_and(|(target, _)| target != &path)
        {
            self.goto = None;
        }
        self.open_request += 1;
        // Opening is also what gives a file its tab, so a path reached from
        // the tree, quick open or a search hit all land in the same place.
        if !self.project.tabs.contains(&path) {
            self.project.tabs.push(path.clone());
        }
        if self
            .project
            .image
            .as_ref()
            .is_some_and(|(image_path, _)| image_path == &path)
        {
            self.project.active = Some(path);
            self.loading = false;
            self.tree_focus.focus(window, cx);
            self.update_title(window);
            self.follow_diff(window, cx);
            cx.notify();
            // Two ways into this function open nothing, and so never reach the
            // completion below: whatever is left of a session being put back
            // has to be started from here instead.
            self.next_restore(window, cx);
            return;
        }
        if let Some(doc) = self.project.documents.get(&path) {
            let editor = doc.editor.clone();
            self.project.active = Some(path.clone());
            self.loading = false;
            editor.focus_handle(cx).focus(window, cx);
            self.apply_goto(&path, window, cx);
            self.apply_recovered(&path, window, cx);
            self.update_title(window);
            self.follow_diff(window, cx);
            cx.notify();
            self.next_restore(window, cx);
            return;
        }
        let Some(workspace) = self.project.workspace.clone() else {
            return;
        };
        let generation = self.generation;
        let request = self.open_request;
        self.loading = true;
        cx.notify();
        let configured = editorconfig::Indent {
            columns: self.settings.tab_size,
            hard_tabs: self.settings.hard_tabs,
        };
        let task = cx.background_executor().spawn(async move {
            let path = workspace.resolve(&path)?;
            let content = preview::read(&path)?;
            // A file's own `.editorconfig` has the last word on its
            // indentation, and reading one is a handful of small files up the
            // directory tree — which is work for this side of the executor.
            let indent = match &content {
                Content::Text(_) => editorconfig::Rules::for_path(&path).indent(configured),
                Content::Image(_) => configured,
            };
            Ok::<_, std::io::Error>((path, content, indent))
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.generation != generation || this.open_request != request {
                    return;
                }
                this.loading = false;
                match result {
                    Ok((path, Content::Image(image), _)) => {
                        this.project.image = Some((path.clone(), image));
                        this.project.active = Some(path);
                        this.message = None;
                        this.tree_focus.focus(window, cx);
                        this.update_title(window);
                        this.follow_diff(window, cx);
                    }
                    Ok((path, Content::Text(text), indent)) => {
                        let large = text.len() > buffer::HIGHLIGHT_LIMIT;
                        let language = if large {
                            // "text" is gpui-component's grammardless language.
                            "text"
                        } else {
                            buffer::language(&path)
                        };
                        let saved: SharedString = text.into();
                        let tabs = TabSize {
                            tab_size: indent.columns,
                            hard_tabs: indent.hard_tabs,
                        };
                        let editor = cx.new(|cx| {
                            EditorState::new(language, window, cx)
                                .default_value(saved.clone())
                                .line_number(true)
                                .folding(true)
                                .tab_size(TabSize {
                                    tab_size: tabs.tab_size,
                                    hard_tabs: tabs.hard_tabs,
                                })
                        });
                        editor.update(cx, |state, cx| {
                            state.prepare(window, cx);
                            state
                                .base_state()
                                .update(cx, |base, cx| {
                                    base.set_soft_wrap(this.settings.soft_wrap, window, cx)
                                });
                        });
                        let project_id = this.project.id;
                        let subscription =
                            Self::subscribe_document(&editor, project_id, window, cx);
                        editor.focus_handle(cx).focus(window, cx);
                        this.project.documents.insert(
                            path.clone(),
                            Document {
                                editor,
                                saved,
                                dirty: false,
                                large,
                                _subscription: subscription,
                            },
                        );
                        this.apply_goto(&path, window, cx);
                        this.project.active = Some(path.clone());
                        this.message = None;
                        this.apply_recovered(&path, window, cx);
                        this.update_title(window);
                        this.follow_diff(window, cx);
                        // The blame belongs to the file that was open, and it
                        // is not this one.
                        if this.blame.is_some() {
                            this.read_blame(window, cx);
                        }
                    }
                    Err(e) => this.error(e.to_string(), cx),
                }
                cx.notify();
                this.next_restore(window, cx);
            });
        })
        .detach();
    }

    /// Give a recovered buffer its edits back, if this is the file they were
    /// waiting for. Answers whether it did.
    ///
    /// Both ways into `open_file` call this: a file that is already open has no
    /// read to wait for, and a file that is not has one — and the edits belong
    /// to the buffer either way. The text stays unsaved, with what is on disk
    /// still the text it was read from, so saving it goes through the same
    /// conflict check as any other edit.
    fn apply_recovered(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((waiting, text)) = self.restoring.take() else {
            return false;
        };
        if waiting != path {
            // Something else was opened in between. The edits go back where
            // they were rather than into the wrong file.
            self.restoring = Some((waiting, text));
            return false;
        }
        let Some(document) = self.project.documents.get(path) else {
            return false;
        };
        let editor = document.editor.clone();
        editor.update(cx, |editor, cx| editor.set_value(text, window, cx));
        if let Some(document) = self.project.documents.get_mut(path) {
            document.dirty = true;
        }
        self.recovered += 1;
        true
    }

    fn update_title(&self, window: &mut Window) {
        let project = self
            .project
            .workspace
            .as_ref()
            .map(|w| name(&w.root))
            .unwrap_or("Folio".into());
        let title = if let Some(path) = &self.project.active {
            let dirty = self.project.documents.get(path).is_some_and(|d| d.dirty);
            format!(
                "{}{} / {}",
                if dirty { "* " } else { "" },
                project,
                path.strip_prefix(self.project.workspace.as_ref().unwrap().root.as_path())
                    .unwrap_or(path)
                    .to_string_lossy()
            )
        } else {
            project
        };
        window.set_window_title(&title);
    }

    fn save_documents(&mut self, next: Option<Next>, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || self.loading || self.prompting {
            return;
        }
        let all_projects = matches!(next, Some(Next::Quit));
        // Closing a tab saves only that file, not every buffer that happens to
        // be dirty alongside it.
        let only = match &next {
            Some(Next::CloseTabs(paths)) => Some(paths.clone()),
            _ => None,
        };
        let saves = std::iter::once(&self.project)
            .chain(self.parked.iter().filter(|_| all_projects))
            .flat_map(|project| {
                project
                    .documents
                    .iter()
                    .filter(|(path, doc)| {
                        doc.dirty
                            && only
                                .as_ref()
                                .is_none_or(|only| only.iter().any(|kept| kept == *path))
                            && (next.is_some() || project.active.as_ref() == Some(path))
                    })
                    .map(|(path, doc)| {
                        (
                            project.id,
                            project.workspace.clone(),
                            path.clone(),
                            doc.editor.read(cx).value(),
                            doc.saved.clone(),
                        )
                    })
            })
            .collect::<Vec<_>>();
        if saves.is_empty() {
            if let Some(next) = next {
                self.perform(next, window, cx);
            }
            return;
        }
        self.saving = true;
        cx.notify();
        let task = cx.background_executor().spawn(async move {
            saves
                .into_iter()
                .map(|(project_id, workspace, path, text, expected)| {
                    let result = workspace
                        .as_ref()
                        .ok_or_else(|| std::io::Error::other("Project is closed"))
                        .and_then(|ws| ws.resolve(&path))
                        .and_then(|path| buffer::save(&path, &text, &expected));
                    (project_id, path, text, result)
                })
                .collect::<Vec<_>>()
        });
        cx.spawn_in(window, async move |this, cx| {
            let results = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.saving = false;
                let mut errors = vec![];
                for (project_id, path, text, result) in results {
                    match result {
                        Ok(()) => {
                            this.quiet_write(&path);
                            if let Some(doc) = this
                                .project_mut(project_id)
                                .and_then(|p| p.documents.get_mut(&path))
                            {
                                doc.saved = text;
                                doc.dirty = doc.editor.read(cx).value() != doc.saved;
                            }
                            // The blame was read against the buffer as it was;
                            // saving is what makes the two agree again.
                            if this.blame.is_some() {
                                this.read_blame(window, cx);
                            }
                        }
                        Err(e) => errors.push(format!("{}：{e}", name(&path))),
                    }
                }
                this.update_title(window);
                if errors.is_empty() {
                    if let Some(next) = next {
                        this.request(next, window, cx);
                    } else {
                        this.toast("Saved", cx);
                        this.refresh_git(cx);
                    }
                } else {
                    this.error(errors.join("\n"), cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn remove_recent(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.refresh_recent(RecentAction::Remove(path), cx);
    }

    fn refresh_recent(&mut self, action: RecentAction, cx: &mut Context<Self>) {
        let config = self.recent_file.clone();
        // ponytail: one window's JSON operations run in order; use a file lock if multiple instances are supported.
        let previous = self.recent_task.take();
        let executor = cx.background_executor().clone();
        self.recent_task = Some(cx.spawn(async move |this, cx| {
            if let Some(previous) = previous {
                previous.await;
            }
            let result = executor
                .spawn(async move {
                    match action {
                        RecentAction::Load => recent::load(&config),
                        RecentAction::Open(path) => recent::record(&path, &config),
                        RecentAction::Remove(path) => recent::remove(&path, &config),
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(items) => this.recent = items,
                    Err(e) => this.error(format!("Could not update recent projects: {e}"), cx),
                }
                cx.notify();
            });
        }));
    }

    /// Queue up what was open the last time, and start putting it back.
    ///
    /// The session is read here rather than in a task — one small file, the way
    /// the settings are read — and anything in it that is no longer on disk was
    /// dropped as it was read.
    fn start_restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let session = session::Session::load(&self.session_file);
        let unsaved = session::load_unsaved(&self.unsaved_file);

        for project in &session.projects {
            self.restore
                .push_back(Restore::Project(project.root.clone()));
            self.restore
                .extend(project.tabs.iter().cloned().map(Restore::File));
            // Recovered buffers belong to this project, and are opened while it
            // is the one being shown: a file of a parked project has nowhere to
            // appear, and would be read into whichever project happened to be
            // current. Before the file the project was showing, so that
            // recovering one cannot change which file is on screen.
            for (path, text) in &unsaved {
                if path.starts_with(&project.root) {
                    self.restore
                        .push_back(Restore::Unsaved(path.clone(), text.clone()));
                }
            }
            if let Some(active) = &project.active {
                self.restore.push_back(Restore::Activate(active.clone()));
            }
        }

        if !self.restore.is_empty() {
            self.next_restore(window, cx);
        }
    }

    /// Work through the restore queue, one step at a time.
    ///
    /// Each step starts the next when it lands: opening a project and opening a
    /// file are both asynchronous, and either starting while the other is in
    /// flight would have the one in flight thrown away.
    fn next_restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(step) = self.restore.pop_front() else {
            match self.recovered {
                0 => {}
                1 => self.toast("Recovered an unsaved file", cx),
                count => self.toast(&format!("Recovered {count} unsaved files"), cx),
            }
            self.recovered = 0;
            return;
        };
        match step {
            Restore::Project(root) => self.open_project(root, window, cx),
            Restore::File(path) => self.open_file(path, window, cx),
            Restore::Activate(path) => {
                // Already open, so this is not an open at all: it is the focus
                // and the strip's idea of which file is being read, both of
                // which the last file opened would otherwise have taken.
                if let Some(document) = self.project.documents.get(&path) {
                    document.editor.read(cx).focus_handle(cx).focus(window, cx);
                    self.project.active = Some(path.clone());
                    self.apply_goto(&path, window, cx);
                    self.update_title(window);
                    cx.notify();
                }
                self.next_restore(window, cx);
            }
            Restore::Unsaved(path, text) => {
                self.restoring = Some((path.clone(), text));
                self.open_file(path, window, cx);
            }
        }
    }

    /// The projects that are open, in the order they were opened, with the
    /// files each one has showing.
    fn collect_session(&self) -> session::Session {
        let mut projects = Vec::new();
        for root in &self.project_order {
            let Some(project) = self.project_for(root) else {
                continue;
            };
            let Some(workspace) = project.workspace.as_ref() else {
                continue;
            };
            projects.push(session::SessionProject {
                root: workspace.root.clone(),
                tabs: project.tabs.clone(),
                active: project.active.clone(),
            });
        }
        session::Session { projects }
    }

    /// The project with this root, whichever list it is in.
    fn project_for(&self, root: &Path) -> Option<&Project> {
        std::iter::once(&self.project)
            .chain(self.parked.iter())
            .find(|project| {
                project
                    .workspace
                    .as_ref()
                    .is_some_and(|workspace| workspace.root == root)
            })
    }

    /// Every buffer with edits that are not on disk, across every project.
    fn collect_unsaved(&self, cx: &App) -> session::UnsavedBuffers {
        std::iter::once(&self.project)
            .chain(self.parked.iter())
            .flat_map(|project| project.documents.iter())
            .filter(|(_, document)| document.dirty)
            .map(|(path, document)| (path.clone(), document.editor.read(cx).value().to_string()))
            .collect()
    }

    /// What has changed since the last write, if anything.
    fn pending_writes(&self, cx: &App) -> Writes {
        let session = self.collect_session();
        let unsaved = self.collect_unsaved(cx);
        Writes {
            session_file: self.session_file.clone(),
            unsaved_file: self.unsaved_file.clone(),
            session: (session != self.written_session).then_some(session),
            unsaved: (unsaved != self.written_unsaved).then_some(unsaved),
        }
    }

    /// Write down what is open and what is unsaved, while the app is running.
    ///
    /// On a timer rather than from every place that changes something: what
    /// this is for is the case where there is no clean exit to write at.
    fn watch_unsaved(&mut self, cx: &mut Context<Self>) {
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(UNSAVED_EVERY).await;
                let Ok(writes) = this.update(cx, |this, cx| this.pending_writes(cx)) else {
                    // The window is gone, and with it anything worth writing.
                    break;
                };
                if writes.is_empty() {
                    continue;
                }
                let written = executor.spawn(async move { writes.write() }).await;
                // Remembered only once the write has landed, so a failure is
                // retried on the next tick rather than assumed done.
                let _ = this.update(cx, |this, _| {
                    if let Some(session) = written.session {
                        this.written_session = session;
                    }
                    if let Some(unsaved) = written.unsaved {
                        this.written_unsaved = unsaved;
                    }
                });
            }
        })
        .detach();
    }

    fn sync_disk_watch(&mut self) {
        self.disk_watch.sync(&self.project_order);
    }

    /// Drain kernel events and apply them to the tree, git dots, and open
    /// buffers. Started once with the session so a test window that never
    /// calls `start_session` does not watch the machine.
    fn watch_disk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_disk_watch();
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(watch::DEBOUNCE).await;
                let Ok(paths) = this.update(cx, |this, _| this.disk_watch.take()) else {
                    break;
                };
                if paths.is_empty() {
                    continue;
                }
                let _ = this.update_in(cx, |this, window, cx| {
                    this.apply_watch_paths(paths, window, cx);
                });
            }
        })
        .detach();
    }

    fn quiet_write(&mut self, path: &Path) {
        self.quiet_writes.insert(
            path.to_path_buf(),
            Instant::now() + watch::QUIET_AFTER_WRITE,
        );
    }

    fn apply_watch_paths(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let now = Instant::now();
        self.quiet_writes.retain(|_, until| now < *until);
        let ignored = self.settings.ignored.clone();
        let cached = std::iter::once(&self.project)
            .chain(self.parked.iter())
            .flat_map(|project| project.directories.keys().cloned())
            .collect::<HashSet<_>>();
        let open = std::iter::once(&self.project)
            .chain(self.parked.iter())
            .flat_map(|project| project.documents.keys().cloned())
            .collect::<HashSet<_>>();
        let paths = paths
            .into_iter()
            .map(|path| path.canonicalize().unwrap_or(path))
            .collect::<Vec<_>>();
        let batch = watch::batch(paths, &ignored, &self.quiet_writes, now, &cached, &open);
        let git_ids = if batch.git {
            std::iter::once(&self.project)
                .chain(self.parked.iter())
                .filter(|project| project.workspace.is_some())
                .map(|project| project.id)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let needs_index = !batch.files.is_empty() || !batch.directories.is_empty();
        for dir in batch.directories {
            self.reload_directory(dir, cx);
        }
        if needs_index {
            for project_id in std::iter::once(&self.project)
                .chain(self.parked.iter())
                .map(|project| project.id)
                .collect::<Vec<_>>()
            {
                self.reindex_project(project_id, cx);
            }
        }
        for project_id in git_ids {
            self.refresh_git_project(project_id, cx);
        }
        if batch.files.is_empty() {
            cx.notify();
            return;
        }
        let files = batch.files;
        let task = cx.background_executor().spawn(async move {
            files
                .into_iter()
                .map(|path| {
                    let result = buffer::read(&path);
                    (path, result)
                })
                .collect::<Vec<_>>()
        });
        cx.spawn_in(window, async move |this, cx| {
            let results = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.apply_disk_reads(results, window, cx);
            });
        })
        .detach();
    }

    fn apply_disk_reads(
        &mut self,
        results: Vec<(PathBuf, io::Result<String>)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut reloaded = 0;
        let mut removed = Vec::new();
        for (path, result) in results {
            match result {
                Ok(text) => {
                    if self.apply_disk_text(&path, text, false, window, cx) {
                        reloaded += 1;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    if self.mark_disk_removed(&path) {
                        removed.push(name(&path));
                    }
                }
                Err(_) => {}
            }
        }
        if reloaded == 1 {
            self.toast("Reloaded from disk", cx);
        } else if reloaded > 1 {
            self.toast(&format!("Reloaded {reloaded} files from disk"), cx);
        }
        for name in removed {
            self.toast(&format!("{name} was removed on disk"), cx);
        }
        if self.blame.is_some() {
            self.read_blame(window, cx);
        }
        self.update_title(window);
        cx.notify();
    }

    /// Apply a disk snapshot to an open buffer. Forced reloads come from the
    /// banner and overwrite local edits; the watch path only overwrites when
    /// the buffer is clean. Returns whether a clean buffer was replaced.
    fn apply_disk_text(
        &mut self,
        path: &Path,
        text: String,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(project_id) = self.project_id_for_document(path) else {
            return false;
        };
        let Some(document) = self
            .project_mut(project_id)
            .and_then(|project| project.documents.get(path))
        else {
            return false;
        };
        let editor = document.editor.clone();
        let dirty = document.dirty;
        let saved = document.saved.to_string();
        if text == saved {
            if let Some(project) = self.project_mut(project_id) {
                project.disk_notice.remove(path);
            }
            return false;
        }
        if dirty && !force {
            if let Some(project) = self.project_mut(project_id) {
                project
                    .disk_notice
                    .insert(path.to_path_buf(), DiskNotice::Changed);
            }
            return false;
        }
        if let Some(document) = self
            .project_mut(project_id)
            .and_then(|project| project.documents.get_mut(path))
        {
            document.saved = text.clone().into();
        }
        editor.update(cx, |editor, cx| editor.set_value(text, window, cx));
        if let Some(project) = self.project_mut(project_id) {
            if let Some(document) = project.documents.get_mut(path) {
                document.dirty = document.editor.read(cx).value() != document.saved;
            }
            project.disk_notice.remove(path);
        }
        true
    }

    fn mark_disk_removed(&mut self, path: &Path) -> bool {
        let Some(project_id) = self.project_id_for_document(path) else {
            return false;
        };
        let Some(project) = self.project_mut(project_id) else {
            return false;
        };
        let Some(document) = project.documents.get(path) else {
            return false;
        };
        if document.dirty {
            project
                .disk_notice
                .insert(path.to_path_buf(), DiskNotice::Removed);
            false
        } else {
            true
        }
    }

    fn project_id_for_document(&self, path: &Path) -> Option<u64> {
        std::iter::once(&self.project)
            .chain(self.parked.iter())
            .find(|project| project.documents.contains_key(path))
            .map(|project| project.id)
    }

    fn reload_from_disk(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let task = cx
            .background_executor()
            .spawn(async move { (path.clone(), buffer::read(&path)) });
        cx.spawn_in(window, async move |this, cx| {
            let (path, result) = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(text) => {
                        this.apply_disk_text(&path, text, true, window, cx);
                        this.toast("Reloaded from disk", cx);
                    }
                    Err(error) => this.error(error.to_string(), cx),
                }
                this.update_title(window);
                cx.notify();
            });
        })
        .detach();
    }

    /// The last write, on the way out: the session as it stands, and no
    /// unsaved file at all — whatever was not saved was either saved or
    /// deliberately let go of at the prompt.
    fn finish_session(&mut self) {
        let _ = session::Session::save(&self.collect_session(), &self.session_file);
        let _ = session::clear_unsaved(&self.unsaved_file);
    }

    fn filter(&mut self, cx: &App) {
        let query = self.query.read(cx).value();
        // ponytail: a bounded 100-result linear filename search; use a scored index if huge repositories need it.
        self.matches = self
            .project
            .files
            .iter()
            .filter(|p| tree::fuzzy_match(query.trim(), &name(p)))
            .take(100)
            .cloned()
            .collect();
        self.match_selected = 0;
        self.quick_scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    fn show_quick_open(&mut self, line: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.project.workspace.is_none() || self.project_loading || self.prompting {
            return;
        }
        self.panel = Some(Panel::Files);
        self.query.update(cx, |query, cx| {
            query.set_value(if line { ":" } else { "" }, window, cx);
            query.focus(window, cx);
        });
        self.filter(cx);
        cx.notify();
    }

    /// Open the project-search panel, optionally revealing the replace field.
    fn show_search(&mut self, replace: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.project.workspace.is_none() || self.project_loading || self.prompting {
            return;
        }
        self.panel = Some(Panel::Search);
        // Only "Find in Folder" narrows the scan; reopening the panel starts
        // from the whole project again.
        self.search.scope = None;
        if replace {
            self.search.show_replace = true;
        }
        self.search_query
            .update(cx, |query, cx| query.focus(window, cx));
        if !self.search.query.is_empty() {
            self.start_search(cx);
        }
        cx.notify();
    }

    fn select_panel(&mut self, panel: Panel, window: &mut Window, cx: &mut Context<Self>) {
        if self.panel == Some(panel) {
            return;
        }
        self.panel = Some(panel);
        match panel {
            Panel::Files => {
                self.query.update(cx, |query, cx| query.focus(window, cx));
                self.filter(cx);
            }
            Panel::Search => {
                self.search.scope = None;
                self.search_query
                    .update(cx, |query, cx| query.focus(window, cx));
                self.start_search(cx);
            }
        }
        cx.notify();
    }

    /// A left or right while the panel is open. It switches the mode tab
    /// — Files sits to the left of Contents, and the arrows follow that
    /// order — but only when the focused query's caret is already against
    /// the edge the key points at. Anywhere inside the text, or with the
    /// focus somewhere the panel cannot see, the key keeps its ordinary
    /// meaning. Returns whether the tab switched.
    fn panel_arrow(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let left = key == "left";
        let neighbor = match (self.panel, left) {
            (Some(Panel::Search), true) => Some(Panel::Files),
            (Some(Panel::Files), false) => Some(Panel::Search),
            _ => None,
        };
        let Some(neighbor) = neighbor else {
            return false;
        };
        if !self.panel_caret_at_edge(left, window, cx) {
            return false;
        }
        self.select_panel(neighbor, window, cx);
        true
    }

    /// Whether the caret of the query the panel's focus sits in is against
    /// the edge `left` names — offset 0, or the end of the text — with
    /// nothing selected. Only a panel input counts, so a caret in the
    /// editor behind the panel never spends an arrow on the tabs.
    fn panel_caret_at_edge(&self, left: bool, window: &Window, cx: &App) -> bool {
        let at_edge = |input: &Entity<InputState>| {
            if !input.focus_handle(cx).is_focused(window) {
                return false;
            }
            let base = input.read(cx).base_state().read(cx);
            base.selected_value().is_empty()
                && base.cursor() == if left { 0 } else { base.value().len() }
        };
        match self.panel {
            Some(Panel::Files) => at_edge(&self.query),
            Some(Panel::Search) => at_edge(&self.search_query) || at_edge(&self.replace_query),
            None => false,
        }
    }

    fn close_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.panel = None;
        if let Some(doc) = self
            .project
            .active
            .as_ref()
            .and_then(|p| self.project.documents.get(p))
        {
            doc.editor.focus_handle(cx).focus(window, cx);
        } else {
            self.tree_focus.focus(window, cx);
        }
        cx.notify();
    }

    /// The active project's terminal tabs, creating its first the first
    /// time the drawer asks. A terminal lives until its tab closes or its
    /// project does; hiding the drawer keeps the children running.
    fn ensure_terminal(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.project.workspace.as_ref().map(|w| w.root.clone()) else {
            return;
        };
        self.terminals
            .entry(root.clone())
            .or_insert_with(|| TerminalTabs {
                views: vec![cx.new(|cx| TerminalView::new(&root, 80, 24, cx))],
                active: 0,
            });
    }

    /// A fresh terminal tab, focused and showing.
    fn new_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.project.workspace.as_ref().map(|w| w.root.clone()) else {
            return;
        };
        self.terminal_open = true;
        let view = cx.new(|cx| TerminalView::new(&root, 80, 24, cx));
        view.update(cx, |view, cx| view.focus_handle().focus(window, cx));
        let tabs = self.terminals.entry(root).or_insert(TerminalTabs {
            views: vec![],
            active: 0,
        });
        tabs.views.push(view);
        tabs.active = tabs.views.len() - 1;
        cx.notify();
    }

    fn active_terminal(&self) -> Option<&Entity<TerminalView>> {
        let root = self.project.workspace.as_ref()?.root.clone();
        let tabs = self.terminals.get(&root)?;
        tabs.views.get(tabs.active)
    }

    /// Close one terminal tab. The drawer goes away with the last one, and
    /// the tab that slid into the closed one's place takes the keyboard.
    fn close_terminal_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.project.workspace.as_ref().map(|w| w.root.clone()) else {
            return;
        };
        let empty = {
            let Some(tabs) = self.terminals.get_mut(&root) else {
                return;
            };
            if index >= tabs.views.len() {
                return;
            }
            tabs.views.remove(index);
            if tabs.views.is_empty() {
                true
            } else {
                if index < tabs.active {
                    tabs.active -= 1;
                }
                tabs.active = tabs.active.min(tabs.views.len() - 1);
                if let Some(view) = tabs.views.get(tabs.active) {
                    view.update(cx, |view, cx| view.focus_handle().focus(window, cx));
                }
                false
            }
        };
        if empty {
            self.terminals.remove(&root);
            self.terminal_open = false;
            if let Some(doc) = self
                .project
                .active
                .as_ref()
                .and_then(|p| self.project.documents.get(p))
            {
                doc.editor.focus_handle(cx).focus(window, cx);
            } else {
                self.tree_focus.focus(window, cx);
            }
        }
        cx.notify();
    }

    /// `⌘J`: show or hide the terminal dock.
    fn toggle_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.project.workspace.is_none() || self.project_loading || self.prompting {
            return;
        }
        if self.terminal_open {
            self.terminal_open = false;
            if let Some(doc) = self
                .project
                .active
                .as_ref()
                .and_then(|p| self.project.documents.get(p))
            {
                doc.editor.focus_handle(cx).focus(window, cx);
            } else {
                self.tree_focus.focus(window, cx);
            }
        } else {
            self.terminal_open = true;
            self.ensure_terminal(cx);
            if let Some(view) = self.active_terminal() {
                view.update(cx, |view, cx| view.focus_handle().focus(window, cx));
            }
        }
        cx.notify();
    }

    /// `⌘B` and the first rail icon: show or hide the file tree.
    fn toggle_file_tree(&mut self, cx: &mut Context<Self>) {
        self.settings.sidebar = !self.sidebar;
        self.apply_settings(cx);
    }

    /// The title-bar button: show or hide the icon rail. The file tree
    /// stays as it is.
    fn toggle_activity_bar(&mut self, cx: &mut Context<Self>) {
        self.settings.activity_bar = !self.activity_bar;
        self.apply_settings(cx);
    }

    /// How far the rounded card starts from the window's left edge: the
    /// rail, when it is showing, plus the gap that keeps the card off it.
    fn workspace_left_inset(&self) -> f32 {
        if self.activity_bar {
            ACTIVITY_BAR_WIDTH + WORKSPACE_RAIL_GAP
        } else {
            WORKSPACE_INSET
        }
    }

    /// The drawer: a drag handle on top, a tab strip with one entry per
    /// terminal — each closing itself, the last closing the drawer — a
    /// `+` for another one, and the active terminal filling the rest. The
    /// grid sizes itself to the pixels the body gives it.
    fn render_terminal_dock(&self, _: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some((root, active)) = self
            .project
            .workspace
            .as_ref()
            .map(|w| w.root.clone())
            .and_then(|root| {
                self.terminals
                    .get(&root)
                    .map(|tabs| (root, tabs.active.min(tabs.views.len().saturating_sub(1))))
            })
        else {
            return div().into_any_element();
        };
        let entity = self.terminals[&root].views[active].clone();
        let tab_count = self.terminals[&root].views.len();
        let labels: Vec<String> = self.terminals[&root]
            .views
            .iter()
            .map(|view| {
                let view = view.read(cx);
                match (view.title().map(str::to_string), view.exit()) {
                    (Some(title), _) => title,
                    (None, Some(Some(code))) => format!("Terminal · exited ({code})"),
                    (None, Some(None)) => "Terminal · exited".to_string(),
                    (None, None) => "Terminal".to_string(),
                }
            })
            .collect();
        div()
            .flex_shrink_0()
            .h(px(self.terminal_height))
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(cx.theme().border)
            .rounded_br(px(WORKSPACE_RADIUS))
            .when(!self.sidebar, |el| el.rounded_bl(px(WORKSPACE_RADIUS)))
            // The top edge drags the height, the way the sidebar's edge
            // drags the width.
            .child(
                div()
                    .id("terminal-resize")
                    .h(px(4.))
                    .cursor_row_resize()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, _| this.resizing_terminal = true),
                    ),
            )
            // The tab strip reads like the one above the code: a tab per
            // terminal, each with its own close, and a `+` for another.
            .child(
                div()
                    .h(px(26.))
                    .flex()
                    .items_center()
                    .pr_2()
                    .children((0..tab_count).map(|index| {
                        self.render_terminal_tab(index, &labels[index], index == active, cx)
                    }))
                    .child(div().flex_1())
                    .child(Self::icon_button(
                        "terminal-new",
                        IconName::Plus,
                        "New Terminal",
                        |this, window, cx| this.new_terminal(window, cx),
                        cx,
                    )),
            )
            .child(div().flex_1().min_h_0().overflow_hidden().child(entity))
            .into_any_element()
    }

    /// One terminal tab: the terminal's mark and title, and the close that
    /// takes the tab — and, if it was the last, the drawer — with it.
    fn render_terminal_tab(
        &self,
        index: usize,
        label: &str,
        active: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(("terminal-tab", index))
            .role(Role::Button)
            .aria_label(label.to_string())
            .aria_selected(active)
            .h_full()
            .min_w_0()
            .max_w(px(220.))
            .px_2()
            .flex()
            .items_center()
            .gap_1p5()
            .flex_shrink_0()
            .border_r_1()
            .border_color(cx.theme().border)
            .cursor_pointer()
            .text_size(ui(11.))
            .text_color(if active {
                cx.theme().foreground
            } else {
                cx.theme().muted_foreground
            })
            .when(active, |el| el.bg(cx.theme().list_active))
            .hover(|el| el.bg(cx.theme().list_hover))
            .on_click(
                cx.listener(move |this, _, window, cx| {
                    this.activate_terminal_tab(index, window, cx)
                }),
            )
            .child(
                Icon::new(FolioIcon::SquareTerminal)
                    .size_4()
                    .text_color(if active {
                        cx.theme().accent_foreground
                    } else {
                        cx.theme().muted_foreground
                    }),
            )
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(label.to_string()),
            )
            .child(
                div()
                    .id(("terminal-tab-close", index))
                    .role(Role::Button)
                    .aria_label("Close Terminal")
                    .size(px(14.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_color(cx.theme().muted_foreground)
                    .hover(|el| el.text_color(cx.theme().foreground))
                    // Without this the tab underneath would take the same
                    // click and merely activate itself.
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.close_terminal_tab(index, window, cx)
                    }))
                    .child(Icon::new(IconName::Close).xsmall()),
            )
            .into_any_element()
    }

    fn activate_terminal_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.project.workspace.as_ref().map(|w| w.root.clone()) else {
            return;
        };
        let Some(tabs) = self.terminals.get_mut(&root) else {
            return;
        };
        if index >= tabs.views.len() {
            return;
        }
        tabs.active = index;
        if let Some(view) = tabs.views.get(index) {
            view.update(cx, |view, cx| view.focus_handle().focus(window, cx));
        }
        cx.notify();
    }

    /// Flip one of the `Aa` / `ab` / `.*` toggles and rescan.
    fn toggle_search_option(
        &mut self,
        toggle: impl Fn(&mut search::Options),
        cx: &mut Context<Self>,
    ) {
        toggle(&mut self.search.options);
        self.start_search(cx);
        cx.notify();
    }

    /// Compile the current query and scan the project for it.
    ///
    /// The scan runs on the background executor after a short debounce.
    /// `search_request` identifies the newest scan and lets an older one give up
    /// instead of racing it for the CPU.
    fn start_search(&mut self, cx: &mut Context<Self>) {
        let query = self.search_query.read(cx).value().to_string();
        self.search.query = query.clone();
        self.search.selected = 0;
        self.search.error = None;
        self.search.truncated = false;
        self.search.results.clear();
        self.search.rows.clear();
        self.search.scroll.scroll_to_item(0, ScrollStrategy::Top);
        let request = self.search_request.fetch_add(1, Ordering::Relaxed) + 1;
        if query.trim().is_empty() {
            self.search.running = false;
            cx.notify();
            return;
        }
        let matcher = match search::Matcher::new(&query, self.search.options) {
            Ok(matcher) => matcher,
            Err(error) => {
                self.search.running = false;
                self.search.error = Some(error);
                cx.notify();
                return;
            }
        };
        self.search.running = true;
        cx.notify();
        let cancel = self.search_request.clone();
        let cancel_inner = cancel.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SEARCH_DEBOUNCE).await;
            if cancel.load(Ordering::Relaxed) != request {
                return;
            }
            // Snapshot the file list and the open buffers only once the query
            // has settled, so typing does not copy them on every keystroke.
            let prepared = this
                .update(cx, |this, cx| {
                    if this.search_request.load(Ordering::Relaxed) != request {
                        return None;
                    }
                    let mut files = this.project.files.clone();
                    if let Some(scope) = &this.search.scope {
                        files.retain(|path| path.starts_with(scope));
                    }
                    let open = this
                        .project
                        .documents
                        .iter()
                        .map(|(path, document)| {
                            (path.clone(), document.editor.read(cx).value().to_string())
                        })
                        .collect::<HashMap<PathBuf, String>>();
                    Some((files, open))
                })
                .ok()
                .flatten();
            let Some((files, open)) = prepared else {
                return;
            };
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    search::search_files(&files, &open, &matcher, || {
                        cancel_inner.load(Ordering::Relaxed) == request
                    })
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.search_request.load(Ordering::Relaxed) != request {
                    return;
                }
                this.search.running = false;
                this.search.truncated = outcome.truncated;
                this.search.results = outcome.files;
                this.search.rows = search_rows(&this.search.results);
                this.search.selected = this
                    .search
                    .rows
                    .iter()
                    .position(|row| matches!(row, SearchRow::Hit { .. }))
                    .unwrap_or(0);
                this.search.scroll.scroll_to_item(0, ScrollStrategy::Top);
                cx.notify();
            });
        })
        .detach();
    }

    fn open_selected_hit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(SearchRow::Hit { file, hit }) =
            self.search.rows.get(self.search.selected).copied()
        else {
            return;
        };
        let Some(file) = self.search.results.get(file) else {
            return;
        };
        let (path, hit) = (file.path.clone(), file.hits[hit].clone());
        self.goto = Some((path.clone(), Position::new(hit.line, hit.column)));
        self.open_file(path, window, cx);
    }

    /// Move the highlight to the next or previous match, skipping file headers.
    fn move_search_selection(&mut self, down: bool) {
        let hits = self
            .search
            .rows
            .iter()
            .enumerate()
            .filter(|(_, row)| matches!(row, SearchRow::Hit { .. }))
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let current = hits.iter().position(|&index| index == self.search.selected);
        let next = match (current, down) {
            (Some(current), true) => (current + 1).min(hits.len() - 1),
            (Some(current), false) => current.saturating_sub(1),
            (None, _) => 0,
        };
        let Some(&selected) = hits.get(next) else {
            return;
        };
        self.search.selected = selected;
        self.search
            .scroll
            .scroll_to_item(selected, ScrollStrategy::Nearest);
    }

    fn total_hits(&self) -> usize {
        self.search.results.iter().map(|file| file.hits.len()).sum()
    }

    /// Confirm, then replace every match the results list points at.
    fn replace_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving
            || self.prompting
            || self.project_loading
            || self.search.running
            || self.search.error.is_some()
            || self.search.results.is_empty()
        {
            return;
        }
        let files = self.search.results.len();
        let hits = self.total_hits();
        self.prompting = true;
        cx.notify();
        let answer = window.prompt(
            PromptLevel::Warning,
            "Replace All",
            Some(&format!(
                "Replace {hits} occurrences across {files} files. Files that are not open are written to disk immediately; this cannot be undone."
            )),
            &["Replace All", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = answer.await.unwrap_or(1);
            let _ = this.update_in(cx, |this, window, cx| {
                this.prompting = false;
                if answer == 0 {
                    this.apply_replace(window, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn apply_replace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let replacement = self.replace_query.read(cx).value().to_string();
        let Ok(matcher) = search::Matcher::new(&self.search.query, self.search.options) else {
            return;
        };
        let targets = self
            .search
            .results
            .iter()
            .map(|file| file.path.clone())
            .collect::<Vec<_>>();

        // Open buffers are edited in memory: the change stays undoable and
        // reaches the disk through the usual ⌘S flow.
        let mut edited = 0;
        let mut open_files = 0;
        for path in &targets {
            let Some(document) = self.project.documents.get(path) else {
                continue;
            };
            let text = document.editor.read(cx).value().to_string();
            let (updated, count) = matcher.replace(&text, &replacement);
            if count == 0 {
                continue;
            }
            let editor = document.editor.clone();
            editor.update(cx, |state, cx| {
                state
                    .base_state()
                    .clone()
                    .update(cx, |base, cx| base.replace_all(updated, window, cx));
            });
            edited += count;
            open_files += 1;
        }
        for path in &targets {
            if let Some(document) = self.project.documents.get_mut(path) {
                document.dirty = document.editor.read(cx).value() != document.saved;
            }
        }

        // Closed files are rewritten on disk, atomically and only while they
        // still hold what the search saw.
        let disk = targets
            .iter()
            .filter(|path| !self.project.documents.contains_key(*path))
            .cloned()
            .collect::<Vec<_>>();
        let workspace = self.project.workspace.clone();
        if !disk.is_empty() {
            self.saving = true;
            cx.notify();
        }
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut replaced = 0;
                    let mut files = 0;
                    let mut errors = Vec::new();
                    for path in disk {
                        let outcome = workspace
                            .as_ref()
                            .ok_or_else(|| io::Error::other("Project is closed"))
                            .and_then(|workspace| workspace.resolve(&path))
                            .and_then(|path| {
                                let original = buffer::read(&path)?;
                                let (updated, count) = matcher.replace(&original, &replacement);
                                if count == 0 {
                                    return Ok(0);
                                }
                                buffer::save(&path, &updated, &original)?;
                                Ok(count)
                            });
                        match outcome {
                            Ok(count) => {
                                replaced += count;
                                files += 1;
                            }
                            Err(error) => errors.push(format!("{}：{error}", name(&path))),
                        }
                    }
                    (files, replaced, errors)
                })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.saving = false;
                let (files, replaced, errors) = result;
                let replaced = replaced + edited;
                let files = files + open_files;
                if errors.is_empty() {
                    this.update_title(window);
                    this.refresh_git(cx);
                    this.toast(
                        &if open_files == 0 {
                            format!("Replaced {replaced} in {files} files")
                        } else {
                            format!(
                                "Replaced {replaced} in {files} files ({open_files} open files still to save)"
                            )
                        },
                        cx,
                    );
                } else {
                    this.error(errors.join("\n"), cx);
                }
                this.start_search(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn accept_match(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.query.read(cx).value();
        if let Some(line) = query.trim().strip_prefix(':') {
            if let Ok(line) = line.parse::<u32>()
                && line > 0
                && let Some(doc) = self
                    .project
                    .active
                    .as_ref()
                    .and_then(|p| self.project.documents.get(p))
            {
                doc.editor
                    .read(cx)
                    .base_state()
                    .clone()
                    .update(cx, |base, cx| {
                        base.set_cursor_position(Position::new(line - 1, 0), window, cx)
                    });
                self.panel = None;
                cx.notify();
                return;
            }
            self.error("Enter a line number, for example :123".into(), cx);
        } else if let Some(path) = self.matches.get(self.match_selected).cloned() {
            self.open_file(path, window, cx);
        }
    }

    fn error(&mut self, message: String, cx: &mut Context<Self>) {
        self.message = Some(message);
        cx.notify();
    }
    fn toast(&mut self, message: &str, cx: &mut Context<Self>) {
        self.message = Some(message.into());
        let message = message.to_string();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            let _ = this.update(cx, |this, cx| {
                if this.message.as_ref() == Some(&message) {
                    this.message = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub fn close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.request(Next::Quit, window, cx);
    }

    fn tree_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // While a row is being named the keyboard belongs to its text field.
        if self.editing.is_some() {
            return;
        }
        let Some(row) = self.project.rows.get(self.project.selected_row).cloned() else {
            return;
        };
        match event.keystroke.key.as_str() {
            "up" => self.project.selected_row = self.project.selected_row.saturating_sub(1),
            "down" => {
                self.project.selected_row =
                    (self.project.selected_row + 1).min(self.project.rows.len().saturating_sub(1))
            }
            "enter" => match row.entry.kind {
                EntryKind::Directory => self.toggle_directory(row.entry.path, cx),
                EntryKind::File => self.open_file(row.entry.path, window, cx),
            },
            "right"
                if row.entry.kind == EntryKind::Directory
                    && !self.project.expanded.contains(&row.entry.path) =>
            {
                self.toggle_directory(row.entry.path, cx)
            }
            "left" => {
                if self.project.expanded.contains(&row.entry.path) {
                    self.toggle_directory(row.entry.path, cx);
                } else if let Some(parent) = row.entry.path.parent()
                    && let Some(i) = self
                        .project
                        .rows
                        .iter()
                        .position(|r| r.entry.path == parent)
                {
                    self.project.selected_row = i;
                }
            }
            _ => return,
        }
        self.project
            .tree_scroll
            .scroll_to_item(self.project.selected_row, ScrollStrategy::Nearest);
        cx.stop_propagation();
        cx.notify();
    }

    fn icon_button(
        id: impl Into<ElementId>,
        // `impl Into<Icon>` rather than `IconName`, so the vendored icons in
        // `src/assets.rs` can be passed here too.
        icon: impl Into<Icon>,
        label: &'static str,
        activate: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &Context<Self>,
    ) -> AnyElement {
        let activate = std::rc::Rc::new(activate);
        let keyboard = activate.clone();
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(label)
            .focusable()
            .tab_index(0)
            .size(px(24.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_color(cx.theme().muted_foreground)
            .hover(|el| el.text_color(cx.theme().foreground))
            .focus_visible(|el| el.text_color(cx.theme().accent_foreground))
            .tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(label).build(window, cx)
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| activate(this, window, cx)))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    keyboard(this, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(Icon::new(icon).small())
            .into_any_element()
    }

    /// A rail icon: the same hit target as [`Self::icon_button`], painted
    /// in the accent when the surface it opens is showing.
    fn rail_icon(
        id: impl Into<ElementId>,
        icon: impl Into<Icon>,
        label: &'static str,
        active: bool,
        activate: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &Context<Self>,
    ) -> AnyElement {
        let activate = std::rc::Rc::new(activate);
        let keyboard = activate.clone();
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(label)
            .focusable()
            .tab_index(0)
            .size(px(24.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_color(if active {
                cx.theme().accent_foreground
            } else {
                cx.theme().muted_foreground
            })
            .hover(|el| el.text_color(cx.theme().foreground))
            .focus_visible(|el| el.text_color(cx.theme().accent_foreground))
            .tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(label).build(window, cx)
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| activate(this, window, cx)))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    keyboard(this, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(Icon::new(icon).small())
            .into_any_element()
    }

    /// The far-left rail: small icons, no names. The first one is the
    /// file tree; the rest open the surfaces Folio already has.
    fn render_activity_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("activity-bar")
            .w(px(ACTIVITY_BAR_WIDTH))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .pt_2()
            .pb_2()
            .gap_1()
            .child(Self::rail_icon(
                "rail-files",
                IconName::Folder,
                "File Tree",
                self.sidebar,
                |this, _, cx| this.toggle_file_tree(cx),
                cx,
            ))
            .child(Self::rail_icon(
                "rail-search",
                IconName::Search,
                "Search",
                self.panel == Some(Panel::Search),
                |this, window, cx| this.show_search(false, window, cx),
                cx,
            ))
            .child(Self::rail_icon(
                "rail-terminal",
                FolioIcon::SquareTerminal,
                "Terminal",
                self.terminal_open,
                |this, window, cx| this.toggle_terminal(window, cx),
                cx,
            ))
            .child(div().flex_1())
            .child(Self::rail_icon(
                "rail-settings",
                IconName::Settings,
                "Settings",
                false,
                |this, _, cx| this.show_settings(cx),
                cx,
            ))
            .into_any_element()
    }

    fn render_launcher(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .size_full()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(470.))
                    .flex()
                    .flex_col()
                    .gap(px(28.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_4()
                            .child(
                                Icon::new(IconName::BookOpen)
                                    .size(px(40.))
                                    .text_color(cx.theme().accent_foreground),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(div().text_size(ui(32.)).child("Folio"))
                                    .child(
                                        div()
                                            .text_size(ui(13.))
                                            .text_color(cx.theme().muted_foreground)
                                            .child("Read the code, change a few lines."),
                                    ),
                            ),
                    )
                    .child(
                        Button::new("open-project")
                            .text()
                            .icon(IconName::FolderOpen)
                            .label(if self.loading {
                                "Opening…"
                            } else {
                                "Open Project…"
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.request(Next::Picker, window, cx)
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .text_size(ui(11.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Recent Projects")
                                    .child("⌘ O"),
                            )
                            .when(self.recent.is_empty(), |el| {
                                el.child(
                                    div()
                                        .py_4()
                                        .text_size(ui(13.))
                                        .text_color(cx.theme().muted_foreground)
                                        .child("Start from a local folder."),
                                )
                            })
                            .children(self.recent.iter().enumerate().map(|(i, item)| {
                                let path = item.path.clone();
                                let remove = path.clone();
                                let keyboard_path = path.clone();
                                div()
                                    .id(("recent", i))
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .py_2()
                                    .border_b_1()
                                    .border_color(cx.theme().border)
                                    .child(
                                        div()
                                            .id(("recent-open", i))
                                            .role(Role::Button)
                                            .aria_label(format!("Open project {}", name(&path)))
                                            .focusable()
                                            .tab_index(0)
                                            .on_key_down(cx.listener(
                                                move |this, event: &KeyDownEvent, window, cx| {
                                                    if event.keystroke.key == "enter" {
                                                        this.request(
                                                            Next::Open(keyboard_path.clone()),
                                                            window,
                                                            cx,
                                                        );
                                                        cx.stop_propagation();
                                                    }
                                                },
                                            ))
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .cursor_default()
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                this.request(Next::Open(path.clone()), window, cx)
                                            }))
                                            .child(
                                                div()
                                                    .flex()
                                                    .justify_between()
                                                    .child(format!(
                                                        "{}{}",
                                                        name(&item.path),
                                                        if item.available {
                                                            ""
                                                        } else {
                                                            " · missing"
                                                        }
                                                    ))
                                                    .child(
                                                        div()
                                                            .text_size(ui(11.))
                                                            .text_color(cx.theme().muted_foreground)
                                                            .child(relative_time(item.last_opened)),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_size(ui(11.))
                                                    .text_color(cx.theme().muted_foreground)
                                                    .truncate()
                                                    .child(
                                                        item.path.to_string_lossy().into_owned(),
                                                    ),
                                            ),
                                    )
                                    .child(Self::icon_button(
                                        ("remove", i),
                                        IconName::Close,
                                        "Remove from recent projects",
                                        move |this, _, cx| this.remove_recent(remove.clone(), cx),
                                        cx,
                                    ))
                            })),
                    )
                    .child(
                        div()
                            .text_size(ui(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child("Or drop a folder here"),
                    ),
            )
            .into_any_element()
    }

    fn select_project(&mut self, root: &Path, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || self.prompting || self.project_loading {
            return;
        }
        if self
            .project
            .workspace
            .as_ref()
            .is_some_and(|w| w.root == root)
        {
            self.toggle_directory(root.to_path_buf(), cx);
        } else {
            self.switch_project(root, window, cx);
            self.project.expanded.insert(root.to_path_buf());
            self.rebuild_rows();
            cx.notify();
        }
    }

    fn render_project_header(&self, project: &Project, cx: &mut Context<Self>) -> AnyElement {
        let root = project.workspace.as_ref().unwrap().root.clone();
        let keyboard_root = root.clone();
        let menu_root = root.clone();
        let label = name(&root);
        let current = project.id == self.project.id;
        let expanded = current && project.expanded.contains(&root);
        div()
            .id(("project-root", project.id as usize))
            .role(Role::Button)
            .aria_label(label.clone())
            .aria_expanded(expanded)
            .focusable()
            .tab_index(0)
            .h(px(42.))
            .flex_shrink_0()
            .mx_1()
            .px_3()
            .rounded(cx.theme().radius)
            .flex()
            .items_center()
            .gap_2()
            .text_size(ui(12.))
            .text_color(if current {
                cx.theme().foreground
            } else {
                cx.theme().muted_foreground
            })
            .cursor_default()
            .hover(|el| {
                el.bg(cx.theme().list_hover)
                    .text_color(cx.theme().accent_foreground)
            })
            .on_click(
                cx.listener(move |this, _, window, cx| this.select_project(&root, window, cx)),
            )
            .on_drop(cx.listener({
                let onto = keyboard_root.clone();
                move |this, drag: &TreeDrag, window, cx| {
                    this.drop_tree_entry(drag.path.clone(), onto.clone(), window, cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    // The menu always acts on the project it opened over, so
                    // make that project current and open before offering it.
                    if this
                        .project
                        .workspace
                        .as_ref()
                        .is_none_or(|w| w.root != menu_root)
                    {
                        this.switch_project(&menu_root, window, cx);
                        this.project.expanded.insert(menu_root.clone());
                        this.rebuild_rows();
                    }
                    if !this.project.expanded.contains(&menu_root) {
                        this.toggle_directory(menu_root.clone(), cx);
                    }
                    this.open_menu(
                        MenuTarget::Tree {
                            path: menu_root.clone(),
                            root: true,
                        },
                        event.position,
                        window,
                        cx,
                    );
                    cx.stop_propagation();
                }),
            )
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                let current = this
                    .project
                    .workspace
                    .as_ref()
                    .is_some_and(|w| w.root == keyboard_root);
                let expanded = current && this.project.expanded.contains(&keyboard_root);
                match event.keystroke.key.as_str() {
                    "enter" | "space" => this.select_project(&keyboard_root, window, cx),
                    "left" if expanded => this.select_project(&keyboard_root, window, cx),
                    "right" if !expanded => this.select_project(&keyboard_root, window, cx),
                    "left" | "right" => {}
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .child(
                Icon::new(if expanded {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .xsmall(),
            )
            .child(
                Icon::new(if expanded {
                    IconName::FolderOpen
                } else {
                    IconName::Folder
                })
                .small(),
            )
            .child(div().flex_1().min_w_0().truncate().child(label))
            .when(project.documents.values().any(|d| d.dirty), |el| {
                el.child(Icon::new(IconName::Asterisk).xsmall())
            })
            .into_any_element()
    }

    fn render_tree(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("sidebar")
            .role(Role::Tree)
            .aria_label("Project")
            .track_focus(&self.tree_focus)
            .tab_index(0)
            .key_context("FolioTree")
            .w(px(self.sidebar_width))
            .h_full()
            .flex_shrink_0()
            .rounded_tl(px(WORKSPACE_RADIUS))
            .rounded_bl(px(WORKSPACE_RADIUS))
            .flex()
            .flex_col()
            .on_key_down(cx.listener(Self::tree_key))
            .children(self.project_order.iter().filter_map(|root| {
                let project = std::iter::once(&self.project)
                    .chain(self.parked.iter())
                    .find(|p| p.workspace.as_ref().is_some_and(|w| &w.root == root))?;
                let expanded = project.id == self.project.id && project.expanded.contains(root);
                Some(
                    div()
                        .flex()
                        .flex_col()
                        .min_h_0()
                        .when(expanded, |el| el.flex_1())
                        .when(!expanded, |el| el.flex_shrink_0())
                        .child(self.render_project_header(project, cx))
                        .when(expanded, |el| el.child(self.render_file_tree(cx))),
                )
            }))
            .child(
                div().p_2().flex_shrink_0().child(
                    Button::new("add-project")
                        .text()
                        .icon(IconName::Plus)
                        .label("Add Project")
                        .small()
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.request(Next::Picker, window, cx)
                        })),
                ),
            )
            .into_any_element()
    }

    fn render_file_tree(&self, cx: &mut Context<Self>) -> AnyElement {
        uniform_list(
            "tree",
            self.project.rows.len(),
            cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|i| {
                        if this.editing.as_ref().is_some_and(|edit| edit.row == i) {
                            return this.render_inline_edit(cx);
                        }
                        let row = this.project.rows[i].clone();
                        let path = row.entry.path.clone();
                        let drag_path = path.clone();
                        let drop_path = path.clone();
                        let menu_path = path.clone();
                        let selected = this.project.active.as_ref() == Some(&path)
                            || i == this.project.selected_row;
                        let status = this
                            .project
                            .workspace
                            .as_ref()
                            .and_then(|w| path.strip_prefix(&w.root).ok())
                            .and_then(|p| this.project.git_status.get(p.to_string_lossy().as_ref()))
                            .copied();
                        let icon = if row.entry.kind == EntryKind::Directory {
                            if this.project.expanded.contains(&path) {
                                IconName::FolderOpen
                            } else {
                                IconName::Folder
                            }
                        } else {
                            IconName::File
                        };
                        let radius = cx.theme().radius;
                        let focus = this
                            .tree_hover_row
                            .filter(|&ix| ix < this.project.rows.len())
                            .or_else(|| {
                                (!this.project.rows.is_empty()).then_some(this.project.selected_row)
                            });
                        let guides =
                            tree_guide_elements(&this.project.rows, i, row.depth, focus, cx);
                        div()
                            .id(("row", i))
                            .role(Role::TreeItem)
                            .aria_label(row.entry.name.clone())
                            .relative()
                            .w_full()
                            .h(px(TREE_ROW_HEIGHT))
                            .rounded(radius)
                            .pl(tree_row_indent(row.depth))
                            .pr_3()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_size(ui(12.))
                            .cursor_default()
                            .when(selected, |el| el.bg(cx.theme().list_active))
                            .when(!selected, |el| el.hover(|el| el.bg(cx.theme().list_hover)))
                            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                if *hovered {
                                    if this.tree_hover_row != Some(i) {
                                        this.tree_hover_row = Some(i);
                                        cx.notify();
                                    }
                                } else if this.tree_hover_row == Some(i) {
                                    this.tree_hover_row = None;
                                    cx.notify();
                                }
                            }))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.project.selected_row = i;
                                this.tree_focus.focus(window, cx);
                                match row.entry.kind {
                                    EntryKind::Directory => this.toggle_directory(path.clone(), cx),
                                    EntryKind::File => this.open_file(path.clone(), window, cx),
                                }
                            }))
                            .on_drag(
                                TreeDrag {
                                    path: drag_path,
                                    name: row.entry.name.clone().into(),
                                },
                                |drag, _, _, cx| {
                                    cx.new(|_| TreeDragPreview {
                                        name: drag.name.clone(),
                                    })
                                },
                            )
                            .on_drop(cx.listener(move |this, drag: &TreeDrag, window, cx| {
                                this.drop_tree_entry(
                                    drag.path.clone(),
                                    drop_path.clone(),
                                    window,
                                    cx,
                                );
                            }))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                    // Naming another row abandons the field that
                                    // was open, which shifts every index below it.
                                    if this.editing.is_some() {
                                        this.cancel_create(window, cx);
                                    }
                                    let index = this
                                        .project
                                        .rows
                                        .iter()
                                        .position(|row| row.entry.path == menu_path)
                                        .unwrap_or(i);
                                    this.project.selected_row =
                                        index.min(this.project.rows.len().saturating_sub(1));
                                    this.tree_focus.focus(window, cx);
                                    this.open_menu(
                                        MenuTarget::Tree {
                                            path: menu_path.clone(),
                                            root: false,
                                        },
                                        event.position,
                                        window,
                                        cx,
                                    );
                                    cx.stop_propagation();
                                }),
                            )
                            .children(guides)
                            .child(
                                Icon::new(icon)
                                    .small()
                                    .text_color(cx.theme().muted_foreground),
                            )
                            .child(div().flex_1().min_w_0().truncate().child(row.entry.name))
                            .when_some(status, |el, status| {
                                el.child(Icon::new(IconName::Asterisk).xsmall().text_color(
                                    if status == 'U' {
                                        cx.theme().success
                                    } else {
                                        cx.theme().warning
                                    },
                                ))
                            })
                            .when(selected && cx.theme().list.active_highlight, |el| {
                                el.child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .rounded(radius)
                                        .border_1()
                                        .border_color(cx.theme().list_active_border),
                                )
                            })
                            .into_any_element()
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.project.tree_scroll)
        .p_1()
        .flex_1()
        .into_any_element()
    }

    /// The text field that names a new entry, drawn in the row it will occupy.
    fn render_inline_edit(&self, cx: &Context<Self>) -> AnyElement {
        let Some(edit) = &self.editing else {
            return div().into_any_element();
        };
        let depth = self
            .project
            .rows
            .get(edit.row)
            .map(|row| row.depth)
            .unwrap_or(0);
        let focus = self
            .tree_hover_row
            .filter(|&ix| ix < self.project.rows.len())
            .or_else(|| (!self.project.rows.is_empty()).then_some(self.project.selected_row));
        div()
            .id("inline-edit")
            .relative()
            .w_full()
            .h(px(TREE_ROW_HEIGHT))
            .rounded(cx.theme().radius)
            .pl(tree_row_indent(depth))
            .pr_3()
            .flex()
            .items_center()
            .gap_2()
            .children(tree_guide_elements(
                &self.project.rows,
                edit.row,
                depth,
                focus,
                cx,
            ))
            .child(
                Icon::new(if edit.kind == EntryKind::Directory {
                    IconName::Folder
                } else {
                    IconName::File
                })
                .small()
                .text_color(cx.theme().muted_foreground),
            )
            .child(div().flex_1().min_w_0().child(
                // No border and no background, so the field reads as part
                // of the row rather than a box dropped into the tree.
                Input::new(&edit.input).appearance(false).h(px(20.)),
            ))
            .into_any_element()
    }

    /// Swallows the click that dismisses the menu, so the tree underneath does
    /// not also act on it.
    fn render_menu_backdrop(&self, cx: &Context<Self>) -> AnyElement {
        div()
            .id("tree-menu-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.close_menu(cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| this.close_menu(cx)),
            )
            .on_scroll_wheel(cx.listener(|this, _, _, cx| this.close_menu(cx)))
            .into_any_element()
    }

    /// The right-click menu. Drawn last in the root stack so it covers the tree
    /// and the lookup panel, and darker than the sidebar it opens over so the
    /// edge is visible without relying on the shadow alone.
    fn render_menu(&self, menu: &ContextMenu, cx: &Context<Self>) -> AnyElement {
        let (entries, rules) = surface_of(&menu.target).menu();
        let path = menu.target.path();
        div()
            .id("tree-menu")
            .absolute()
            .left(menu.position.x)
            .top(menu.position.y)
            .w(px(MENU_WIDTH))
            .occlude()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .shadow_lg()
            .p_1()
            .flex()
            .flex_col()
            .text_size(ui(12.))
            .text_color(cx.theme().foreground)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(
                visible_menu_items(&menu.target)
                    .into_iter()
                    .enumerate()
                    .flat_map(|(position, index)| {
                        let (item, label, shortcut) = entries[index];
                        let label = self.menu_label(item, path, label);
                        let enabled = self.menu_item_enabled(item, path);
                        // A rule only ever sits between two entries.
                        let separator = (position > 0 && rules.contains(&index)).then(|| {
                            div()
                                .h(px(1.))
                                .my(px(4.))
                                .bg(cx.theme().border)
                                .into_any_element()
                        });
                        let row = div()
                            .id(("menu-item", index))
                            .role(Role::Button)
                            .aria_label(label.clone())
                            .h(px(MENU_ROW))
                            .px_2()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_4()
                            .rounded_sm()
                            .when(index == menu.selected, |el| el.bg(cx.theme().list_active))
                            .when(enabled, |el| {
                                el.cursor_pointer()
                                    .hover(|el| el.bg(cx.theme().list_hover))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.run_menu_item(item, window, cx)
                                    }))
                            })
                            .when(!enabled, |el| {
                                el.cursor_default()
                                    .text_color(cx.theme().muted_foreground)
                                    .opacity(0.5)
                            })
                            .child(div().child(label))
                            .child(
                                div()
                                    .text_size(ui(11.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(shortcut_label(shortcut)),
                            )
                            .into_any_element();
                        separator.into_iter().chain(std::iter::once(row))
                    }),
            )
            .into_any_element()
    }

    fn relative_path(&self, path: &Path) -> String {
        self.project
            .workspace
            .as_ref()
            .and_then(|workspace| path.strip_prefix(&workspace.root).ok())
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned()
    }

    /// The `⌘P` / `⇧⌘F` overlay. Both lookups share one panel so the mode switch
    /// is a click away instead of a separate dialog.
    fn render_panel(&self, panel: Panel, cx: &mut Context<Self>) -> AnyElement {
        let width = if panel == Panel::Search { 720. } else { 520. };
        div()
            .id("panel")
            .absolute()
            .top(px(48.))
            .left(relative(0.5))
            .ml(px(-width / 2.))
            .w(px(width))
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().sidebar)
            .shadow_lg()
            .p_2()
            .flex()
            .flex_col()
            .gap_2()
            .child(self.render_panel_header(panel, cx))
            .when(panel == Panel::Files, |el| {
                el.child(self.render_file_finder(cx))
            })
            .when(panel == Panel::Search, |el| {
                el.child(self.render_project_search(cx))
            })
            .into_any_element()
    }

    /// Mode tabs, the `Aa` / `ab` / `.*` filters, and the replace toggle.
    fn render_panel_header(&self, panel: Panel, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap_1()
            .child(self.panel_tab("panel-tab-files", "Files", Panel::Files, cx))
            .child(self.panel_tab("panel-tab-search", "Contents", Panel::Search, cx))
            .child(div().flex_1())
            .when(panel == Panel::Search, |el| {
                el.child(self.option_toggle(
                    "search-case",
                    "Aa",
                    "Match case",
                    self.search.options.case_sensitive,
                    |options| options.case_sensitive = !options.case_sensitive,
                    cx,
                ))
                .child(self.option_toggle(
                    "search-word",
                    "ab",
                    "Match whole word",
                    self.search.options.whole_word,
                    |options| options.whole_word = !options.whole_word,
                    cx,
                ))
                .child(self.option_toggle(
                    "search-regex",
                    ".*",
                    "Regular expression; $1 in the replacement refers to a capture group",
                    self.search.options.regex,
                    |options| options.regex = !options.regex,
                    cx,
                ))
                .child(Self::icon_button(
                    "search-replace-toggle",
                    IconName::Replace,
                    "Replace",
                    |this, window, cx| {
                        this.search.show_replace = !this.search.show_replace;
                        if this.search.show_replace {
                            this.replace_query
                                .update(cx, |query, cx| query.focus(window, cx));
                        } else {
                            this.search_query
                                .update(cx, |query, cx| query.focus(window, cx));
                        }
                        cx.notify();
                    },
                    cx,
                ))
            })
            .child(Self::icon_button(
                "panel-close",
                IconName::Close,
                "Close",
                |this, window, cx| this.close_panel(window, cx),
                cx,
            ))
            .into_any_element()
    }

    fn panel_tab(
        &self,
        id: &'static str,
        label: &'static str,
        panel: Panel,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = self.panel == Some(panel);
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(label)
            .focusable()
            .tab_index(0)
            .h(px(22.))
            .px_2()
            .flex()
            .items_center()
            .rounded_sm()
            .cursor_pointer()
            .text_size(ui(11.))
            .text_color(if active {
                cx.theme().accent_foreground
            } else {
                cx.theme().muted_foreground
            })
            .hover(|el| el.text_color(cx.theme().foreground))
            .focus_visible(|el| el.text_color(cx.theme().accent_foreground))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, window, cx| this.select_panel(panel, window, cx)))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.select_panel(panel, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(label)
            .into_any_element()
    }

    /// A text toggle. An "on" state is shown with colour alone, matching the
    /// icon rule in AGENTS.md: no background box, no border, hover or not.
    fn option_toggle(
        &self,
        id: &'static str,
        label: &'static str,
        tooltip: &'static str,
        active: bool,
        toggle: fn(&mut search::Options),
        cx: &Context<Self>,
    ) -> AnyElement {
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(if active {
                format!("{tooltip} (on)")
            } else {
                tooltip.to_string()
            })
            .focusable()
            .tab_index(0)
            .h(px(22.))
            .min_w(px(24.))
            .px_1()
            .flex()
            .items_center()
            .justify_center()
            .rounded_sm()
            .cursor_pointer()
            .font_family("JetBrains Mono")
            .text_size(ui(11.))
            .text_color(if active {
                cx.theme().accent_foreground
            } else {
                cx.theme().muted_foreground
            })
            .hover(|el| el.text_color(cx.theme().foreground))
            .focus_visible(|el| el.text_color(cx.theme().accent_foreground))
            .tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(tooltip).build(window, cx)
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_search_option(toggle, cx)))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.toggle_search_option(toggle, cx);
                    cx.stop_propagation();
                }
            }))
            .child(label)
            .into_any_element()
    }

    /// `⌘P`: fuzzy file names, or `:123` to jump to a line.
    fn render_file_finder(&self, cx: &mut Context<Self>) -> AnyElement {
        let query = self.query.read(cx).value();
        let jumping = query.starts_with(':');
        div()
            .flex()
            .flex_col()
            .gap_2()
            // `focus_bordered(false)` drops the focus ring: without it gpui-component
            // repaints the border in `theme().ring` and draws a second ring outside
            // the box, which reads as a grey halo on a panel this small.
            .child(Input::new(&self.query).focus_bordered(false))
            .child(
                div()
                    .px_2()
                    .text_size(ui(11.))
                    .text_color(cx.theme().muted_foreground)
                    .child(if self.project.indexing {
                        "Indexing files…"
                    } else if jumping {
                        "Enter to go to the line · Esc to close"
                    } else {
                        "↑ ↓ to choose · Enter to open · Esc to close"
                    }),
            )
            .when(!jumping, |el| {
                el.child(
                    uniform_list(
                        "quick-results",
                        self.matches.len(),
                        cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                            range
                                .map(|i| {
                                    let path = this.matches[i].clone();
                                    let label = this.relative_path(&path);
                                    div()
                                        .id(("match", i))
                                        .h(px(30.))
                                        .px_2()
                                        .flex()
                                        .items_center()
                                        .text_size(ui(12.))
                                        .rounded_sm()
                                        .when(i == this.match_selected, |el| {
                                            el.bg(cx.theme().list_active)
                                        })
                                        .hover(|el| el.bg(cx.theme().list_hover))
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            this.open_file(path.clone(), window, cx)
                                        }))
                                        .child(div().truncate().child(label))
                                })
                                .collect()
                        }),
                    )
                    .track_scroll(&self.quick_scroll)
                    .h(px((self.matches.len().min(10) * 30) as f32)),
                )
            })
            .into_any_element()
    }

    /// `⇧⌘F`: project-wide content search with optional replace.
    fn render_project_search(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(Input::new(&self.search_query).focus_bordered(false))
            .when_some(self.search.scope.clone(), |el, scope| {
                let relative = self.relative_path(&scope);
                let label = if relative.is_empty() {
                    name(&scope)
                } else {
                    relative
                };
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .px_2()
                        .text_size(ui(11.))
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("In {label}"))
                        .child(Self::icon_button(
                            "search-scope-clear",
                            IconName::Close,
                            "Search the whole project",
                            |this, _, cx| {
                                this.search.scope = None;
                                this.start_search(cx);
                                cx.notify();
                            },
                            cx,
                        )),
                )
            })
            .when(self.search.show_replace, |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(Input::new(&self.replace_query).focus_bordered(false)),
                        )
                        .child(
                            Button::new("replace-all")
                                .text()
                                .label("Replace All")
                                .xsmall()
                                .disabled(
                                    self.search.results.is_empty()
                                        || self.search.error.is_some()
                                        || self.search.running
                                        || self.saving,
                                )
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.replace_project(window, cx)
                                })),
                        ),
                )
            })
            .child(self.render_search_status(cx))
            .when(!self.search.rows.is_empty(), |el| {
                el.child(
                    uniform_list(
                        "project-search-results",
                        self.search.rows.len(),
                        cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                            range.map(|i| this.render_search_row(i, cx)).collect()
                        }),
                    )
                    .track_scroll(&self.search.scroll)
                    .h(px((self.search.rows.len().min(12) * 26) as f32)),
                )
            })
            .into_any_element()
    }

    fn render_search_status(&self, cx: &Context<Self>) -> AnyElement {
        let error = self.search.error.is_some();
        let text = if let Some(error) = &self.search.error {
            error.clone()
        } else if self.project.indexing && self.search.running {
            "Indexing files…".into()
        } else if self.search.running {
            "Searching…".into()
        } else if self.search.query.trim().is_empty() {
            "Type to search the project".into()
        } else if self.search.results.is_empty() {
            "No results".into()
        } else {
            format!(
                "{} matches · {} files{}",
                self.total_hits(),
                self.search.results.len(),
                if self.search.truncated {
                    " · results truncated"
                } else {
                    ""
                }
            )
        };
        div()
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .text_size(ui(11.))
            .text_color(if error {
                cx.theme().warning
            } else {
                cx.theme().muted_foreground
            })
            .child(div().flex_1().min_w_0().child(text))
            .when(!error && !self.search.rows.is_empty(), |el| {
                el.child("↑ ↓ to choose · Enter to open · Esc to close")
            })
            .into_any_element()
    }

    fn render_search_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let selected = index == self.search.selected;
        let base = div()
            .id(("search-row", index))
            .w_full()
            .h(px(26.))
            .flex()
            .items_center()
            .gap_2()
            .text_size(ui(12.))
            .cursor_default()
            .when(selected, |el| el.bg(cx.theme().list_active))
            .hover(|el| el.bg(cx.theme().list_hover));
        match self.search.rows[index] {
            SearchRow::File(file) => {
                let entry = &self.search.results[file];
                let label = self.relative_path(&entry.path);
                let count = entry.hits.len();
                let path = entry.path.clone();
                let position = entry
                    .hits
                    .first()
                    .map(|hit| Position::new(hit.line, hit.column));
                base.px_2()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(position) = position {
                            this.goto = Some((path.clone(), position));
                        }
                        this.open_file(path.clone(), window, cx);
                    }))
                    .child(
                        Icon::new(IconName::File)
                            .xsmall()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(label))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(ui(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{count}")),
                    )
                    .into_any_element()
            }
            SearchRow::Hit { file, hit } => {
                let found = &self.search.results[file].hits[hit];
                let preview = found.preview.clone();
                let before = preview[..found.start].to_string();
                let matched = preview[found.start..found.end].to_string();
                let after = preview[found.end..].to_string();
                let line = found.line + 1;
                let path = self.search.results[file].path.clone();
                let position = Position::new(found.line, found.column);
                base.pl(px(18.))
                    .pr_2()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.goto = Some((path.clone(), position));
                        this.open_file(path.clone(), window, cx);
                    }))
                    .child(
                        div()
                            .w(px(44.))
                            .flex_shrink_0()
                            .text_right()
                            .font_family("JetBrains Mono")
                            .text_size(ui(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{line}")),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .flex()
                            .items_center()
                            .font_family("JetBrains Mono")
                            .child(div().flex_shrink_0().whitespace_nowrap().child(before))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .whitespace_nowrap()
                                    .text_color(cx.theme().accent_foreground)
                                    .child(matched),
                            )
                            .child(div().flex_shrink_0().whitespace_nowrap().child(after)),
                    )
                    .into_any_element()
            }
        }
    }

    fn render_titlebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let project = self
            .project
            .workspace
            .as_ref()
            .map(|w| name(&w.root))
            .unwrap_or_default();
        let relative = self
            .project
            .active
            .as_ref()
            .and_then(|p| {
                self.project
                    .workspace
                    .as_ref()
                    .and_then(|w| p.strip_prefix(&w.root).ok())
            })
            .map(|p| p.to_string_lossy().into_owned());
        let doc = self
            .project
            .active
            .as_ref()
            .and_then(|p| self.project.documents.get(p));
        TitleBar::new()
            .bg(if self.project.workspace.is_some() {
                workspace_chrome(cx)
            } else {
                cx.theme().background
            })
            // The workspace card already draws its own top edge. A second
            // line here would sit on top of that one.
            .border_color(gpui::transparent_black())
            .on_close_window(cx.listener(|this, _, window, cx| this.close_window(window, cx)))
            .child(
                div()
                    .h_full()
                    .flex_1()
                    .min_w_0()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_3()
                    .pr_3()
                    .when(self.project.workspace.is_some(), |el| {
                        el.child(Self::icon_button(
                            "activity-bar-toggle",
                            if self.activity_bar {
                                FolioIcon::PanelLeftDashed
                            } else {
                                FolioIcon::PanelRightDashed
                            },
                            "Toggle activity bar",
                            |this, _, cx| this.toggle_activity_bar(cx),
                            cx,
                        ))
                    })
                    .child(div().text_size(ui(12.)).child(project))
                    .when(relative.is_some(), |el| {
                        el.child(div().text_color(cx.theme().muted_foreground).child("/"))
                    })
                    .child(
                        div()
                            .flex_1()
                            .text_size(ui(12.))
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(relative.unwrap_or_default()),
                    )
                    .when(doc.is_some_and(|d| d.dirty), |el| {
                        el.child(
                            div()
                                .text_color(cx.theme().accent_foreground)
                                .child(Icon::new(IconName::Asterisk).xsmall()),
                        )
                    })
                    .when(self.loading, |el| {
                        el.child(
                            div()
                                .text_size(ui(11.))
                                .text_color(cx.theme().muted_foreground)
                                .child("Loading…"),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_disk_notice(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let path = self.project.active.clone().unwrap_or_default();
        let removed = self.project.disk_notice.get(&path) == Some(&DiskNotice::Removed);
        div()
            .px_4()
            .py_1()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                div()
                    .text_size(ui(11.))
                    .text_color(cx.theme().muted_foreground)
                    .child(if removed {
                        "This file was removed on disk"
                    } else {
                        "This file changed on disk"
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(!removed, |el| {
                        let path = path.clone();
                        el.child(
                            Button::new("reload-disk")
                                .text()
                                .xsmall()
                                .label("Reload")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.reload_from_disk(path.clone(), window, cx);
                                })),
                        )
                    })
                    .child({
                        let path = path.clone();
                        Button::new("keep-disk")
                            .text()
                            .xsmall()
                            .label("Keep")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.project.disk_notice.remove(&path);
                                cx.notify();
                            }))
                    }),
            )
    }

    fn render_workspace(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        // Read here rather than stored: what it says is about the line the
        // caret is on, which moves without anything else changing.
        let blame = self.blame_text(cx);
        // The changes view takes the whole area, so whatever the editor would
        // have shown steps out of the way rather than being covered up.
        let showing_diff = self.project.diff.is_some();
        // A tab can be locked from its own menu. The buffer stays open and
        // readable; only the edits are refused.
        let locked = self
            .project
            .active
            .as_ref()
            .is_some_and(|path| self.project.read_only.contains(path));
        let doc = (!showing_diff).then(|| {
            self.project
                .active
                .as_ref()
                .and_then(|p| self.project.documents.get(p))
        });
        let doc = doc.flatten();
        let image = (!showing_diff).then(|| {
            self.project
                .image
                .as_ref()
                .filter(|(path, _)| self.project.active.as_ref() == Some(path))
        });
        let image = image.flatten();
        div()
            .id("workspace")
            .size_full()
            .flex()
            .rounded(px(WORKSPACE_RADIUS))
            .overflow_hidden()
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .when(self.sidebar, |el| {
                el.child(self.render_tree(cx)).child(
                    div()
                        .id("sidebar-resize")
                        .w(px(5.))
                        .h_full()
                        .flex()
                        .justify_center()
                        .cursor_col_resize()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, _| this.resizing = true),
                        )
                        .child(div().w(px(1.)).h_full().bg(sidebar_rule(cx))),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .rounded_tr(px(WORKSPACE_RADIUS))
                    .rounded_br(px(WORKSPACE_RADIUS))
                    .when(!self.sidebar, |el| {
                        el.rounded_tl(px(WORKSPACE_RADIUS))
                            .rounded_bl(px(WORKSPACE_RADIUS))
                    })
                    .when(self.project.tabs.len() > 1, |el| {
                        el.child(self.render_tabs(cx))
                    })
                    // Everything under the strip keeps the padding
                    // full-screen mode gives it; the strip itself runs
                    // the full width.
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .w_full()
                            .flex()
                            .flex_col()
                            .when(!self.sidebar && !showing_diff, |el| el.px_6())
                            .when(showing_diff, |el| el.child(self.render_diff(cx)))
                            .when_some(doc, |el, doc| {
                                el.when(
                                    self.project.active.as_ref().is_some_and(|path| {
                                        self.project.disk_notice.contains_key(path)
                                    }),
                                    |el| el.child(self.render_disk_notice(cx)),
                                )
                                .when(doc.large, |el| {
                                    el.child(
                                        div()
                                            .px_4()
                                            .py_1()
                                            .text_size(ui(11.))
                                            .text_color(cx.theme().muted_foreground)
                                            .child("Large file · syntax highlighting off"),
                                    )
                                })
                                .child({
                                    // Rendered through `Input` rather than the
                                    // `Editor` wrapper: the wrapper hides the
                                    // context-menu hook, and the wrapper is
                                    // otherwise doing exactly this.
                                    let base = doc.editor.read(cx).base_state().clone();
                                    let editor = {
                                        let capabilities =
                                            base.read(cx).context_menu_capabilities();
                                        let enabled = !capabilities.is_disabled();
                                        EditorMenuState {
                                            enabled,
                                            editable: enabled && !capabilities.is_readonly(),
                                            code_editor: capabilities.is_code_editor(),
                                            has_selection: capabilities.has_selection(),
                                            can_go_to_definition: capabilities
                                                .can_go_to_definition(),
                                            has_code_actions: capabilities.has_code_actions(),
                                        }
                                    };
                                    // The wrapper gives the code editor a
                                    // key context of its own, which is what
                                    // scopes the bracket bindings to it and
                                    // keeps them off every other text field.
                                    div().key_context("FolioEditor").size_full().child(
                                        Input::from_base(&base)
                                            .appearance(false)
                                            .bordered(false)
                                            .focus_bordered(false)
                                            .readonly(
                                                self.saving
                                                    || self.loading
                                                    || self.prompting
                                                    || locked,
                                            )
                                            .context_menu(move |menu, window, cx| {
                                                editor_context_menu(editor, menu, window, cx)
                                            })
                                            // The code size is its own setting, so it
                                            // is absolute rather than in the
                                            // interface's scale.
                                            .text_size(px(self.settings.code_font_size))
                                            .font_family(self.code_font())
                                            .line_height(gpui::relative(1.6))
                                            .rounded_none()
                                            .h_full(),
                                    )
                                })
                                // The line the caret is on, and who last
                                // wrote it. One line rather than a
                                // column in the gutter: the same
                                // reading, without narrowing every line
                                // of code for it.
                                .when_some(blame, |el, text| {
                                    el.child(
                                        div()
                                            .px_4()
                                            .py_1()
                                            .text_size(ui(11.))
                                            .text_color(cx.theme().muted_foreground)
                                            .child(text),
                                    )
                                })
                            })
                            .when_some(image, |el, (path, image)| {
                                el.child(
                                    div().flex_1().min_h_0().w_full().p_6().child(
                                        img(image.clone())
                                            .size_full()
                                            .object_fit(ObjectFit::Contain),
                                    ),
                                )
                                .child(
                                    div()
                                        .px_4()
                                        .py_2()
                                        .text_size(ui(11.))
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!(
                                            "{} · {} × {} · static preview",
                                            name(path),
                                            u32::from(image.size(0).width),
                                            u32::from(image.size(0).height)
                                        )),
                                )
                            })
                            .when(!showing_diff && doc.is_none() && image.is_none(), |el| {
                                el.child(
                                    div()
                                        .size_full()
                                        .flex()
                                        .flex_col()
                                        .gap_3()
                                        .justify_center()
                                        .items_center()
                                        .child(
                                            div()
                                                .text_size(ui(24.))
                                                .text_color(cx.theme().accent_foreground)
                                                .child("Some room to read a little code."),
                                        )
                                        .child(
                                            div()
                                                .text_size(ui(12.))
                                                .text_color(cx.theme().muted_foreground)
                                                .child("Pick a file on the left, or press ⌘ P"),
                                        ),
                                )
                            }),
                    )
                    // The terminal drawer hangs under the content
                    // column alone; the sidebar keeps its full
                    // height, and the editor above shrinks.
                    .when(self.terminal_open, |el| {
                        el.child(self.render_terminal_dock(window, cx))
                    }),
            )
            .into_any_element()
    }
}

/// The About window: the name, the version and what the application is.
struct AboutView;

impl Render for AboutView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("about")
            .key_context("FolioAbout")
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .text_size(ui(13.))
            .child(
                TitleBar::new()
                    .bg(cx.theme().background)
                    .border_color(cx.theme().border)
                    .child(div().h_full().flex_1()),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .child(div().text_size(ui(22.)).child("Folio"))
                    // Read from the manifest rather than written out here, so
                    // it cannot drift from the version that was built.
                    .child(
                        div()
                            .text_size(ui(12.))
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("Version {}", env!("CARGO_PKG_VERSION"))),
                    )
                    .child(
                        div()
                            .text_size(ui(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child("A local code reading editor, built with Rust and GPUI."),
                    ),
            )
    }
}

/// The `⌘,` window: a plain form over `Folio`'s settings. It owns its own text
/// fields and pushes every change back through the app view, so there is one
/// copy of the settings and one place that applies them.
/// A page of settings. These are the leaves of the sidebar tree, and each one
/// is one screen of rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page {
    Interface,
    Type,
    Indentation,
    Sidebar,
    Ignored,
}

impl Page {
    fn title(self) -> &'static str {
        match self {
            Page::Interface => "Interface",
            Page::Type => "Type",
            Page::Indentation => "Indentation",
            Page::Sidebar => "Sidebar",
            Page::Ignored => "Ignored Folders",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Page::Interface => "interface",
            Page::Type => "type",
            Page::Indentation => "indentation",
            Page::Sidebar => "sidebar",
            Page::Ignored => "ignored",
        }
    }

    fn subtitle(self) -> &'static str {
        match self {
            Page::Interface => "How the interface itself looks",
            Page::Type => "The font code is drawn in",
            Page::Indentation => "Tabs, spaces and how wide they are",
            Page::Sidebar => "How the window is arranged",
            Page::Ignored => "Which files a project shows",
        }
    }
}

/// A heading in the sidebar and the pages under it. Zed's shape: the headings
/// collapse, and only the pages are screens.
struct Group {
    title: &'static str,
    pages: &'static [Page],
}

const GROUPS: &[Group] = &[
    Group {
        title: "Appearance",
        pages: &[Page::Interface],
    },
    Group {
        title: "Editor",
        pages: &[Page::Type, Page::Indentation],
    },
    Group {
        title: "Window & Layout",
        pages: &[Page::Sidebar],
    },
    Group {
        title: "Files",
        pages: &[Page::Ignored],
    },
];

struct SettingsView {
    folio: WeakEntity<Folio>,
    /// A snapshot of what is stored. The view renders this instead of reading
    /// the app view: the settings window is built from inside `Folio`'s own
    /// update, and rendering it while that is in flight would be a re-entrant
    /// read. It is only ever written through `set_settings` and `change`.
    settings: Settings,
    /// Which page is showing, and which headings are open. A view of its own
    /// rather than part of the stored settings: it is where you were, not what
    /// you chose.
    page: Page,
    expanded: [bool; GROUPS.len()],
    font_query: Entity<InputState>,
    code_font_query: Entity<InputState>,
    ignore_query: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl SettingsView {
    fn new(
        folio: WeakEntity<Folio>,
        settings: Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let font_query = cx.new(|cx| InputState::new(window, cx).placeholder("System default"));
        let code_font_query =
            cx.new(|cx| InputState::new(window, cx).placeholder("JetBrains Mono"));
        let ignore_query =
            cx.new(|cx| InputState::new(window, cx).placeholder("node_modules, target"));
        font_query.update(cx, |input, cx| {
            input.set_value(settings.font_family.clone().unwrap_or_default(), window, cx)
        });
        code_font_query.update(cx, |input, cx| {
            input.set_value(
                settings.code_font_family.clone().unwrap_or_default(),
                window,
                cx,
            )
        });
        ignore_query.update(cx, |input, cx| {
            input.set_value(settings.ignored.join(", "), window, cx)
        });
        // All three fields settle the same way, and each one re-reads all of
        // them, so a value typed in one is not dropped by settling another.
        // Enter and clicking away both count as settled.
        let subscriptions = [&font_query, &code_font_query, &ignore_query]
            .into_iter()
            .map(|input| {
                cx.subscribe_in(
                    input,
                    window,
                    |this: &mut Self, _, event: &InputEvent, window, cx| {
                        if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                            this.commit_text(window, cx);
                        }
                    },
                )
            })
            .collect();
        Self {
            folio,
            settings,
            page: Page::Interface,
            // Open to begin with: five pages do not need hiding, and a tree
            // that starts folded reads as a list of nothing.
            expanded: [true; GROUPS.len()],
            font_query,
            code_font_query,
            ignore_query,
            _subscriptions: subscriptions,
        }
    }

    /// Adopt what the app view settled on. Called after every apply, so a
    /// value the app clamped or rejected shows up here too.
    fn set_settings(&mut self, settings: Settings, cx: &mut Context<Self>) {
        if self.settings != settings {
            self.settings = settings;
            cx.notify();
        }
    }

    /// Apply a change through the app view, so the theme, the tree, the file
    /// and the other window all move together. The snapshot is updated first
    /// so this window draws the new value on its next frame either way.
    fn change(&mut self, change: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        change(&mut self.settings);
        self.settings = self.settings.clone().clamped();
        if let Some(folio) = self.folio.upgrade() {
            let settings = self.settings.clone();
            folio.update(cx, |folio, cx| {
                folio.settings = settings;
                folio.apply_settings(cx);
            });
        }
        cx.notify();
    }

    /// Pull the text fields into the settings. All three are read together, so
    /// committing one does not drop what was typed in another.
    fn commit_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let font = self.font_query.read(cx).value().trim().to_string();
        let code_font = self.code_font_query.read(cx).value().trim().to_string();
        let ignored = self.ignore_query.read(cx).value().to_string();
        self.change(
            move |settings| {
                settings.font_family = (!font.is_empty()).then_some(font);
                settings.code_font_family = (!code_font.is_empty()).then_some(code_font);
                settings.ignored = ignored
                    .split(',')
                    .map(|name| name.trim().to_string())
                    .filter(|name| !name.is_empty())
                    .collect();
            },
            cx,
        );
        let _ = window;
    }

    /// A row: a fixed-width label, then the control.
    /// A settings row, the shape Zed gives them: what the setting is called
    /// and what it does stacked on the left, its control flush right, and a
    /// hairline underneath to separate it from the next one.
    fn row(
        title: &'static str,
        description: &'static str,
        control: AnyElement,
        cx: &Context<Self>,
    ) -> AnyElement {
        div()
            .px_6()
            .py_3()
            .flex()
            .items_center()
            .gap_6()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(ui(12.))
                            .text_color(cx.theme().foreground)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(ui(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child(description),
                    ),
            )
            .child(div().flex_shrink_0().child(control))
            .into_any_element()
    }

    /// A field in a row. Wide enough for a font name or a short path list, and
    /// the same width everywhere so the controls line up down the column.
    fn field(&self, input: &Entity<InputState>) -> AnyElement {
        div()
            .w(px(FIELD_WIDTH))
            .child(Input::new(input).focus_bordered(false))
            .into_any_element()
    }

    /// A sidebar row that opens and closes the pages under it. Clicking the
    /// row itself does the toggling, which is what the chevron promises.
    fn group_row(&self, index: usize, cx: &Context<Self>) -> AnyElement {
        let group = &GROUPS[index];
        let open = self.expanded[index];
        // A heading counts as current while one of its pages is showing, so
        // the open page is never hidden inside a collapsed-looking heading.
        let current = group.pages.contains(&self.page);
        div()
            .id(format!("settings-group-{}", index))
            .role(Role::Button)
            .aria_label(group.title)
            .aria_expanded(open)
            .focusable()
            .tab_index(0)
            .h(px(30.))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .rounded_sm()
            .cursor_pointer()
            .text_size(ui(12.))
            .text_color(if current {
                cx.theme().foreground
            } else {
                cx.theme().muted_foreground
            })
            .hover(|el| el.bg(cx.theme().list_hover))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.expanded[index] = !this.expanded[index];
                cx.notify();
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.expanded[index] = !this.expanded[index];
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            .child(
                Icon::new(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .xsmall(),
            )
            .child(group.title)
            .into_any_element()
    }

    /// A page under a heading. Indented to sit under it, the way Zed nests
    /// them.
    fn page_row(&self, page: Page, cx: &Context<Self>) -> AnyElement {
        let selected = self.page == page;
        div()
            .id(format!("settings-page-{}", page.key()))
            .role(Role::Button)
            .aria_label(page.title())
            .focusable()
            .tab_index(0)
            .h(px(28.))
            .pl(px(26.))
            .pr_2()
            .flex()
            .items_center()
            .rounded_sm()
            .cursor_pointer()
            .text_size(ui(12.))
            .text_color(if selected {
                cx.theme().foreground
            } else {
                cx.theme().muted_foreground
            })
            .when(selected, |el| el.bg(cx.theme().list_active))
            .hover(|el| el.bg(cx.theme().list_hover))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.page = page;
                cx.notify();
            }))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    this.page = page;
                    cx.notify();
                    cx.stop_propagation();
                }
            }))
            .child(page.title())
            .into_any_element()
    }

    /// The sidebar. Same surface as the pane beside it — the split is shown by
    /// the hairline, not by two shades.
    fn render_sidebar(&self, cx: &Context<Self>) -> AnyElement {
        div()
            .w(px(204.))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .bg(cx.theme().background)
            .border_r_1()
            .border_color(cx.theme().border)
            .children(GROUPS.iter().enumerate().flat_map(|(index, group)| {
                let open = self.expanded[index];
                std::iter::once(self.group_row(index, cx)).chain(open.then(|| {
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .children(group.pages.iter().map(|page| self.page_row(*page, cx)))
                        .into_any_element()
                }))
            }))
            .into_any_element()
    }

    /// The bordered control the settings rows are built from: a rounded
    /// outline with its actions split by hairline dividers, the shape Zed's
    /// settings use. One box reads as one control where loose buttons read as
    /// three, and it keeps the row's hit area in one piece.
    fn control_box(children: Vec<AnyElement>, cx: &Context<Self>) -> AnyElement {
        div()
            .h(px(28.))
            .flex()
            .items_center()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().border)
            // So a hovered end segment stops at the rounded corners.
            .overflow_hidden()
            .children(children)
            .into_any_element()
    }

    fn control_divider(cx: &Context<Self>) -> AnyElement {
        div()
            .w(px(1.))
            .h_full()
            .flex_shrink_0()
            .bg(cx.theme().border)
            .into_any_element()
    }

    /// One segment of a control. `selected` only means something in a group
    /// that shows state; a stepper's segments are never selected.
    fn control_segment(
        id: String,
        label: &'static str,
        selected: bool,
        width: Pixels,
        content: AnyElement,
        activate: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &Context<Self>,
    ) -> AnyElement {
        let activate = std::rc::Rc::new(activate);
        let keyboard = activate.clone();
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(if selected {
                format!("{label} (on)")
            } else {
                label.to_string()
            })
            .focusable()
            .tab_index(0)
            .w(width)
            .h_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .text_size(ui(11.))
            .text_color(if selected {
                cx.theme().accent_foreground
            } else {
                cx.theme().muted_foreground
            })
            .when(selected, |el| el.bg(cx.theme().list_active))
            .hover(|el| {
                el.bg(cx.theme().list_hover)
                    .text_color(cx.theme().foreground)
            })
            .focus_visible(|el| el.text_color(cx.theme().accent_foreground))
            .on_click(cx.listener(move |this, _, window, cx| activate(this, window, cx)))
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    keyboard(this, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(content)
            .into_any_element()
    }

    /// A numeric setting: the value between a `−` and a `+`, in one box.
    fn stepper(
        &self,
        id: &'static str,
        value: String,
        step: fn(&mut Settings, f32),
        cx: &Context<Self>,
    ) -> AnyElement {
        let button = |id: String, icon: IconName, label: &'static str, delta: f32| {
            Self::control_segment(
                id,
                label,
                false,
                px(30.),
                Icon::new(icon).xsmall().into_any_element(),
                move |this, _, cx| this.change(move |settings| step(settings, delta), cx),
                cx,
            )
        };
        Self::control_box(
            vec![
                button(
                    format!("settings-down-{id}"),
                    IconName::Minus,
                    "Decrease",
                    -1.,
                ),
                Self::control_divider(cx),
                div()
                    .w(px(58.))
                    .h_full()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(ui(12.))
                    .child(value)
                    .into_any_element(),
                Self::control_divider(cx),
                button(format!("settings-up-{id}"), IconName::Plus, "Increase", 1.),
            ],
            cx,
        )
    }

    /// A two-choice setting, in the same box as a stepper.
    fn choice(
        &self,
        id: &'static str,
        left: (&'static str, bool, fn(&mut Settings)),
        right: (&'static str, bool, fn(&mut Settings)),
        cx: &Context<Self>,
    ) -> AnyElement {
        let segment = |side: &str, option: (&'static str, bool, fn(&mut Settings))| {
            let (label, selected, set) = option;
            Self::control_segment(
                format!("{id}-{side}"),
                label,
                selected,
                px(72.),
                div().child(label).into_any_element(),
                move |this, _, cx| this.change(set, cx),
                cx,
            )
        };
        Self::control_box(
            vec![
                segment("left", left),
                Self::control_divider(cx),
                segment("right", right),
            ],
            cx,
        )
    }

    /// The rows of the open page, under its title. Everything is in one
    /// column: the window is tall enough for the longest page, so nothing here
    /// scrolls.
    fn render_page(&self, settings: &Settings, cx: &Context<Self>) -> AnyElement {
        let page = self.page;
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .px_6()
                    .py_4()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_size(ui(15.)).child(page.title()))
                    .child(
                        div()
                            .text_size(ui(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child(page.subtitle()),
                    ),
            )
            .child(div().h(px(1.)).flex_shrink_0().bg(cx.theme().border))
            .children(match page {
                Page::Interface => vec![
                    Self::row(
                        "Font",
                        "Used for menus, the sidebar and the settings window.",
                        self.field(&self.font_query),
                        cx,
                    ),
                    Self::row(
                        "Size",
                        "Scales every label in the app.",
                        self.stepper(
                            "interface-size",
                            format!("{:.0}", settings.font_size),
                            |settings, step| settings.font_size += step,
                            cx,
                        ),
                        cx,
                    ),
                ],
                Page::Type => vec![
                    Self::row(
                        "Font",
                        "Used in the editor. Glyphs it has no coverage for fall back to a CJK face.",
                        self.field(&self.code_font_query),
                        cx,
                    ),
                    Self::row(
                        "Soft wrap",
                        "Break long lines to the editor width. Off keeps a character grid.",
                        self.choice(
                            "soft-wrap",
                            ("On", settings.soft_wrap, |settings| settings.soft_wrap = true),
                            ("Off", !settings.soft_wrap, |settings| {
                                settings.soft_wrap = false
                            }),
                            cx,
                        ),
                        cx,
                    ),
                    Self::row(
                        "Size",
                        "How large code is drawn.",
                        self.stepper(
                            "code-size",
                            format!("{:.0}", settings.code_font_size),
                            |settings, step| settings.code_font_size += step,
                            cx,
                        ),
                        cx,
                    ),
                ],
                Page::Indentation => vec![
                    Self::row(
                        "Tab size",
                        "Applies to files opened from now on.",
                        self.stepper(
                            "tab-size",
                            settings.tab_size.to_string(),
                            |settings, step| {
                                settings.tab_size =
                                    (settings.tab_size as f32 + step).clamp(1., 8.) as usize
                            },
                            cx,
                        ),
                        cx,
                    ),
                    Self::row(
                        "Indent with",
                        "Spaces or tab characters.",
                        self.choice(
                            "indent",
                            ("Spaces", !settings.hard_tabs, |settings| {
                                settings.hard_tabs = false
                            }),
                            ("Tabs", settings.hard_tabs, |settings| settings.hard_tabs = true),
                            cx,
                        ),
                        cx,
                    ),
                ],
                Page::Sidebar => vec![
                    Self::row(
                        "Show the file tree",
                        "Whether the project tree is shown when Folio opens.",
                        self.choice(
                            "sidebar",
                            ("Shown", settings.sidebar, |settings| settings.sidebar = true),
                            ("Hidden", !settings.sidebar, |settings| settings.sidebar = false),
                            cx,
                        ),
                        cx,
                    ),
                    Self::row(
                        "Show the activity bar",
                        "The icon rail on the far left. The title-bar button writes this back.",
                        self.choice(
                            "activity-bar",
                            (
                                "Shown",
                                settings.activity_bar,
                                |settings| settings.activity_bar = true,
                            ),
                            (
                                "Hidden",
                                !settings.activity_bar,
                                |settings| settings.activity_bar = false,
                            ),
                            cx,
                        ),
                        cx,
                    ),
                ],
                Page::Ignored => vec![Self::row(
                    "Folders",
                    "Comma-separated folder names, hidden from the tree and skipped by search.",
                    self.field(&self.ignore_query),
                    cx,
                )],
            })
            .into_any_element()
    }
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = self.settings.clone();
        div()
            .id("settings")
            .key_context("FolioSettings")
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .text_size(ui(13.))
            .on_action(cx.listener(|_, _: &CloseWindow, window, _| window.remove_window()))
            .on_action(cx.listener(|this, _: &Quit, window, cx| {
                if let Some(folio) = this.folio.upgrade() {
                    folio.update(cx, |folio, cx| folio.request(Next::Quit, window, cx));
                }
            }))
            // Drawn by the app rather than AppKit, so it carries the theme's
            // own background in both appearances. The traffic lights are still
            // the real ones, sitting over the transparent title bar area.
            .child(
                TitleBar::new()
                    .bg(cx.theme().background)
                    .border_color(cx.theme().border)
                    .child(
                        div()
                            .h_full()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_center()
                            .pr_3()
                            .text_size(ui(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child("Settings"),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.render_sidebar(cx))
                    .child(self.render_page(&settings, cx)),
            )
    }
}

impl Render for Folio {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.applied_wrap != self.settings.soft_wrap {
            self.sync_soft_wrap(window, cx);
        }
        div()
            .id("folio")
            .key_context("Folio")
            // GPUI's own fallback stack names no CJK family, so without this a
            // Chinese glyph is drawn in whatever the platform picks — usually a
            // proportional face, which breaks the character grid.
            .font(Font {
                family: cx.theme().font_family.clone(),
                fallbacks: Some(FontFallbacks::from_fonts(
                    CJK_FALLBACKS.iter().map(|name| name.to_string()).collect(),
                )),
                ..Default::default()
            })
            .when(self.project.workspace.is_none(), |el| {
                el.track_focus(&self.tree_focus)
            })
            .size_full()
            .flex()
            .flex_col()
            .relative()
            .bg(if self.project.workspace.is_some() {
                workspace_chrome(cx)
            } else {
                cx.theme().background
            })
            .text_color(cx.theme().foreground)
            .text_size(ui(13.))
            .on_action(cx.listener(|this, _: &OpenProject, window, cx| {
                this.request(Next::Picker, window, cx)
            }))
            .on_action(cx.listener(|this, _: &CloseProject, window, cx| {
                this.request(Next::Close, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &Quit, window, cx| this.request(Next::Quit, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &Save, window, cx| this.save_documents(None, window, cx)),
            )
            .on_action(cx.listener(|this, _: &QuickOpen, window, cx| {
                this.show_quick_open(false, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ProjectSearch, window, cx| {
                this.show_search(false, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ProjectReplace, window, cx| {
                this.show_search(true, window, cx)
            }))
            .on_action(
                cx.listener(|this, _: &GoToLine, window, cx| {
                    this.show_quick_open(true, window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.toggle_file_tree(cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleActivityBar, _, cx| {
                this.toggle_activity_bar(cx);
            }))
            .on_action(
                cx.listener(|this, _: &ToggleTerminal, window, cx| {
                    this.toggle_terminal(window, cx)
                }),
            )
            .on_action(cx.listener(|this, _: &OpenSettings, _, cx| this.show_settings(cx)))
            .on_action(cx.listener(|this, _: &ToggleDiff, window, cx| this.toggle_diff(window, cx)))
            .on_action(
                cx.listener(|this, _: &ToggleBlame, window, cx| this.toggle_blame(window, cx)),
            )
            .on_action(cx.listener(|this, _: &OpenAbout, _, cx| this.show_about(cx)))
            .on_action(cx.listener(|this, _: &CheckForUpdates, _, cx| this.check_for_updates(cx)))
            .on_action(
                cx.listener(|this, _: &ToggleComment, window, cx| this.toggle_comment(window, cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleBlockComment, window, cx| {
                this.toggle_block_comment(window, cx)
            }))
            .on_action(cx.listener(|this, _: &DuplicateLines, window, cx| {
                this.duplicate_lines(window, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveLinesUp, window, cx| {
                this.move_lines(false, window, cx)
            }))
            .on_action(cx.listener(|this, _: &MoveLinesDown, window, cx| {
                this.move_lines(true, window, cx)
            }))
            // The editor's own commands, bound only where the code editor has
            // the keyboard.
            .on_action(cx.listener(|this, _: &Fold, _, cx| {
                this.on_active_buffer(cx, |base, cx| {
                    base.fold_at_cursor(cx);
                })
            }))
            .on_action(cx.listener(|this, _: &Unfold, _, cx| {
                this.on_active_buffer(cx, |base, cx| {
                    base.unfold_at_cursor(cx);
                })
            }))
            .on_action(cx.listener(|this, _: &SelectNextOccurrence, _, cx| {
                this.on_active_buffer(cx, |base, cx| base.select_next_occurrence(cx))
            }))
            .on_action(cx.listener(|this, _: &SelectAllOccurrences, _, cx| {
                this.on_active_buffer(cx, |base, cx| base.select_all_occurrences(cx))
            }))
            .on_action(cx.listener(|this, _: &AddCursorAbove, _, cx| {
                this.on_active_buffer(cx, |base, cx| base.add_cursor_above(cx))
            }))
            .on_action(cx.listener(|this, _: &AddCursorBelow, _, cx| {
                this.on_active_buffer(cx, |base, cx| base.add_cursor_below(cx))
            }))
            .on_action(cx.listener(|this, _: &SelectColumnUp, _, cx| {
                this.on_active_buffer(cx, |base, cx| base.extend_box(-1, cx))
            }))
            .on_action(cx.listener(|this, _: &SelectColumnDown, _, cx| {
                this.on_active_buffer(cx, |base, cx| base.extend_box(1, cx))
            }))
            // These are bound to the code editor's own key context, so the
            // search box and the settings fields keep typing brackets the
            // ordinary way.
            .on_action(
                cx.listener(|this, _: &PairParen, window, cx| this.open_pair("(", ")", window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &PairBracket, window, cx| {
                    this.open_pair("[", "]", window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &PairBrace, window, cx| this.open_pair("{", "}", window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &PairQuote, window, cx| this.pair_quote("\"", window, cx)),
            )
            // An apostrophe is safe to pair only because a quote after a word
            // is not treated as an opening quote.
            .on_action(
                cx.listener(|this, _: &PairApostrophe, window, cx| {
                    this.pair_quote("'", window, cx)
                }),
            )
            .on_action(
                cx.listener(|this, _: &PairBacktick, window, cx| this.pair_quote("`", window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SkipParen, window, cx| this.skip_closer(")", window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SkipBracket, window, cx| this.skip_closer("]", window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &SkipBrace, window, cx| this.skip_closer("}", window, cx)),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                this.resizing &= event.dragging();
                this.resizing_terminal &= event.dragging();
                if this.resizing {
                    this.sidebar_width =
                        (f32::from(event.position.x) - this.workspace_left_inset()).clamp(
                            160.,
                            (f32::from(window.viewport_size().width) * 0.4).max(160.),
                        );
                    cx.notify();
                }
                if this.resizing_terminal {
                    let viewport = window.viewport_size();
                    this.terminal_height = (f32::from(viewport.height)
                        - f32::from(event.position.y))
                    .clamp(120., f32::from(viewport.height) * 0.7);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.resizing = false;
                    this.resizing_terminal = false;
                    // A drag that ended anywhere, dropped or not, takes its
                    // caret with it.
                    if this.tab_drop.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                if let Some(path) = paths.0.first() {
                    this.request(Next::Open(path.clone()), window, cx);
                }
            }))
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                // The menu sits on top of everything, so it gets first refusal
                // on every key while it is open.
                if this.menu.is_some() {
                    match event.keystroke.key.as_str() {
                        "escape" => this.close_menu(cx),
                        "up" => this.move_menu_selection(false),
                        "down" => this.move_menu_selection(true),
                        "enter" => {
                            let chosen = this.menu.as_ref().and_then(|menu| {
                                surface_of(&menu.target)
                                    .menu()
                                    .0
                                    .get(menu.selected)
                                    .map(|(item, _, _)| *item)
                            });
                            if let Some(item) = chosen {
                                this.run_menu_item(item, window, cx);
                            }
                        }
                        _ => {
                            let Some(menu) = this.menu.as_ref() else {
                                return;
                            };
                            let (entries, _) = surface_of(&menu.target).menu();
                            let Some(item) = visible_menu_items(&menu.target)
                                .into_iter()
                                .filter_map(|index| entries.get(index))
                                .find(|(_, _, shortcut)| {
                                    matches_shortcut(&event.keystroke, shortcut)
                                })
                                .map(|(item, _, _)| *item)
                            else {
                                // Anything else keeps its usual meaning.
                                return;
                            };
                            this.run_menu_item(item, window, cx);
                        }
                    }
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                // The editor binds ⌘⇧F to its own in-file replace, and a
                // binding on the focused element beats one further out — so
                // while the code has focus, which is most of the time, the
                // project search never saw it. The capture phase runs before
                // bindings are resolved at all, so it is claimed here.
                if matches_shortcut(&event.keystroke, "\u{2318}\u{21e7}F") {
                    this.show_search(false, window, cx);
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                // Escape has to be caught here: the input owns the key context
                // while a row is being named.
                if this.editing.is_some() {
                    if event.keystroke.key == "escape" {
                        this.cancel_create(window, cx);
                        cx.stop_propagation();
                        cx.notify();
                    }
                    return;
                }
                let Some(panel) = this.panel else {
                    // Nothing else wants it: Escape leaves the changes view,
                    // which is otherwise only reachable by its shortcut.
                    if this.project.diff.is_some() && event.keystroke.key == "escape" {
                        this.toggle_diff(window, cx);
                        cx.stop_propagation();
                    }
                    return;
                };
                match event.keystroke.key.as_str() {
                    "escape" => this.close_panel(window, cx),
                    "down" | "up" => {
                        let down = event.keystroke.key == "down";
                        match panel {
                            Panel::Search => this.move_search_selection(down),
                            Panel::Files => {
                                let last = this.matches.len().saturating_sub(1);
                                this.match_selected = if down {
                                    (this.match_selected + 1).min(last)
                                } else {
                                    this.match_selected.saturating_sub(1)
                                };
                                this.quick_scroll
                                    .scroll_to_item(this.match_selected, ScrollStrategy::Nearest);
                            }
                        }
                    }
                    "left" | "right" => {
                        if !this.panel_arrow(event.keystroke.key.as_str(), window, cx) {
                            return;
                        }
                    }
                    _ => return,
                }
                cx.stop_propagation();
                cx.notify();
            }))
            .child(self.render_titlebar(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(if self.project.workspace.is_some() {
                        div()
                            .size_full()
                            .flex()
                            .when(self.activity_bar, |el| {
                                el.child(self.render_activity_bar(cx))
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .pt(px(WORKSPACE_INSET))
                                    .pr(px(WORKSPACE_INSET))
                                    .pb(px(WORKSPACE_INSET))
                                    .pl(px(if self.activity_bar {
                                        WORKSPACE_RAIL_GAP
                                    } else {
                                        WORKSPACE_INSET
                                    }))
                                    .child(self.render_workspace(window, cx)),
                            )
                            .into_any_element()
                    } else {
                        self.render_launcher(cx)
                    }),
            )
            .when_some(self.panel, |el, panel| {
                el.child(self.render_panel(panel, cx))
            })
            .when(self.menu.is_some(), |el| {
                el.child(self.render_menu_backdrop(cx))
            })
            .when_some(self.menu.as_ref(), |el, menu| {
                el.child(self.render_menu(menu, cx))
            })
            .when_some(self.message.clone(), |el, message| {
                el.child(
                    div()
                        .absolute()
                        .bottom(px(18.))
                        .right(px(20.))
                        .max_w(px(600.))
                        .rounded_md()
                        .border_1()
                        .border_color(cx.theme().border)
                        .bg(cx.theme().sidebar)
                        .px_4()
                        .py_3()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(div().flex_1().text_size(ui(12.)).child(message))
                        .child(Self::icon_button(
                            "dismiss-message",
                            IconName::Close,
                            "Dismiss",
                            |this, _, cx| {
                                this.message = None;
                                cx.notify();
                            },
                            cx,
                        )),
                )
            })
    }
}

/// The geometry of every window that remembers one, as x, y, width, height.
#[derive(Default, Serialize, Deserialize)]
pub struct WindowState {
    #[serde(default)]
    pub main: Option<[f32; 4]>,
    #[serde(default)]
    pub settings: Option<[f32; 4]>,
}

impl WindowState {
    /// Read what was saved. The bare array that older versions wrote for the
    /// main window alone still loads, so an upgrade keeps its window.
    pub fn load(file: &Path) -> Self {
        let Ok(raw) = std::fs::read(file) else {
            return Self::default();
        };
        if let Ok(state) = serde_json::from_slice::<Self>(&raw) {
            return state;
        }
        serde_json::from_slice::<[f32; 4]>(&raw)
            .map(|main| Self {
                main: Some(main),
                settings: None,
            })
            .unwrap_or_default()
    }

    pub fn save(&self, file: &Path) {
        let Ok(raw) = serde_json::to_vec_pretty(self) else {
            return;
        };
        let parent = file.parent().unwrap_or(Path::new("."));
        let written = std::fs::create_dir_all(parent).and_then(|()| std::fs::write(file, raw));
        if let Err(error) = written {
            eprintln!("Could not save the window geometry: {error}");
        }
    }
}

fn bounds_values(bounds: Bounds<Pixels>) -> [f32; 4] {
    [
        f32::from(bounds.origin.x),
        f32::from(bounds.origin.y),
        f32::from(bounds.size.width),
        f32::from(bounds.size.height),
    ]
}

/// A saved rectangle, if it is big enough to be worth restoring. Anything not
/// finite, or smaller than the window's own minimum, counts as nothing saved.
pub fn restore_bounds(values: [f32; 4], minimum: Size<Pixels>) -> Option<WindowBounds> {
    if !values.iter().all(|value| value.is_finite())
        || values[2] < f32::from(minimum.width)
        || values[3] < f32::from(minimum.height)
    {
        return None;
    }
    Some(WindowBounds::Windowed(Bounds::new(
        point(px(values[0]), px(values[1])),
        size(px(values[2]), px(values[3])),
    )))
}

/// The smallest window the main view can be, and the size it starts at.
pub const MAIN_WINDOW_MIN: Size<Pixels> = size(px(640.), px(480.));
/// The same for the settings window.
const SETTINGS_WINDOW_MIN: Size<Pixels> = size(px(460.), px(360.));
const SETTINGS_WINDOW_DEFAULT: Size<Pixels> = size(px(918.), px(683.));

pub fn config_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    #[cfg(target_os = "macos")]
    {
        home.join("Library/Application Support/Folio")
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or(home)
            .join("Folio")
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"))
            .join("folio")
    }
}
fn name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}
fn relative_time(time: u64) -> String {
    let age = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .saturating_sub(time);
    match age {
        0..3600 => "just now".into(),
        3600..86400 => format!("{} hours ago", age / 3600),
        _ => format!("{} days ago", age / 86400),
    }
}

#[cfg(all(test, feature = "desktop-tests"))]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[gpui::test]
    fn multiple_projects_keep_buffers_and_guard_all_unsaved_changes(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let a = root.join("A");
        let b = root.join("B");
        for dir in [&a, &b] {
            std::fs::create_dir(dir).unwrap();
            std::fs::write(dir.join("main.rs"), "// original\n").unwrap();
        }
        std::fs::create_dir(a.join("src")).unwrap();
        std::fs::write(a.join("src/lib.rs"), "// child\n").unwrap();
        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let view = cx.update(|window, cx| {
            cx.new(|cx| {
                let mut app = Folio::new(window, cx);
                app.recent_task = None;
                app.recent_file = root.join("recent.json");
                // Never read or write the real settings file from a test.
                app.settings_file = root.join("settings.json");
                app.window_file = root.join("window.json");
                app.settings = Settings::default();
                app.applied_ignored = app.settings.ignored.clone();
                app
            })
        });
        view.update_in(cx, |app, window, cx| {
            app.request(Next::Open(a.clone()), window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            app.open_file(a.join("main.rs"), window, cx)
        });
        cx.run_until_parked();
        let original_editor = view.update_in(cx, |app, window, cx| {
            let editor = app.project.documents[&a.join("main.rs")].editor.clone();
            editor
                .read(cx)
                .base_state()
                .clone()
                .update(cx, |base, cx| base.replace_all("// edit A\n", window, cx));
            app.toggle_directory(a.join("src"), cx);
            // Switch before the child directory read finishes.
            app.request(Next::Open(b.clone()), window, cx);
            editor
        });
        cx.run_until_parked();
        assert!(
            !cx.has_pending_prompt(),
            "adding a project must preserve dirty buffers without prompting"
        );
        view.update_in(cx, |app, window, cx| {
            assert_eq!(app.project_order, vec![a.clone(), b.clone()]);
            assert_eq!(app.parked[0].directories[&a.join("src")].len(), 1);
            app.open_file(b.join("main.rs"), window, cx);
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            app.project.documents[&b.join("main.rs")]
                .editor
                .read(cx)
                .base_state()
                .clone()
                .update(cx, |base, cx| base.replace_all("// edit B\n", window, cx));
            app.select_project(&a, window, cx);
            assert_eq!(
                app.project.documents[&a.join("main.rs")].editor,
                original_editor
            );
            assert!(app.project.expanded.contains(&a.join("src")));
            assert!(app.project.documents[&a.join("main.rs")].dirty);
            app.request(Next::Open(a.join(".")), window, cx);
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            assert_eq!(
                app.project_order.len(),
                2,
                "canonical paths must not create duplicate projects"
            );
            app.request(Next::Quit, window, cx);
        });
        assert!(cx.has_pending_prompt());
        assert!(cx.pending_prompt().unwrap().0.contains('2'));
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        std::fs::write(b.join("main.rs"), "// external edit\n").unwrap();
        view.update_in(cx, |app, window, cx| {
            app.save_documents(Some(Next::Quit), window, cx)
        });
        cx.run_until_parked();
        assert_eq!(
            std::fs::read_to_string(a.join("main.rs")).unwrap(),
            "// edit A\n"
        );
        assert_eq!(
            std::fs::read_to_string(b.join("main.rs")).unwrap(),
            "// external edit\n"
        );
        view.update_in(cx, |app, window, cx| {
            assert!(app.parked[0].documents[&b.join("main.rs")].dirty);
            assert!(app.message.is_some(), "a failed save must prevent quitting");
            app.request(Next::Close, window, cx);
            assert_eq!(app.project.workspace.as_ref().unwrap().root, b);
            assert_eq!(app.project_order, vec![b.clone()]);
            app.request(Next::Close, window, cx);
        });
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        view.update_in(cx, |app, _, _| {
            assert!(app.project.documents[&b.join("main.rs")].dirty)
        });
    }

    #[gpui::test]
    fn system_appearance_updates_chrome_and_editor(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let window = cx.add_window(|window, cx| {
            let mut app = Folio::new(window, cx);
            app.recent_task = None;
            app.settings_file = std::env::temp_dir().join("folio-test-settings.json");
            app.window_file = std::env::temp_dir().join("folio-test-window.json");
            app.settings = Settings::default();
            app.applied_ignored = app.settings.ignored.clone();
            app
        });
        for appearance in [
            WindowAppearance::Dark,
            WindowAppearance::Light,
            WindowAppearance::Dark,
        ] {
            window
                .update(cx, |app, window, cx| {
                    sync_appearance(appearance, &app.settings, Some(window), cx);
                    let theme = cx.theme();
                    assert_eq!(theme.is_dark(), appearance == WindowAppearance::Dark);
                    assert_eq!(theme.title_bar, theme.background);
                    assert_eq!(
                        theme.highlight_theme.style.editor_background,
                        Some(theme.background)
                    );
                    assert_eq!(
                        theme.highlight_theme.style.editor_foreground,
                        Some(theme.foreground)
                    );
                    assert_ne!(theme.foreground, theme.background);
                })
                .unwrap();
        }
        let options = TitleBar::window_options();
        assert!(options.app_owns_titlebar_drag);
        let titlebar = options.titlebar.unwrap();
        assert!(titlebar.appears_transparent);
        assert!(titlebar.title.is_none());
    }

    #[gpui::test]
    fn asynchronous_navigation_preserves_latest_intent(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let dir = root.join("src");
        std::fs::create_dir(&dir).unwrap();
        let file = dir.join("main.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        let config = root.join("recent.json");
        cx.update(gpui_component::init);
        // Exercise the real entities and executor without native input rendering.
        let cx = cx.add_empty_window();
        let view = cx.update(|window, cx| {
            cx.new(|cx| {
                let mut app = Folio::new(window, cx);
                // Cancel startup loading before the test executor runs; use isolated settings.
                app.recent_task = None;
                app.recent_file = config.clone();
                app.settings_file = root.join("settings.json");
                app.window_file = root.join("window.json");
                app.settings = Settings::default();
                app.applied_ignored = app.settings.ignored.clone();
                app
            })
        });
        view.update_in(cx, |app, _, cx| {
            app.project.workspace = Some(Workspace::open(&root).unwrap());
            let ignored = app.settings.ignored.clone();
            app.project
                .directories
                .insert(root.clone(), tree::children(&root, &ignored).unwrap());
            app.toggle_directory(dir.clone(), cx);
            app.toggle_directory(dir.clone(), cx);
            app.refresh_recent(RecentAction::Open(root.clone()), cx);
            app.refresh_recent(RecentAction::Open(dir.clone()), cx);
            app.refresh_recent(RecentAction::Remove(root.clone()), cx);
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            assert!(
                !app.project.expanded.contains(&dir),
                "late directory reads must not expand a collapsed row"
            );
            assert_eq!(app.project.directories[&dir].len(), 1);
            app.toggle_directory(root.clone(), cx);
            assert_eq!(app.project.rows.len(), 1);
            app.toggle_directory(dir.clone(), cx);
            assert_eq!(app.project.rows.len(), 2);
            app.toggle_directory(root.clone(), cx);
            assert!(
                app.project.rows.is_empty(),
                "collapsing the root hides all descendants"
            );
            assert!(
                app.project.expanded.contains(&dir),
                "child expansion state is retained"
            );
            app.toggle_directory(root.clone(), cx);
            assert_eq!(
                app.project.rows.len(),
                2,
                "expanding the root restores its descendants"
            );
            assert_eq!(app.recent.len(), 1);
            assert_eq!(app.recent[0].path, dir);
            assert_eq!(recent::load(&config).unwrap()[0].path, dir);
            app.open_file(file.clone(), window, cx);
            app.request(Next::Open(dir.clone()), window, cx);
            app.open_file(file.clone(), window, cx);
            assert!(app.project_loading);
            assert!(
                app.loading,
                "file navigation must not clear project loading"
            );
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            assert_eq!(app.project.workspace.as_ref().unwrap().root, dir);
            assert!(
                app.project.documents.is_empty(),
                "old file results must not enter the new workspace"
            );
            assert!(!app.loading);
            app.request(Next::Picker, window, cx);
            app.request(Next::Close, window, cx);
            assert!(app.prompting);
            assert!(
                app.project.workspace.is_some(),
                "project actions must wait for the picker"
            );
        });
        cx.simulate_path_prompt_response(|_| None);
        cx.run_until_parked();
        view.update_in(cx, |app, _, _| assert!(!app.prompting));
        view.update_in(cx, |app, window, cx| {
            app.open_file(file.clone(), window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            let doc = app.project.documents.get_mut(&file).unwrap();
            doc.editor
                .read(cx)
                .base_state()
                .clone()
                .update(cx, |base, cx| {
                    base.replace_all("// first edit\n", window, cx);
                });
        });
        view.update_in(cx, |app, window, cx| {
            assert!(app.project.documents[&file].dirty);
            app.save_documents(Some(Next::Close), window, cx);
            // An edit already queued before read-only rendering must survive the save result.
            app.project.documents[&file]
                .editor
                .read(cx)
                .base_state()
                .clone()
                .update(cx, |base, cx| {
                    base.replace_all("// newer edit\n", window, cx);
                });
        });
        cx.run_until_parked();
        assert!(cx.has_pending_prompt());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "// first edit\n");
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        view.update_in(cx, |app, _, cx| {
            assert!(app.project.documents[&file].dirty);
            assert_eq!(
                app.project.documents[&file]
                    .editor
                    .read(cx)
                    .value()
                    .as_ref(),
                "// newer edit\n"
            );
        });
        let picture = dir.join("preview.png");
        image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 255]))
            .save(&picture)
            .unwrap();
        view.update_in(cx, |app, window, cx| {
            app.open_file(picture.clone(), window, cx);
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            assert_eq!(app.project.active.as_ref(), Some(&picture));
            assert!(app.project.image.is_some());
            assert!(!app.project.documents.contains_key(&picture));
            assert!(app.project.documents[&file].dirty);
            app.save_documents(None, window, cx);
        });
        cx.run_until_parked();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "// first edit\n");
        view.update_in(cx, |app, window, cx| {
            app.open_file(file.clone(), window, cx);
            assert_eq!(app.project.active.as_ref(), Some(&file));
            assert!(app.project.documents[&file].dirty);
            assert_eq!(
                app.project.documents[&file]
                    .editor
                    .read(cx)
                    .value()
                    .as_ref(),
                "// newer edit\n"
            );
        });
    }

    #[gpui::test]
    fn project_search_reads_dirty_buffers_and_replace_splits_open_and_closed_files(
        cx: &mut TestAppContext,
    ) {
        // The scan is debounced, so a test has to move its clock past the delay.
        fn settle(cx: &VisualTestContext) {
            cx.executor().advance_clock(SEARCH_DEBOUNCE * 2);
            cx.run_until_parked();
        }

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let open = root.join("open.rs");
        let closed = root.join("closed.rs");
        std::fs::write(&open, "let value = 1;\nlet other = 2;\n").unwrap();
        std::fs::write(&closed, "let value = 3;\n").unwrap();
        std::fs::write(root.join("notes.md"), "value in prose\n").unwrap();
        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let view = cx.update(|window, cx| {
            cx.new(|cx| {
                let mut app = Folio::new(window, cx);
                app.recent_task = None;
                app.recent_file = root.join("recent.json");
                // Never read or write the real settings file from a test.
                app.settings_file = root.join("settings.json");
                app.window_file = root.join("window.json");
                app.settings = Settings::default();
                app.applied_ignored = app.settings.ignored.clone();
                app
            })
        });
        view.update_in(cx, |app, window, cx| {
            app.request(Next::Open(root.clone()), window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            app.open_file(open.clone(), window, cx)
        });
        cx.run_until_parked();

        // Edit the buffer without saving: the search must see what is on screen,
        // not what is on disk, or every reported position would be wrong.
        view.update_in(cx, |app, window, cx| {
            app.project.documents[&open]
                .editor
                .read(cx)
                .base_state()
                .clone()
                .update(cx, |base, cx| {
                    base.replace_all("let value = 1;\nlet added = 5;\n", window, cx)
                });
            app.show_search(true, window, cx);
            app.search_query
                .update(cx, |query, cx| query.set_value("added", window, cx));
            app.start_search(cx);
        });
        settle(cx);
        view.update_in(cx, |app, _, _| {
            assert!(!app.search.running);
            assert!(app.search.error.is_none());
            assert_eq!(app.total_hits(), 1, "the unsaved line must be searchable");
            assert_eq!(app.search.results[0].path, open);
            assert_eq!(app.search.results[0].hits[0].line, 1);
        });

        // Case sensitivity is a filter over the same code path.
        view.update_in(cx, |app, window, cx| {
            app.search_query
                .update(cx, |query, cx| query.set_value("Value", window, cx));
            app.search.options.case_sensitive = true;
            app.start_search(cx);
        });
        settle(cx);
        view.update_in(cx, |app, _, _| assert_eq!(app.total_hits(), 0));
        view.update_in(cx, |app, window, cx| {
            app.search.options.case_sensitive = false;
            app.search_query
                .update(cx, |query, cx| query.set_value("value", window, cx));
            app.start_search(cx);
        });
        settle(cx);
        view.update_in(cx, |app, _, _| {
            assert_eq!(app.total_hits(), 3, "open.rs, closed.rs and notes.md");
            assert_eq!(app.search.results.len(), 3);
        });

        // Opening a hit leaves the panel up so the next match is one key away.
        view.update_in(cx, |app, window, cx| {
            let file = app
                .search
                .results
                .iter()
                .position(|file| file.path == open)
                .unwrap();
            let row = app
                .search
                .rows
                .iter()
                .position(
                    |row| matches!(row, SearchRow::Hit { file: f, hit } if *f == file && *hit == 0),
                )
                .unwrap();
            app.search.selected = row;
            app.open_selected_hit(window, cx);
        });
        cx.run_until_parked();
        view.update_in(cx, |app, _, cx| {
            assert!(app.goto.is_none(), "the pending jump must be consumed");
            assert_eq!(app.panel, Some(Panel::Search));
            let cursor = app.project.documents[&open]
                .editor
                .read(cx)
                .base_state()
                .read(cx)
                .cursor_position();
            assert_eq!((cursor.line, cursor.character), (0, 4));
        });

        // Replace all: the open buffer is edited in memory and left dirty while
        // the files without an editor are rewritten on disk.
        view.update_in(cx, |app, window, cx| {
            app.show_search(true, window, cx);
            app.search_query
                .update(cx, |query, cx| query.set_value("value", window, cx));
            app.replace_query
                .update(cx, |query, cx| query.set_value("const value", window, cx));
            app.start_search(cx);
        });
        settle(cx);
        view.update_in(cx, |app, window, cx| {
            assert_eq!(app.total_hits(), 3);
            app.replace_project(window, cx);
        });
        cx.run_until_parked();
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Replace All");
        cx.run_until_parked();
        assert_eq!(
            std::fs::read_to_string(&closed).unwrap(),
            "let const value = 3;\n",
            "a closed file is rewritten in place"
        );
        assert_eq!(
            std::fs::read_to_string(&open).unwrap(),
            "let value = 1;\nlet other = 2;\n",
            "an open buffer must not be written behind the user's back"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("notes.md")).unwrap(),
            "const value in prose\n"
        );
        view.update_in(cx, |app, _, cx| {
            let document = &app.project.documents[&open];
            assert!(document.dirty);
            assert_eq!(
                document.editor.read(cx).value().as_ref(),
                "let const value = 1;\nlet added = 5;\n"
            );
        });
        settle(cx);
        view.update_in(cx, |app, _, _| {
            assert_eq!(app.total_hits(), 3, "the search re-runs after a replace");
            assert_eq!(app.search.results.len(), 3);
        });
    }

    /// The tree context menu end to end: the row it splices in, the entry it
    /// writes, and the clipboard it moves entries with. Reveal / Open in
    /// Default App / Open in Terminal are left alone — they hand the path to
    /// the OS, which during a test run would open Finder and a terminal.
    /// The saved geometry: what round-trips, what an older file still loads as,
    /// and what is too far out of range to restore.
    #[gpui::test]
    fn window_geometry_round_trips(_cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("config/window.json");

        // Nothing saved yet is not an error.
        assert!(WindowState::load(&file).main.is_none());

        WindowState {
            main: Some([10., 20., 1200., 800.]),
            settings: Some([30., 40., 900., 660.]),
        }
        .save(&file);
        let loaded = WindowState::load(&file);
        assert_eq!(loaded.main, Some([10., 20., 1200., 800.]));
        assert_eq!(loaded.settings, Some([30., 40., 900., 660.]));

        // The bare array older versions wrote still loads, as the main window.
        std::fs::write(&file, "[1.0, 2.0, 1000.0, 700.0]").unwrap();
        let legacy = WindowState::load(&file);
        assert_eq!(legacy.main, Some([1., 2., 1000., 700.]));
        assert!(legacy.settings.is_none());

        // A rectangle too small for the window, or not finite, is not restored.
        let minimum = size(px(640.), px(480.));
        assert!(restore_bounds([0., 0., 320., 200.], minimum).is_none());
        assert!(restore_bounds([0., 0., f32::NAN, 800.], minimum).is_none());
        assert!(restore_bounds([0., 0., 1200., 800.], minimum).is_some());

        // A file that is not JSON at all counts as nothing saved.
        std::fs::write(&file, "not json").unwrap();
        assert!(WindowState::load(&file).main.is_none());
    }

    /// Both windows are recorded when the app writes its geometry, and it goes
    /// to the file the app was pointed at rather than the real one.
    #[gpui::test]
    fn both_windows_geometry_is_recorded(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let file = root.join("window.json");
        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let view = cx.update(|window, cx| {
            cx.new(|cx| {
                let mut app = Folio::new(window, cx);
                app.recent_task = None;
                app.recent_file = root.join("recent.json");
                app.settings_file = root.join("settings.json");
                app.window_file = file.clone();
                app.settings = Settings::default();
                app.applied_ignored = app.settings.ignored.clone();
                app
            })
        });
        view.update_in(cx, |app, _, _| {
            app.main_bounds = Bounds::new(point(px(10.), px(20.)), size(px(1000.), px(700.)));
            app.remember_settings_bounds(Bounds::new(
                point(px(30.), px(40.)),
                size(px(900.), px(660.)),
            ));
        });
        let state = WindowState::load(&file);
        assert_eq!(state.main, Some([10., 20., 1000., 700.]));
        assert_eq!(state.settings, Some([30., 40., 900., 660.]));
    }

    /// A dragged tab lands where the tab it was dropped on sits, and that
    /// works the same in either direction — the leftward one-slot drag is the
    /// case a pointer-following rule could not be relied on for.
    #[gpui::test]
    fn dragging_a_tab_reorders_the_strip(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let first = root.join("first.rs");
        let second = root.join("second.rs");
        let third = root.join("third.rs");
        for file in [&first, &second, &third] {
            std::fs::write(file, "// code\n").unwrap();
        }
        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let view = cx.update(|window, cx| {
            cx.new(|cx| {
                let mut app = Folio::new(window, cx);
                app.recent_task = None;
                app.recent_file = root.join("recent.json");
                app.settings_file = root.join("settings.json");
                app.window_file = root.join("window.json");
                app.settings = Settings::default();
                app.applied_ignored = app.settings.ignored.clone();
                app
            })
        });
        view.update_in(cx, |app, window, cx| {
            app.request(Next::Open(root.clone()), window, cx)
        });
        cx.run_until_parked();
        for file in [&first, &second, &third] {
            view.update_in(cx, |app, window, cx| {
                app.open_file(file.clone(), window, cx)
            });
            cx.run_until_parked();
        }

        // One slot to the right, onto the tab it should trade with.
        view.update_in(cx, |app, _, cx| app.move_tab(&first, 1, cx));
        view.update_in(cx, |app, _, _| {
            assert_eq!(
                app.project.tabs,
                vec![second.clone(), first.clone(), third.clone()]
            )
        });

        // And one slot back to the left, which is the drag that used to do
        // nothing.
        view.update_in(cx, |app, _, cx| app.move_tab(&first, 0, cx));
        view.update_in(cx, |app, _, _| {
            assert_eq!(
                app.project.tabs,
                vec![first.clone(), second.clone(), third.clone()]
            )
        });

        // Right across the strip, and back to the front.
        view.update_in(cx, |app, _, cx| app.move_tab(&first, 2, cx));
        view.update_in(cx, |app, _, _| {
            assert_eq!(
                app.project.tabs,
                vec![second.clone(), third.clone(), first.clone()]
            )
        });
        view.update_in(cx, |app, _, cx| app.move_tab(&first, 0, cx));
        view.update_in(cx, |app, _, _| {
            assert_eq!(
                app.project.tabs,
                vec![first.clone(), second.clone(), third.clone()]
            )
        });

        // Past the end lands at the end; dropping on itself changes nothing.
        view.update_in(cx, |app, _, cx| app.move_tab(&first, 99, cx));
        view.update_in(cx, |app, _, _| {
            assert_eq!(
                app.project.tabs,
                vec![second.clone(), third.clone(), first.clone()]
            )
        });
        view.update_in(cx, |app, _, cx| app.move_tab(&first, 2, cx));
        view.update_in(cx, |app, _, _| {
            assert_eq!(
                app.project.tabs,
                vec![second.clone(), third.clone(), first.clone()]
            )
        });

        // A pinned tab stays in front whatever the drop says, and an unpinned
        // one cannot be dropped ahead of it.
        view.update_in(cx, |app, _, cx| {
            app.project.pinned.insert(second.clone());
            app.reorder_tabs();
            app.move_tab(&second, 2, cx);
        });
        view.update_in(cx, |app, _, _| {
            assert_eq!(
                app.project.tabs.first(),
                Some(&second),
                "a pinned tab cannot be dragged out of the front"
            );
        });
        view.update_in(cx, |app, _, cx| app.move_tab(&first, 0, cx));
        view.update_in(cx, |app, _, _| {
            assert_eq!(
                app.project.tabs,
                vec![second.clone(), first.clone(), third.clone()],
                "an unpinned tab cannot be dropped in front of a pinned one"
            );
        });

        // The caret draws between tabs while a drag is in flight.
        view.update_in(cx, |app, window, cx| {
            app.tab_drop = Some(1);
            let _ = app.render(window, cx);
        });

        // A path that is not in the strip changes nothing.
        view.update_in(cx, |app, _, cx| app.move_tab(&root.join("ghost.rs"), 0, cx));
        view.update_in(cx, |app, _, _| {
            assert_eq!(
                app.project.tabs,
                vec![second.clone(), first.clone(), third.clone()]
            )
        });
    }

    /// The tab strip's menu: the closes act on the tab that was clicked, and
    /// the bulk ones leave pinned tabs alone.
    #[gpui::test]
    fn the_tab_menu_closes_pins_and_locks(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let first = root.join("first.rs");
        let second = root.join("second.rs");
        let third = root.join("third.rs");
        for file in [&first, &second, &third] {
            std::fs::write(file, "// code\n").unwrap();
        }
        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let view = cx.update(|window, cx| {
            cx.new(|cx| {
                let mut app = Folio::new(window, cx);
                app.recent_task = None;
                app.recent_file = root.join("recent.json");
                app.settings_file = root.join("settings.json");
                app.window_file = root.join("window.json");
                app.settings = Settings::default();
                app.applied_ignored = app.settings.ignored.clone();
                app
            })
        });
        view.update_in(cx, |app, window, cx| {
            app.request(Next::Open(root.clone()), window, cx)
        });
        cx.run_until_parked();
        for file in [&first, &second, &third] {
            view.update_in(cx, |app, window, cx| {
                app.open_file(file.clone(), window, cx)
            });
            cx.run_until_parked();
        }

        // The strip offers its own table, and closing leftwards off the first
        // tab is one of the things that has nothing to do.
        view.update_in(cx, |app, window, cx| {
            app.open_menu(
                MenuTarget::Tab {
                    path: first.clone(),
                    index: 0,
                },
                point(px(60.), px(40.)),
                window,
                cx,
            );
            let target = app.menu.as_ref().unwrap().target.clone();
            assert_eq!(visible_menu_items(&target).len(), TAB_MENU.len());
            assert!(!app.menu_item_enabled(MenuItem::CloseLeft, &first));
            assert!(app.menu_item_enabled(MenuItem::CloseRight, &first));
            // The strip draws under it.
            let _ = app.render(window, cx);
            app.run_menu_item(MenuItem::CloseRight, window, cx);
        });
        cx.run_until_parked();
        view.update_in(cx, |app, _, _| {
            assert_eq!(app.project.tabs, vec![first.clone()]);
        });

        // Close Others keeps the tab the menu was opened on, and takes the
        // rest — including the one that was showing.
        view.update_in(cx, |app, window, cx| {
            app.open_file(second.clone(), window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            app.open_menu(
                MenuTarget::Tab {
                    path: first.clone(),
                    index: 0,
                },
                point(px(60.), px(40.)),
                window,
                cx,
            );
            app.run_menu_item(MenuItem::CloseOthers, window, cx);
        });
        cx.run_until_parked();
        view.update_in(cx, |app, _, _| {
            assert_eq!(app.project.tabs, vec![first.clone()]);
            assert_eq!(app.project.active.as_ref(), Some(&first));
        });

        // Pinning moves the tab to the front and takes it out of the bulk
        // closes' reach.
        view.update_in(cx, |app, window, cx| {
            app.open_file(third.clone(), window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            app.open_menu(
                MenuTarget::Tab {
                    path: third.clone(),
                    index: 1,
                },
                point(px(60.), px(40.)),
                window,
                cx,
            );
            app.run_menu_item(MenuItem::PinTab, window, cx);
            assert!(app.is_pinned(&third));
            assert_eq!(app.project.tabs.first(), Some(&third));
            // And the entry says what it will do next time.
            assert_eq!(
                app.menu_label(MenuItem::PinTab, &third, "Pin Tab").as_ref(),
                "Unpin Tab"
            );
        });
        view.update_in(cx, |app, window, cx| {
            app.open_menu(
                MenuTarget::Tab {
                    path: first.clone(),
                    index: 1,
                },
                point(px(60.), px(40.)),
                window,
                cx,
            );
            assert!(!app.menu_item_enabled(MenuItem::CloseOthers, &first));
            app.run_menu_item(MenuItem::CloseOthers, window, cx);
        });
        cx.run_until_parked();
        view.update_in(cx, |app, _, _| {
            assert_eq!(app.project.tabs, vec![third.clone(), first.clone()]);
        });

        // Locking a tab is view state: it flips the label and refuses edits,
        // and nothing about it reaches the file.
        view.update_in(cx, |app, window, cx| {
            app.open_menu(
                MenuTarget::Tab {
                    path: first.clone(),
                    index: 1,
                },
                point(px(60.), px(40.)),
                window,
                cx,
            );
            app.run_menu_item(MenuItem::ToggleReadOnly, window, cx);
            assert!(app.is_read_only(&first));
            assert_eq!(
                app.menu_label(MenuItem::ToggleReadOnly, &first, "Make Tab Read-Only")
                    .as_ref(),
                "Make Tab Writable"
            );
            let _ = app.render(window, cx);
        });

        // Copying a relative path puts it on the clipboard, without the root.
        view.update_in(cx, |app, window, cx| {
            app.open_menu(
                MenuTarget::Tab {
                    path: first.clone(),
                    index: 1,
                },
                point(px(60.), px(40.)),
                window,
                cx,
            );
            app.run_menu_item(MenuItem::CopyRelativePath, window, cx);
        });
        assert_eq!(
            cx.read_from_clipboard()
                .and_then(|item| item.text())
                .as_deref(),
            Some("first.rs")
        );

        // Revealing opens the folders above the file — its row does not exist
        // until they are — and puts the tree's cursor on it.
        let nested = root.join("nested");
        let deep = nested.join("deep.rs");
        std::fs::create_dir(&nested).unwrap();
        std::fs::write(&deep, "// deep\n").unwrap();
        view.update_in(cx, |app, window, cx| {
            app.open_file(deep.clone(), window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            assert!(!app.project.expanded.contains(&nested));
            app.open_menu(
                MenuTarget::Tab {
                    path: deep.clone(),
                    index: 2,
                },
                point(px(60.), px(40.)),
                window,
                cx,
            );
            app.run_menu_item(MenuItem::RevealInTree, window, cx);
        });
        cx.run_until_parked();
        view.update_in(cx, |app, _, _| {
            assert!(app.project.expanded.contains(&nested));
            let row = &app.project.rows[app.project.selected_row];
            assert_eq!(row.entry.path, deep, "the cursor is on the revealed file");
        });
    }

    /// Tabs: opening adds them, closing one releases its buffer and leaves a
    /// neighbour showing, and a dirty one is asked about first.
    #[gpui::test]
    fn tabs_open_close_and_guard_unsaved_changes(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let first = root.join("first.rs");
        let second = root.join("second.rs");
        std::fs::write(&first, "// one\n").unwrap();
        std::fs::write(&second, "// two\n").unwrap();
        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let view = cx.update(|window, cx| {
            cx.new(|cx| {
                let mut app = Folio::new(window, cx);
                app.recent_task = None;
                app.recent_file = root.join("recent.json");
                app.settings_file = root.join("settings.json");
                app.window_file = root.join("window.json");
                app.settings = Settings::default();
                app.applied_ignored = app.settings.ignored.clone();
                app
            })
        });
        view.update_in(cx, |app, window, cx| {
            app.request(Next::Open(root.clone()), window, cx)
        });
        cx.run_until_parked();

        view.update_in(cx, |app, window, cx| {
            app.open_file(first.clone(), window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |app, _, _| {
            assert_eq!(app.project.tabs, vec![first.clone()]);
            assert_eq!(app.project.active.as_ref(), Some(&first));
        });

        view.update_in(cx, |app, window, cx| {
            app.open_file(second.clone(), window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            assert_eq!(app.project.tabs, vec![first.clone(), second.clone()]);
            assert_eq!(app.project.active.as_ref(), Some(&second));
            // The strip draws; the harness never draws on its own.
            let _ = app.render(window, cx);
        });

        // Opening a file that already has a tab focuses it rather than adding
        // a second one.
        view.update_in(cx, |app, window, cx| {
            app.open_file(first.clone(), window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |app, _, _| {
            assert_eq!(app.project.tabs.len(), 2);
            assert_eq!(app.project.active.as_ref(), Some(&first));
        });

        // Closing the active tab releases its buffer and shows the neighbour.
        view.update_in(cx, |app, window, cx| {
            app.close_tabs(vec![first.clone()], window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |app, _, _| {
            assert_eq!(app.project.tabs, vec![second.clone()]);
            assert!(!app.project.documents.contains_key(&first));
            assert_eq!(app.project.active.as_ref(), Some(&second));
        });

        // An untouched tab closes without a word.
        view.update_in(cx, |app, window, cx| {
            app.request(Next::CloseTabs(vec![second.clone()]), window, cx)
        });
        cx.run_until_parked();
        assert!(!cx.has_pending_prompt());
        view.update_in(cx, |app, _, _| {
            assert!(app.project.tabs.is_empty());
            assert!(app.project.active.is_none());
            assert!(app.project.documents.is_empty());
        });

        // A dirty tab is asked about, and cancelling leaves it alone.
        view.update_in(cx, |app, window, cx| {
            app.open_file(second.clone(), window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            app.project.documents.get_mut(&second).unwrap().dirty = true;
            app.request(Next::CloseTabs(vec![second.clone()]), window, cx);
            assert!(app.prompting);
        });
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        view.update_in(cx, |app, _, _| {
            assert_eq!(app.project.tabs, vec![second.clone()]);
            assert!(app.project.documents.contains_key(&second));
        });

        // Discarding closes it, and putting the tab back works the same way.
        view.update_in(cx, |app, window, cx| {
            app.request(Next::CloseTabs(vec![second.clone()]), window, cx)
        });
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Don't Save");
        cx.run_until_parked();
        view.update_in(cx, |app, _, _| {
            assert!(app.project.tabs.is_empty());
            assert!(app.project.documents.is_empty());
        });
    }

    /// Left and right in the panel switch the mode tabs from the caret's
    /// edges, and keep their ordinary meaning everywhere else.
    #[gpui::test]
    fn panel_arrows_switch_the_mode_tabs(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::write(root.join("main.rs"), "// original\n").unwrap();
        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let view = cx.update(|window, cx| {
            cx.new(|cx| {
                let mut app = Folio::new(window, cx);
                app.recent_task = None;
                app.recent_file = root.join("recent.json");
                app.settings_file = root.join("settings.json");
                app.window_file = root.join("window.json");
                app.settings = Settings::default();
                app.applied_ignored = app.settings.ignored.clone();
                app
            })
        });
        view.update_in(cx, |app, window, cx| {
            app.request(Next::Open(root.clone()), window, cx)
        });
        cx.run_until_parked();

        // The query is focused and empty when the panel opens, so the caret
        // sits on both edges at once: right takes Files to Contents, and
        // left, which would leave the first tab, does not move.
        view.update_in(cx, |app, window, cx| {
            app.show_quick_open(false, window, cx);
            assert_eq!(app.panel, Some(Panel::Files));
            assert!(
                !app.panel_arrow("left", window, cx),
                "left at the first tab stays"
            );
            assert_eq!(app.panel, Some(Panel::Files));
            assert!(
                app.panel_arrow("right", window, cx),
                "right at the edge takes the tab to Contents"
            );
            assert_eq!(app.panel, Some(Panel::Search));
            assert!(
                !app.panel_arrow("right", window, cx),
                "right at the last tab stays"
            );
            assert!(
                app.panel_arrow("left", window, cx),
                "left at the edge takes the tab back to Files"
            );
            assert_eq!(app.panel, Some(Panel::Files));
        });

        // A caret inside the text keeps the key, and so does a selection.
        view.update_in(cx, |app, window, cx| {
            app.show_quick_open(false, window, cx);
            let query = app.query.clone();
            query.update(cx, |query, cx| {
                query.set_value("ab", window, cx);
                query
                    .base_state()
                    .update(cx, |base, cx| base.set_selected_range(1..1, cx));
            });
            assert!(
                !app.panel_arrow("right", window, cx),
                "a caret inside the text keeps right"
            );
            assert_eq!(app.panel, Some(Panel::Files));
            query.update(cx, |query, cx| query.select_all(window, cx));
            assert!(
                !app.panel_arrow("right", window, cx),
                "a selection keeps the key"
            );
            assert_eq!(app.panel, Some(Panel::Files));
            query.update(cx, |query, cx| {
                query
                    .base_state()
                    .update(cx, |base, cx| base.set_selected_range(2..2, cx));
            });
            assert!(
                app.panel_arrow("right", window, cx),
                "the caret at the end spends the key on the tab"
            );
            assert_eq!(app.panel, Some(Panel::Search));
        });

        // A focus outside the panel inputs keeps the key too: the editor
        // behind the overlay must not spend its arrows on the tabs.
        view.update_in(cx, |app, window, cx| {
            app.open_file(root.join("main.rs"), window, cx);
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            app.show_search(false, window, cx);
            app.project.documents[&root.join("main.rs")]
                .editor
                .focus_handle(cx)
                .focus(window, cx);
            assert!(
                !app.panel_arrow("left", window, cx),
                "a focus outside the panel keeps left"
            );
            assert_eq!(app.panel, Some(Panel::Search));
            app.search_query
                .update(cx, |query, cx| query.focus(window, cx));
            assert!(
                app.panel_arrow("left", window, cx),
                "the query's empty text puts the caret on both edges again"
            );
            assert_eq!(app.panel, Some(Panel::Files));
        });
        cx.run_until_parked();
    }

    /// The terminal drawer: tabs come and go per project, the children run
    /// while the drawer is hidden, the last close takes the drawer, and
    /// closing the project takes everything.
    #[gpui::test]
    fn the_terminal_dock_follows_the_project(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::write(root.join("main.rs"), "// original\n").unwrap();
        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let view = cx.update(|window, cx| {
            cx.new(|cx| {
                let mut app = Folio::new(window, cx);
                app.recent_task = None;
                app.recent_file = root.join("recent.json");
                app.settings_file = root.join("settings.json");
                app.window_file = root.join("window.json");
                app.settings = Settings::default();
                app.applied_ignored = app.settings.ignored.clone();
                app
            })
        });
        view.update_in(cx, |app, window, cx| {
            app.request(Next::Open(root.clone()), window, cx)
        });
        cx.run_until_parked();

        // Opening the drawer spawns the project's terminal and focuses it.
        view.update_in(cx, |app, window, cx| {
            app.toggle_terminal(window, cx);
            assert!(app.terminal_open);
            let tabs = app.terminals.get(&root).expect("a terminal was spawned");
            assert_eq!(tabs.views.len(), 1);
            assert_eq!(tabs.active, 0);
        });
        cx.run_until_parked();

        // Hiding the drawer keeps the children running.
        view.update_in(cx, |app, window, cx| {
            app.toggle_terminal(window, cx);
            assert!(!app.terminal_open);
            assert_eq!(app.terminals[&root].views.len(), 1);
        });

        // Reopening reuses it rather than spawning a second one.
        view.update_in(cx, |app, window, cx| {
            app.toggle_terminal(window, cx);
            assert_eq!(app.terminals[&root].views.len(), 1);
        });
        cx.run_until_parked();

        // A new tab goes to the front of the drawer and takes the focus.
        view.update_in(cx, |app, window, cx| {
            app.new_terminal(window, cx);
            let tabs = app.terminals.get(&root).unwrap();
            assert_eq!(tabs.views.len(), 2);
            assert_eq!(tabs.active, 1);
        });
        cx.run_until_parked();

        // Closing the active tab hands the drawer back to the first one.
        view.update_in(cx, |app, window, cx| {
            app.close_terminal_tab(1, window, cx);
            let tabs = app.terminals.get(&root).unwrap();
            assert_eq!(tabs.views.len(), 1);
            assert_eq!(tabs.active, 0);
            assert!(app.terminal_open);
        });
        cx.run_until_parked();

        // Closing the last tab closes the drawer with it.
        view.update_in(cx, |app, window, cx| {
            app.close_terminal_tab(0, window, cx);
            assert!(!app.terminals.contains_key(&root));
            assert!(!app.terminal_open);
        });
        cx.run_until_parked();

        // Closing the project releases any terminal it still had.
        view.update_in(cx, |app, window, cx| {
            app.toggle_terminal(window, cx);
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| app.request(Next::Close, window, cx));
        cx.run_until_parked();
        view.update_in(cx, |app, _, _| {
            assert!(app.terminals.is_empty());
        });
    }

    /// The icon rail and the file tree are two toggles. The title-bar
    /// button owns the rail; `⌘B` and the first rail icon own the tree.
    #[gpui::test]
    fn the_activity_bar_and_the_file_tree_are_separate(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::write(root.join("main.rs"), "// original\n").unwrap();
        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let view = cx.update(|window, cx| {
            cx.new(|cx| {
                let mut app = Folio::new(window, cx);
                app.recent_task = None;
                app.recent_file = root.join("recent.json");
                app.settings_file = root.join("settings.json");
                app.window_file = root.join("window.json");
                app.settings = Settings::default();
                app.applied_ignored = app.settings.ignored.clone();
                app
            })
        });
        view.update_in(cx, |app, window, cx| {
            app.request(Next::Open(root.clone()), window, cx)
        });
        cx.run_until_parked();

        view.update(cx, |app, _| {
            assert!(app.activity_bar);
            assert!(app.sidebar);
        });

        view.update(cx, |app, cx| {
            app.toggle_activity_bar(cx);
            assert!(!app.activity_bar);
            assert!(app.sidebar, "hiding the rail leaves the tree");
        });

        view.update(cx, |app, cx| {
            app.toggle_file_tree(cx);
            assert!(!app.sidebar);
            assert!(!app.activity_bar, "hiding the tree leaves the rail");
        });

        view.update(cx, |app, cx| {
            app.toggle_activity_bar(cx);
            app.toggle_file_tree(cx);
            assert!(app.activity_bar);
            assert!(app.sidebar);
        });
    }

    /// The changes view: `⇧⌘D` shows the active file's diff against HEAD,
    /// follows the file it is pointed at, and goes away again.
    #[gpui::test]
    fn the_changes_view_shows_the_active_files_diff(cx: &mut TestAppContext) {
        use std::process::Command;

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let git = |args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .unwrap()
        };
        assert!(git(&["init", "-q"]).status.success());
        let file = root.join("main.rs");
        let other = root.join("other.rs");
        std::fs::write(&file, "let one = 1;\nlet two = 2;\n").unwrap();
        std::fs::write(&other, "// untouched\n").unwrap();
        assert!(git(&["add", "."]).status.success());
        assert!(
            git(&[
                "-c",
                "user.name=Folio Test",
                "-c",
                "user.email=test@localhost",
                "commit",
                "-qm",
                "fixture"
            ])
            .status
            .success()
        );
        // One line changed after the commit, which is what the view shows.
        std::fs::write(&file, "let one = 1;\nlet two = 22;\n").unwrap();

        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let view = cx.update(|window, cx| {
            cx.new(|cx| {
                let mut app = Folio::new(window, cx);
                app.recent_task = None;
                app.recent_file = root.join("recent.json");
                app.settings_file = root.join("settings.json");
                app.window_file = root.join("window.json");
                app.settings = Settings::default();
                app.applied_ignored = app.settings.ignored.clone();
                app
            })
        });
        view.update_in(cx, |app, window, cx| {
            app.request(Next::Open(root.clone()), window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            app.open_file(file.clone(), window, cx)
        });
        cx.run_until_parked();

        view.update_in(cx, |app, window, cx| app.toggle_diff(window, cx));
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            let shown = app.project.diff.as_ref().expect("the changes view is on");
            assert_eq!(shown.path, file);
            assert!(!shown.loading);
            assert!(shown.error.is_none());
            assert!(shown.lines.iter().any(|line| {
                line.change == diff::Change::Added && line.text == "let two = 22;"
            }));
            assert!(shown.lines.iter().any(|line| {
                line.change == diff::Change::Removed && line.text == "let two = 2;"
            }));
            // The editor area draws the list instead of the editor.
            let _ = app.render(window, cx);
        });

        // Opening another file moves the view onto that file's changes.
        view.update_in(cx, |app, window, cx| {
            app.open_file(other.clone(), window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |app, _, _| {
            let shown = app.project.diff.as_ref().expect("still on");
            assert_eq!(shown.path, other);
            assert!(!shown.loading);
            assert!(shown.lines.is_empty(), "the second file has no changes");
        });

        // Right-clicking the changes view offers the way back. It has to: the
        // editor's own menu cannot open while the editor is off screen, and
        // nothing else on this surface says how to leave.
        view.update_in(cx, |app, window, cx| {
            app.open_menu(
                MenuTarget::Changes {
                    path: other.clone(),
                },
                point(px(200.), px(200.)),
                window,
                cx,
            );
            let menu = app.menu.as_ref().expect("the menu is open");
            assert_eq!(
                menu.target,
                MenuTarget::Changes {
                    path: other.clone()
                }
            );
            assert_eq!(CHANGES_MENU[menu.selected].0, MenuItem::HideChanges);
            // The menu draws over the view.
            let _ = app.render(window, cx);
            app.run_menu_item(MenuItem::HideChanges, window, cx);
        });
        view.update_in(cx, |app, window, cx| {
            assert!(app.project.diff.is_none(), "the way back works");
            assert!(app.menu.is_none());
            // And the editor is what the area shows again.
            let _ = app.render(window, cx);
        });
    }

    /// The settings window is created and drawn from inside `Folio`'s own
    /// update — that is where the app menu dispatches `OpenSettings`, and
    /// where `open_window` builds the root. Rendering the form must not read
    /// the app view while that is in flight.
    #[gpui::test]
    fn the_settings_form_renders_from_inside_the_app_view_update(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let view = cx.update(|window, cx| {
            cx.new(|cx| {
                let mut app = Folio::new(window, cx);
                app.recent_task = None;
                app.recent_file = root.join("recent.json");
                app.settings_file = root.join("settings.json");
                app.window_file = root.join("window.json");
                app.settings = Settings::default();
                app.applied_ignored = app.settings.ignored.clone();
                app
            })
        });
        view.update_in(cx, |app, window, cx| {
            let folio = cx.entity().downgrade();
            let settings = cx.new(|cx| SettingsView::new(folio, app.settings.clone(), window, cx));
            settings.update(cx, |form, cx| {
                let _ = form.render(window, cx);
            });
        });
        // The form took the settings it was handed, without asking for them.
        view.update_in(cx, |app, _, _| {
            assert_eq!(app.settings, Settings::default())
        });
    }

    /// The two predicates the pairing rules are built on, which are the part
    /// worth pinning down: both come from Zed's editor, and both stop the
    /// shortcut from doing something the user did not ask for.
    #[test]
    fn pairing_knows_where_a_closer_fits() {
        // Nothing after the caret, whitespace, or a closer already there.
        assert!(allows_autoclose(None));
        assert!(allows_autoclose(Some(' ')));
        assert!(allows_autoclose(Some(')')));
        assert!(allows_autoclose(Some(']')));
        assert!(allows_autoclose(Some('}')));
        // Anything else is where the rest of a word has to go, and a pair
        // opened there would swallow it.
        assert!(!allows_autoclose(Some('f')));
        assert!(!allows_autoclose(Some('1')));
        assert!(!allows_autoclose(Some('_')));
        assert!(!allows_autoclose(Some('"')));

        // A quote after a word is an apostrophe or a lifetime, not an opening
        // quote.
        assert!(is_word_char(Some('n')));
        assert!(is_word_char(Some('9')));
        assert!(is_word_char(Some('_')));
        assert!(!is_word_char(Some(' ')));
        assert!(!is_word_char(Some('(')));
        assert!(!is_word_char(None));
    }

    /// An opening bracket makes a pair with the caret between the two, a
    /// closing one steps over a closer already there, and a selection is
    /// wrapped rather than replaced.
    #[gpui::test]
    fn brackets_pair_and_step_over(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let file = root.join("main.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let view = cx.update(|window, cx| {
            cx.new(|cx| {
                let mut app = Folio::new(window, cx);
                app.recent_task = None;
                app.recent_file = root.join("recent.json");
                app.settings_file = root.join("settings.json");
                app.window_file = root.join("window.json");
                app.settings = Settings::default();
                app.applied_ignored = app.settings.ignored.clone();
                app
            })
        });
        view.update_in(cx, |app, window, cx| {
            app.request(Next::Open(root.clone()), window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            app.open_file(file.clone(), window, cx)
        });
        cx.run_until_parked();

        view.update_in(cx, |app, window, cx| {
            let base = app.project.documents[&file]
                .editor
                .read(cx)
                .base_state()
                .clone();
            let value = |cx: &App| base.read(cx).value().to_string();
            let caret = |cx: &App| base.read(cx).selected_range();
            // Every case starts from a known buffer, so each one reads on its
            // own rather than off whatever the one before it left behind.
            // Takes the window rather than capturing it, so the calls below
            // can borrow it in turn.
            let start = |text: &str,
                         selection: std::ops::Range<usize>,
                         window: &mut Window,
                         cx: &mut Context<Folio>| {
                base.update(cx, |base, cx| {
                    base.set_value(text, window, cx);
                    base.set_selected_range(selection, cx);
                });
            };

            // An empty pair at the end of a line, with the caret between the
            // two characters.
            start("fn main() {}\n", 12..12, window, cx);
            app.open_pair("(", ")", window, cx);
            assert_eq!(value(cx), "fn main() {}()\n");
            assert_eq!(caret(cx), 13..13);

            // In front of a word it is typed alone: the closer would land
            // where the rest of the word has to go.
            start("fn main() {}\n", 0..0, window, cx);
            app.open_pair("(", ")", window, cx);
            assert_eq!(value(cx), "(fn main() {}\n");

            // A closing bracket steps over the one that is already there
            // instead of adding a second.
            start("()fn main() {}\n", 1..1, window, cx);
            app.skip_closer(")", window, cx);
            assert_eq!(value(cx), "()fn main() {}\n");
            assert_eq!(caret(cx), 2..2);

            // With nothing to step over it is just typed.
            start("()fn main() {}\n", 0..0, window, cx);
            app.skip_closer("}", window, cx);
            assert_eq!(value(cx), "}()fn main() {}\n");

            // A selection is wrapped, and the caret lands after the pair.
            start("fn main() {}\n", 0..2, window, cx);
            app.open_pair("(", ")", window, cx);
            assert_eq!(value(cx), "(fn) main() {}\n");
            assert_eq!(caret(cx), 4..4);

            // A quote makes a pair, and the next one steps over it.
            start(" \n", 1..1, window, cx);
            app.pair_quote("\"", window, cx);
            assert_eq!(value(cx), " \"\"\n");
            assert_eq!(caret(cx), 2..2);
            app.pair_quote("\"", window, cx);
            assert_eq!(value(cx), " \"\"\n");
            assert_eq!(caret(cx), 3..3);

            // A quote after a word is not opening a quote — an apostrophe
            // stays an apostrophe.
            start("don\n", 3..3, window, cx);
            app.pair_quote("'", window, cx);
            assert_eq!(value(cx), "don'\n");

            // Enter between a fresh pair lays it out over three lines, with
            // the caret on the line between and the closer at the opener's
            // indentation. One newline would leave the closer under the caret,
            // on the line the body was going to go on.
            start("    if x {}\n", 10..10, window, cx);
            base.update(cx, |base, cx| base.insert_line_break(window, cx));
            assert_eq!(value(cx), "    if x {\n        \n    }\n");
            assert_eq!(caret(cx), 19..19);

            // Anywhere else it is a line break and the indentation of the line
            // it breaks, which is what it has always been.
            start("    let x = 1;\n", 14..14, window, cx);
            base.update(cx, |base, cx| base.insert_line_break(window, cx));
            assert_eq!(value(cx), "    let x = 1;\n    \n");
            assert_eq!(caret(cx), 19..19);

            // Quotes are not brackets: a line break inside one is a string
            // being written over two lines, not a block with a body.
            start("let s = \"\";\n", 9..9, window, cx);
            base.update(cx, |base, cx| base.insert_line_break(window, cx));
            assert_eq!(value(cx), "let s = \"\n\";\n");
            assert_eq!(caret(cx), 10..10);
        });
    }

    /// More than one cursor: the commands that make a set of selections, the
    /// edit that lands at each of them, and the way back down to one.
    ///
    /// A keystroke never reaches the view in this harness, so what is driven
    /// here is what the keybinding's handler calls — one call per press — plus
    /// the editor's own entry points for typing, deleting and Enter, which are
    /// what the platform's input path calls.
    #[gpui::test]
    fn multi_cursor_edits_every_selection(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let file = root.join("main.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let view = cx.update(|window, cx| {
            cx.new(|cx| {
                let mut app = Folio::new(window, cx);
                app.recent_task = None;
                app.recent_file = root.join("recent.json");
                app.settings_file = root.join("settings.json");
                app.window_file = root.join("window.json");
                app.settings = Settings::default();
                app.applied_ignored = app.settings.ignored.clone();
                app
            })
        });
        view.update_in(cx, |app, window, cx| {
            app.request(Next::Open(root.clone()), window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            app.open_file(file.clone(), window, cx)
        });
        cx.run_until_parked();

        view.update_in(cx, |app, window, cx| {
            let base = app.project.documents[&file]
                .editor
                .read(cx)
                .base_state()
                .clone();
            let value = |cx: &App| base.read(cx).value().to_string();
            let ranges = |cx: &App| base.read(cx).selected_ranges();
            let start = |text: &str,
                         selection: std::ops::Range<usize>,
                         window: &mut Window,
                         cx: &mut Context<Folio>| {
                base.update(cx, |base, cx| {
                    base.set_value(text, window, cx);
                    base.set_selected_range(selection, cx);
                });
            };

            // `⌘D` takes the word under the caret, then the next place it
            // appears, and stops when there is nothing left to take.
            let text = "let count = 1;\nlet other = count;\n";
            start(text, 5..5, window, cx);
            app.on_active_buffer(cx, |base, cx| base.select_next_occurrence(cx));
            assert_eq!(ranges(cx), vec![4..9]);
            app.on_active_buffer(cx, |base, cx| base.select_next_occurrence(cx));
            assert_eq!(ranges(cx), vec![4..9, 27..32]);
            app.on_active_buffer(cx, |base, cx| base.select_next_occurrence(cx));
            assert_eq!(ranges(cx), vec![4..9, 27..32]);

            // Typing lands at both, as one edit, and the carets end up after
            // what was typed at each of them.
            base.update(cx, |base, cx| {
                base.replace_in_every_selection("total", window, cx)
            });
            assert_eq!(value(cx), "let total = 1;\nlet other = total;\n");
            assert_eq!(ranges(cx), vec![9..9, 32..32]);

            // `⌘⇧L` takes every occurrence at once, and the one the caret was
            // in stays the one the caret is in.
            start(text, 5..5, window, cx);
            app.on_active_buffer(cx, |base, cx| base.select_all_occurrences(cx));
            assert_eq!(ranges(cx), vec![4..9, 27..32]);
            base.update(cx, |base, cx| assert!(base.collapse_selections(cx)));
            assert_eq!(ranges(cx), vec![4..9]);
            // An escape with one cursor is not the editor's to take.
            base.update(cx, |base, cx| assert!(!base.collapse_selections(cx)));

            // `⌥⌘↓` adds a caret on the line below, in the column the first one
            // was in, and `⌥⌘↑` adds one above the topmost.
            start("x = 1;\ny = 2;\n", 0..0, window, cx);
            app.on_active_buffer(cx, |base, cx| base.add_cursor_below(cx));
            assert_eq!(ranges(cx), vec![0..0, 7..7]);
            base.update(cx, |base, cx| {
                base.replace_in_every_selection("let ", window, cx)
            });
            assert_eq!(value(cx), "let x = 1;\nlet y = 2;\n");
            assert_eq!(ranges(cx), vec![4..4, 15..15]);

            start("x = 1;\ny = 2;\n", 7..7, window, cx);
            app.on_active_buffer(cx, |base, cx| base.add_cursor_above(cx));
            assert_eq!(ranges(cx), vec![0..0, 7..7]);

            // Backspace at two carets takes the character before each of them.
            start("let a = 1;\nlet b = 2;\n", 4..4, window, cx);
            app.on_active_buffer(cx, |base, cx| base.add_cursor_below(cx));
            assert_eq!(ranges(cx), vec![4..4, 15..15]);
            base.update(cx, |base, cx| {
                base.delete_at_every_selection(false, window, cx)
            });
            assert_eq!(value(cx), "leta = 1;\nletb = 2;\n");

            // Enter breaks at each caret, indented to the line it breaks.
            start("  one\n  two\n", 5..5, window, cx);
            app.on_active_buffer(cx, |base, cx| base.add_cursor_below(cx));
            assert_eq!(ranges(cx), vec![5..5, 11..11]);
            base.update(cx, |base, cx| {
                base.insert_line_break_at_every_selection(window, cx)
            });
            assert_eq!(value(cx), "  one\n  \n  two\n  \n");
            assert_eq!(ranges(cx), vec![8..8, 17..17]);

            // Enter between two brackets at once lays out both pairs, and each
            // caret lands on the line between its own — not at the end of what
            // was inserted, which is where a caret goes for every other edit.
            start("if a {}\nif b {}\n", 6..6, window, cx);
            app.on_active_buffer(cx, |base, cx| base.add_cursor_below(cx));
            assert_eq!(ranges(cx), vec![6..6, 14..14]);
            base.update(cx, |base, cx| base.insert_line_break(window, cx));
            assert_eq!(value(cx), "if a {\n    \n}\nif b {\n    \n}\n");
            assert_eq!(ranges(cx), vec![11..11, 25..25]);

            // A bracket with more than one cursor is typed at each of them
            // rather than paired around one selection.
            start("x = 1;\ny = 2;\n", 0..0, window, cx);
            app.on_active_buffer(cx, |base, cx| base.add_cursor_below(cx));
            app.open_pair("(", ")", window, cx);
            assert_eq!(value(cx), "(x = 1;\n(y = 2;\n");
            assert_eq!(ranges(cx), vec![1..1, 9..9]);

            // Commenting is one edit over the block the cursors span.
            start("x = 1;\ny = 2;\n", 0..0, window, cx);
            app.on_active_buffer(cx, |base, cx| base.add_cursor_below(cx));
            app.toggle_comment(window, cx);
            assert_eq!(value(cx), "// x = 1;\n// y = 2;\n");

            start("x = 1;\n", 0..0, window, cx);
            app.toggle_block_comment(window, cx);
            assert_eq!(value(cx), "/* x = 1; */\n");
            app.toggle_block_comment(window, cx);
            assert_eq!(value(cx), "x = 1;\n");

            start("one\ntwo\n", 0..0, window, cx);
            app.duplicate_lines(window, cx);
            assert_eq!(value(cx), "one\none\ntwo\n");

            start("one\ntwo\nthree\n", 0..0, window, cx);
            app.move_lines(true, window, cx);
            assert_eq!(value(cx), "two\none\nthree\n");
            app.move_lines(false, window, cx);
            assert_eq!(value(cx), "one\ntwo\nthree\n");
        });
    }

    /// Column selection: a rectangle becomes one selection per line it covers,
    /// and typing replaces every line of it.
    #[gpui::test]
    fn a_column_selection_covers_a_rectangle(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let file = root.join("main.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let view = cx.update(|window, cx| {
            cx.new(|cx| {
                let mut app = Folio::new(window, cx);
                app.recent_task = None;
                app.recent_file = root.join("recent.json");
                app.settings_file = root.join("settings.json");
                app.window_file = root.join("window.json");
                app.settings = Settings::default();
                app.applied_ignored = app.settings.ignored.clone();
                app
            })
        });
        view.update_in(cx, |app, window, cx| {
            app.request(Next::Open(root.clone()), window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            app.open_file(file.clone(), window, cx)
        });
        cx.run_until_parked();

        view.update_in(cx, |app, window, cx| {
            let base = app.project.documents[&file]
                .editor
                .read(cx)
                .base_state()
                .clone();
            let value = |cx: &App| base.read(cx).value().to_string();
            let ranges = |cx: &App| base.read(cx).selected_ranges();
            let start = |text: &str,
                         selection: std::ops::Range<usize>,
                         window: &mut Window,
                         cx: &mut Context<Folio>| {
                base.update(cx, |base, cx| {
                    base.set_value(text, window, cx);
                    base.set_selected_range(selection, cx);
                });
            };

            // The keys grow a column of carets, a line at a time, and take it
            // back the same way.
            start("aaa\nbbb\nccc\n", 1..1, window, cx);
            app.on_active_buffer(cx, |base, cx| {
                base.extend_box(1, cx);
            });
            assert_eq!(ranges(cx), vec![1..1, 5..5]);
            app.on_active_buffer(cx, |base, cx| {
                base.extend_box(1, cx);
            });
            assert_eq!(ranges(cx), vec![1..1, 5..5, 9..9]);
            app.on_active_buffer(cx, |base, cx| {
                base.extend_box(-1, cx);
            });
            assert_eq!(ranges(cx), vec![1..1, 5..5]);

            // The mouse's rectangle: the columns between the corner it started
            // at and the one it was dragged to, on every line between them.
            start("aaa\nbbb\nccc\n", 0..0, window, cx);
            base.update(cx, |base, cx| {
                base.begin_box(0, cx);
                base.drag_box_to(10, cx);
            });
            assert_eq!(ranges(cx), vec![0..2, 4..6, 8..10]);

            // Typing goes into every line of it, as one edit.
            base.update(cx, |base, cx| {
                base.replace_in_every_selection("X", window, cx)
            });
            assert_eq!(value(cx), "Xa\nXb\nXc\n");
            assert_eq!(ranges(cx), vec![1..1, 4..4, 7..7]);

            // A line too short for the rectangle gives up at its own end
            // rather than reaching into the next one.
            start("aaa\nb\nccc\n", 0..0, window, cx);
            base.update(cx, |base, cx| {
                base.begin_box(0, cx);
                base.drag_box_to(8, cx);
            });
            assert_eq!(ranges(cx), vec![0..2, 4..5, 6..8]);

            // A movement is not part of the rectangle, so the keys start a new
            // one from where the caret has gone.
            base.update(cx, |base, cx| base.set_selected_range(1..1, cx));
            app.on_active_buffer(cx, |base, cx| {
                base.extend_box(1, cx);
            });
            assert_eq!(ranges(cx), vec![1..1, 5..5]);
        });
    }

    /// Folding: the block the caret is in closes, and a caret that was inside
    /// what closed goes to the header rather than staying on a line nothing
    /// draws.
    ///
    /// The fold candidates are handed in rather than derived: in the app they
    /// come from the grammar's fold query, through a highlighter the component
    /// library attaches when it renders the input — which this harness cannot
    /// do (see the note above `a_keystroke_matches_the_shortcut_it_prints`).
    /// That the query produces them is the crate's business; what is checked
    /// here is what the editor does with them.
    #[gpui::test]
    fn folding_closes_the_block_at_the_caret(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let file = root.join("main.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let view = cx.update(|window, cx| {
            cx.new(|cx| {
                let mut app = Folio::new(window, cx);
                app.recent_task = None;
                app.recent_file = root.join("recent.json");
                app.settings_file = root.join("settings.json");
                app.window_file = root.join("window.json");
                app.settings = Settings::default();
                app.applied_ignored = app.settings.ignored.clone();
                app
            })
        });
        view.update_in(cx, |app, window, cx| {
            app.request(Next::Open(root.clone()), window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            app.open_file(file.clone(), window, cx)
        });
        cx.run_until_parked();

        view.update_in(cx, |app, _window, cx| {
            let base = app.project.documents[&file]
                .editor
                .read(cx)
                .base_state()
                .clone();
            let caret = |cx: &App| base.read(cx).selected_range();
            let folded = |cx: &App| base.read(cx).folded_lines();

            // `fn main() {` on line 0 with the `if` block on 1..3 inside it, so
            // the caret in `let y` is in two blocks at once.
            base.update(cx, |base, cx| {
                base.set_value(
                    "fn main() {\n    if x {\n        let y = 1;\n    }\n}\n",
                    _window,
                    cx,
                );
                base.apply_highlighter_fold_candidates(
                    vec![input::FoldRange::new(0, 4), input::FoldRange::new(1, 3)],
                    cx,
                );
                base.set_selected_range(30..30, cx);
            });
            assert!(folded(cx).is_empty());

            // The innermost block, so pressing again takes the one around it.
            base.update(cx, |base, cx| assert!(base.fold_at_cursor(cx)));
            assert_eq!(folded(cx), vec![1]);
            // The caret was on a line that stopped being drawn.
            assert_eq!(caret(cx), 22..22);

            base.update(cx, |base, cx| assert!(base.unfold_at_cursor(cx)));
            assert!(folded(cx).is_empty());

            // The outer block, from the header line.
            base.update(cx, |base, cx| {
                base.set_selected_range(0..0, cx);
                assert!(base.fold_at_cursor(cx));
            });
            assert_eq!(folded(cx), vec![0]);

            // Nothing to fold and nothing to unfold are both quiet, and the
            // caret is left alone either way.
            base.update(cx, |base, cx| {
                base.set_selected_range(50..50, cx);
                assert!(!base.fold_at_cursor(cx));
                assert!(!base.unfold_at_cursor(cx));
            });
            assert_eq!(folded(cx), vec![0]);
            assert_eq!(caret(cx), 50..50);
        });
    }

    /// A file is opened with the indentation its project asks for, ahead of
    /// what the settings say.
    ///
    /// Read through Enter between a pair of brackets, since the indentation is
    /// what that writes and the editor does not expose its tab size otherwise.
    #[gpui::test]
    fn an_editorconfig_decides_a_files_indentation(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        std::fs::write(
            root.join(".editorconfig"),
            "root = true\n\n[*.rs]\nindent_style = space\nindent_size = 2\n",
        )
        .unwrap();
        let file = root.join("main.rs");
        std::fs::write(&file, "if x {}\n").unwrap();
        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let view = cx.update(|window, cx| {
            cx.new(|cx| {
                let mut app = Folio::new(window, cx);
                app.recent_task = None;
                app.recent_file = root.join("recent.json");
                app.settings_file = root.join("settings.json");
                app.window_file = root.join("window.json");
                // The settings ask for four spaces; the file asks for two.
                app.settings = Settings::default();
                app.applied_ignored = app.settings.ignored.clone();
                app
            })
        });
        view.update_in(cx, |app, window, cx| {
            app.request(Next::Open(root.clone()), window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            app.open_file(file.clone(), window, cx)
        });
        cx.run_until_parked();

        view.update_in(cx, |app, window, cx| {
            assert_eq!(app.settings.tab_size, 4);
            let base = app.project.documents[&file]
                .editor
                .read(cx)
                .base_state()
                .clone();
            base.update(cx, |base, cx| {
                base.set_selected_range(6..6, cx);
                base.insert_line_break(window, cx);
            });
            assert_eq!(base.read(cx).value(), "if x {\n  \n}\n");
        });
    }

    /// A session is put back: the projects that were open, the files each was
    /// showing in the order the strip had them, and the buffer whose edits
    /// never reached the disk.
    #[gpui::test]
    fn a_session_is_put_back(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let a = root.join("A");
        let b = root.join("B");
        for dir in [&a, &b] {
            std::fs::create_dir(dir).unwrap();
        }
        let a_main = a.join("main.rs");
        let a_notes = a.join("notes.md");
        let b_lib = b.join("lib.rs");
        std::fs::write(&a_main, "// a\n").unwrap();
        std::fs::write(&a_notes, "// notes\n").unwrap();
        std::fs::write(&b_lib, "// b\n").unwrap();

        let config = root.join("config");
        std::fs::create_dir(&config).unwrap();
        let session_file = config.join("session.json");
        let unsaved_file = config.join("unsaved.json");
        session::Session {
            projects: vec![
                session::SessionProject {
                    root: a.clone(),
                    tabs: vec![a_main.clone(), a_notes.clone()],
                    active: Some(a_main.clone()),
                },
                session::SessionProject {
                    root: b.clone(),
                    tabs: vec![b_lib.clone()],
                    active: Some(b_lib.clone()),
                },
            ],
        }
        .save(&session_file)
        .unwrap();
        let mut unsaved = session::UnsavedBuffers::new();
        unsaved.insert(a_notes.clone(), "// recovered\n".to_string());
        session::save_unsaved(&unsaved_file, &unsaved).unwrap();

        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let view = cx.update(|window, cx| {
            cx.new(|cx| {
                let mut app = Folio::new(window, cx);
                app.recent_task = None;
                app.recent_file = config.join("recent.json");
                app.settings_file = config.join("settings.json");
                app.window_file = config.join("window.json");
                app.session_file = session_file.clone();
                app.unsaved_file = unsaved_file.clone();
                app.settings = Settings::default();
                app.applied_ignored = app.settings.ignored.clone();
                app
            })
        });
        view.update_in(cx, |app, window, cx| app.start_session(window, cx));
        // A project and each of its files are separate turns of the executor,
        // so the queue drains over several of them.
        for _ in 0..8 {
            cx.run_until_parked();
        }

        view.update_in(cx, |app, _, cx| {
            assert_eq!(app.project_order, vec![a.clone(), b.clone()]);
            // The second project is the one being shown; the first is parked
            // with everything it had.
            assert_eq!(app.parked.len(), 1);
            let first = &app.parked[0];
            assert_eq!(first.tabs, vec![a_main.clone(), a_notes.clone()]);
            assert_eq!(first.active.as_ref(), Some(&a_main));
            // The buffer that had unsaved edits came back dirty, with them.
            let notes = &first.documents[&a_notes];
            assert!(notes.dirty);
            assert_eq!(notes.editor.read(cx).value().as_ref(), "// recovered\n");
            // And the one that did not is clean.
            assert!(!first.documents[&a_main].dirty);

            assert_eq!(app.project.tabs, vec![b_lib.clone()]);
            assert_eq!(app.project.active.as_ref(), Some(&b_lib));
        });
    }

    /// What is open, and what is unsaved, get written down while the app runs;
    /// a clean exit takes the unsaved file with it, which is what leaves the
    /// file behind only when there was no clean exit.
    #[gpui::test]
    fn the_session_and_unsaved_edits_are_written_down(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let file = root.join("main.rs");
        std::fs::write(&file, "// original\n").unwrap();
        let config = root.join("config");
        std::fs::create_dir(&config).unwrap();
        let session_file = config.join("session.json");
        let unsaved_file = config.join("unsaved.json");

        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let view = cx.update(|window, cx| {
            cx.new(|cx| {
                let mut app = Folio::new(window, cx);
                app.recent_task = None;
                app.recent_file = config.join("recent.json");
                app.settings_file = config.join("settings.json");
                app.window_file = config.join("window.json");
                app.session_file = session_file.clone();
                app.unsaved_file = unsaved_file.clone();
                app.settings = Settings::default();
                app.applied_ignored = app.settings.ignored.clone();
                app
            })
        });
        view.update_in(cx, |app, window, cx| {
            app.request(Next::Open(root.clone()), window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            app.open_file(file.clone(), window, cx)
        });
        cx.run_until_parked();
        // Nothing has been written yet: the timer has not come round.
        assert!(!session_file.exists());

        view.update_in(cx, |app, window, cx| {
            let base = app.project.documents[&file]
                .editor
                .read(cx)
                .base_state()
                .clone();
            base.update(cx, |base, cx| base.replace_all("// edited\n", window, cx));
        });
        cx.run_until_parked();

        view.update_in(cx, |app, _, cx| {
            assert!(app.project.documents[&file].dirty);
            app.pending_writes(cx).write();
        });

        let session = session::Session::load(&session_file);
        assert_eq!(session.projects.len(), 1);
        assert_eq!(session.projects[0].tabs, vec![file.clone()]);
        assert_eq!(session.projects[0].active.as_ref(), Some(&file));
        assert_eq!(session::load_unsaved(&unsaved_file)[&file], "// edited\n");

        // Saving it would have emptied the unsaved file; quitting leaves it
        // behind either way, which is what the next launch reads.
        view.update_in(cx, |app, _, _| {
            app.finish_session();
            assert!(session_file.exists());
        });
        assert!(session::load_unsaved(&unsaved_file).is_empty());
    }

    /// Blame: the strip says who last wrote the line the caret is on, and says
    /// something else when the caret moves to a line another commit wrote.
    #[gpui::test]
    fn blame_follows_the_caret(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let file = root.join("main.rs");
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-q"]);
        std::fs::write(&file, "let one = 1;\nlet two = 2;\n").unwrap();
        git(&["add", "."]);
        git(&[
            "-c",
            "user.name=Folio Test",
            "-c",
            "user.email=test@localhost",
            "commit",
            "-qm",
            "fixture",
        ]);
        std::fs::write(&file, "let one = 1;\nlet two = 22;\n").unwrap();
        git(&["add", "."]);
        git(&[
            "-c",
            "user.name=Ada Lovelace",
            "-c",
            "user.email=ada@example.com",
            "commit",
            "-qm",
            "second pass",
        ]);

        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let view = cx.update(|window, cx| {
            cx.new(|cx| {
                let mut app = Folio::new(window, cx);
                app.recent_task = None;
                app.recent_file = root.join("recent.json");
                app.settings_file = root.join("settings.json");
                app.window_file = root.join("window.json");
                app.settings = Settings::default();
                app.applied_ignored = app.settings.ignored.clone();
                app
            })
        });
        view.update_in(cx, |app, window, cx| {
            app.request(Next::Open(root.clone()), window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            app.open_file(file.clone(), window, cx)
        });
        cx.run_until_parked();

        view.update_in(cx, |app, _, cx| {
            // Nothing is shown until it is asked for.
            assert!(app.blame_text(cx).is_none());
        });
        view.update_in(cx, |app, window, cx| app.toggle_blame(window, cx));
        // The blame is a git call, which happens behind the window.
        cx.run_until_parked();

        view.update_in(cx, |app, _, cx| {
            let base = app.project.documents[&file]
                .editor
                .read(cx)
                .base_state()
                .clone();
            let text = app.blame_text(cx).expect("a line to blame");
            assert!(text.contains("fixture"), "{text}");
            assert!(text.contains("Folio Test"), "{text}");

            // The second line was rewritten by someone else.
            base.update(cx, |base, cx| base.set_selected_range(17..17, cx));
            let text = app.blame_text(cx).expect("a line to blame");
            assert!(text.contains("second pass"), "{text}");
            assert!(text.contains("Ada Lovelace"), "{text}");
        });

        view.update_in(cx, |app, window, cx| {
            app.toggle_blame(window, cx);
            assert!(app.blame_text(cx).is_none());
        });
    }

    /// The shortcut strings the menus print are matched against real
    /// keystrokes, and the capture-phase claim for the project search rests on
    /// this one matching. The key dispatch itself cannot be exercised here, so
    /// what is pinned is the predicate rather than the dispatch.
    ///
    /// Why it cannot, since `simulate_keystrokes` is the obvious thing to try.
    /// Dispatch resolves against the drawn frame, and nothing is drawn in a
    /// test window: `add_empty_window` has no root view, so the tests that need
    /// a tree build one by hand. Giving the window a root view with
    /// `add_window_view` does make the harness draw — and then the first draw
    /// with an input focused panics inside gpui-component, which asks the
    /// window for its `NSView` to set a text content type and gets "Test
    /// Windows are not backed by a real platform window". An editor is an
    /// input, and opening a file focuses one, so there is no order that both
    /// draws and keeps a focus. Both halves were checked against the editor.
    #[test]
    fn a_keystroke_matches_the_shortcut_it_prints() {
        let keystroke = |source: &str| Keystroke::parse(source).unwrap();
        assert!(matches_shortcut(
            &keystroke("cmd-shift-f"),
            "\u{2318}\u{21e7}F"
        ));
        // A different key, a missing modifier, an extra one.
        assert!(!matches_shortcut(
            &keystroke("cmd-shift-f"),
            "\u{2318}\u{21e7}H"
        ));
        assert!(!matches_shortcut(
            &keystroke("shift-f"),
            "\u{2318}\u{21e7}F"
        ));
        assert!(!matches_shortcut(&keystroke("cmd-f"), "\u{2318}\u{21e7}F"));
        assert!(matches_shortcut(
            &keystroke("cmd-shift-z"),
            "\u{2318}\u{21e7}Z"
        ));
        // The menu's own shortcuts are matched the same way.
        assert!(matches_shortcut(
            &keystroke("alt-cmd-t"),
            "\u{2325}\u{2318}T"
        ));
    }

    /// The About window builds. Its version comes from the manifest, so there
    /// is nothing to assert about it that the compiler has not already.
    #[gpui::test]
    fn the_about_view_renders(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let about = cx.update(|_, cx| cx.new(|_| AboutView));
        about.update_in(cx, |view, window, cx| {
            // The harness never draws on its own.
            let _ = view.render(window, cx);
        });
    }

    /// The settings form end to end: what a change writes to disk and how it
    /// reaches the tree. `show_settings` itself is not covered — opening a
    /// window needs a real platform window, which the test harness does not
    /// have.
    #[gpui::test]
    fn settings_view_persists_and_drives_the_tree(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let src = root.join("src");
        for name in ["src", "notes"] {
            std::fs::create_dir(root.join(name)).unwrap();
        }
        std::fs::write(src.join("main.rs"), "// hi\n").unwrap();
        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let view = cx.update(|window, cx| {
            cx.new(|cx| {
                let mut app = Folio::new(window, cx);
                app.recent_task = None;
                app.recent_file = root.join("recent.json");
                app.settings_file = root.join("settings.json");
                app.window_file = root.join("window.json");
                app.settings = Settings::default();
                app.applied_ignored = app.settings.ignored.clone();
                app
            })
        });
        view.update_in(cx, |app, window, cx| {
            app.request(Next::Open(root.clone()), window, cx)
        });
        cx.run_until_parked();

        // The settings form, wired to the app view the way the window wires it.
        let settings = view.update_in(cx, |app, window, cx| {
            let folio = cx.entity().downgrade();
            let settings = cx.new(|cx| SettingsView::new(folio, app.settings.clone(), window, cx));
            app.settings_view = Some(settings.clone());
            settings
        });

        // Every page builds, not just the one it opens on, with headings both
        // open and closed; the harness never draws on its own.
        settings.update_in(cx, |view, window, cx| {
            assert_eq!(view.page, Page::Interface);
            assert!(view.expanded.iter().all(|open| *open));
            for collapsed in [false, true] {
                view.expanded = [collapsed; GROUPS.len()];
                for group in GROUPS {
                    for page in group.pages {
                        view.page = *page;
                        let _ = view.render(window, cx);
                    }
                }
            }
            view.expanded = [true; GROUPS.len()];
            view.page = Page::Interface;
        });

        // Stepping a value applies it and writes the file.
        settings.update_in(cx, |view, _, cx| {
            view.change(
                |settings| {
                    settings.tab_size = 2;
                    settings.hard_tabs = true;
                },
                cx,
            );
        });
        cx.run_until_parked();
        let stored = settings::load(&root.join("settings.json")).unwrap();
        assert_eq!((stored.tab_size, stored.hard_tabs), (2, true));

        // A value outside the range is pulled back rather than written.
        settings.update_in(cx, |view, _, cx| {
            view.change(|settings| settings.font_size = 99., cx);
        });
        cx.run_until_parked();
        view.update_in(cx, |app, _, _| {
            assert_eq!(app.settings.font_size, settings::FONT_SIZE.1);
        });

        // The text fields settle together, so a value typed in one is not lost
        // by settling another.
        settings.update_in(cx, |view, window, cx| {
            let code_font = view.code_font_query.clone();
            let ignore = view.ignore_query.clone();
            code_font.update(cx, |input, cx| input.set_value("Geist Mono", window, cx));
            ignore.update(cx, |input, cx| input.set_value("src, notes", window, cx));
            view.commit_text(window, cx);
        });
        cx.run_until_parked();
        let stored = settings::load(&root.join("settings.json")).unwrap();
        assert_eq!(stored.code_font_family.as_deref(), Some("Geist Mono"));
        assert_eq!(stored.ignored, vec!["src".to_string(), "notes".to_string()]);

        // Both folders are ignored now, so the tree and the index agree that
        // neither is there — the new rules reached the cached listings.
        view.update_in(cx, |app, _, _| {
            assert!(
                app.project
                    .rows
                    .iter()
                    .all(|row| row.entry.name != "src" && row.entry.name != "notes"),
                "the new rules reached the cached listings"
            );
            assert!(app.project.files.iter().all(|path| !path.starts_with(&src)));
        });
    }

    /// Open the tree's context menu over a folder, the way a right-click does.
    fn open_tree_menu(
        app: &mut Folio,
        folder: &Path,
        root: bool,
        window: &mut Window,
        cx: &mut Context<Folio>,
    ) {
        app.open_menu(
            MenuTarget::Tree {
                path: folder.to_path_buf(),
                root,
            },
            point(px(120.), px(200.)),
            window,
            cx,
        );
    }

    #[gpui::test]
    fn tree_context_menu_creates_moves_and_duplicates_entries(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let src = root.join("src");
        std::fs::create_dir(&src).unwrap();
        std::fs::write(src.join("lib.rs"), "// child\n").unwrap();
        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let view = cx.update(|window, cx| {
            cx.new(|cx| {
                let mut app = Folio::new(window, cx);
                app.recent_task = None;
                app.recent_file = root.join("recent.json");
                // Never read or write the real settings file from a test.
                app.settings_file = root.join("settings.json");
                app.window_file = root.join("window.json");
                app.settings = Settings::default();
                app.applied_ignored = app.settings.ignored.clone();
                app
            })
        });
        view.update_in(cx, |app, window, cx| {
            app.request(Next::Open(root.clone()), window, cx)
        });
        cx.run_until_parked();

        view.update_in(cx, |app, window, cx| {
            assert!(
                !app.menu_item_enabled(MenuItem::Paste, &root),
                "paste stays disabled until something is on the clipboard"
            );
            open_tree_menu(app, &src, false, window, cx);
            assert!(app.menu.is_some());
            // The harness never draws, so build the element tree by hand to
            // exercise the menu's rendering.
            let _ = app.render(window, cx);
            app.move_menu_selection(true);
            assert_eq!(app.menu.as_ref().unwrap().selected, 1);
            assert!(app.menu_item_enabled(TREE_MENU[1].0, &src));

            app.run_menu_item(MenuItem::NewFile, window, cx);
            assert!(app.menu.is_none(), "choosing an item closes the menu");
            let edit = app.editing.as_ref().expect("the row is being named");
            assert_eq!(edit.kind, EntryKind::File);
            assert_eq!(edit.parent, src);
            // The field is spliced in as the folder's first child, one level
            // deeper than the folder's own row.
            assert_eq!(app.project.rows[edit.row - 1].entry.path, src);
            assert_eq!(app.project.rows[edit.row].depth, 1);
            // That row draws a text field instead of an entry; render it too.
            let _ = app.render(window, cx);
        });

        // Enter is what creates the entry, so nothing exists yet.
        assert!(!src.join("笔记.md").exists());
        view.update_in(cx, |app, window, cx| {
            let input = app.editing.as_ref().unwrap().input.clone();
            input.update(cx, |input, cx| input.set_value("笔记.md", window, cx));
            app.commit_edit(window, cx);
            assert!(app.editing.is_none());
        });
        cx.run_until_parked();
        let note = src.join("笔记.md");
        assert!(note.is_file(), "the named file must exist on disk");
        view.update_in(cx, |app, _, _| {
            assert!(app.project.rows.iter().any(|row| row.entry.path == note));
            assert_eq!(app.project.active.as_ref(), Some(&note));
        });

        // Escape abandons the row without touching the filesystem.
        let before = std::fs::read_dir(&src).unwrap().count();
        view.update_in(cx, |app, window, cx| {
            app.begin_create(src.clone(), EntryKind::Directory, window, cx);
            assert!(app.editing.is_some());
            app.cancel_create(window, cx);
            assert!(app.editing.is_none());
        });
        assert_eq!(std::fs::read_dir(&src).unwrap().count(), before);

        // A name the filesystem cannot hold is reported, not written.
        view.update_in(cx, |app, window, cx| {
            app.begin_create(src.clone(), EntryKind::File, window, cx);
            let input = app.editing.as_ref().unwrap().input.clone();
            input.update(cx, |input, cx| input.set_value("a/b", window, cx));
            app.commit_edit(window, cx);
        });
        cx.run_until_parked();
        view.update_in(cx, |app, _, _| {
            assert!(
                app.message
                    .as_deref()
                    .is_some_and(|message| message.contains("path separator")),
                "an unusable name must surface as a message"
            );
        });

        // Cut then paste moves the entry and empties the clipboard.
        view.update_in(cx, |app, window, cx| {
            open_tree_menu(app, &note, false, window, cx);
            app.run_menu_item(MenuItem::Cut, window, cx);
            assert_eq!(app.clipboard.as_ref().map(|entry| entry.cut), Some(true));
            open_tree_menu(app, &root, false, window, cx);
            app.run_menu_item(MenuItem::Paste, window, cx);
            assert!(app.clipboard.is_none(), "a cut is spent once it lands");
        });
        cx.run_until_parked();
        let moved = root.join("笔记.md");
        assert!(moved.is_file() && !note.exists());

        // Copy leaves the source behind, so the name is taken in `src` again.
        view.update_in(cx, |app, window, cx| {
            open_tree_menu(app, &moved, false, window, cx);
            app.run_menu_item(MenuItem::Copy, window, cx);
            open_tree_menu(app, &src, false, window, cx);
            app.run_menu_item(MenuItem::Paste, window, cx);
            assert_eq!(app.clipboard.as_ref().map(|entry| entry.cut), Some(false));
        });
        cx.run_until_parked();
        assert!(src.join("笔记.md").is_file() && moved.is_file());

        view.update_in(cx, |app, window, cx| {
            open_tree_menu(app, &moved, false, window, cx);
            app.run_menu_item(MenuItem::Duplicate, window, cx);
        });
        cx.run_until_parked();
        assert!(root.join("笔记 copy.md").is_file());

        // Find in Folder narrows the project search to the folder it opened on.
        view.update_in(cx, |app, window, cx| {
            open_tree_menu(app, &src, false, window, cx);
            app.run_menu_item(MenuItem::FindInFolder, window, cx);
            assert_eq!(app.panel, Some(Panel::Search));
            assert_eq!(app.search.scope.as_deref(), Some(src.as_path()));
        });

        // Reopening the panel from the keyboard clears that scope again.
        view.update_in(cx, |app, window, cx| {
            app.show_search(false, window, cx);
            assert!(app.search.scope.is_none());
        });

        // The project root is the workspace's identity, so its menu drops the
        // entries that would rename or remove it.
        let labels = |target: &MenuTarget| {
            visible_menu_items(target)
                .into_iter()
                .map(|index| surface_of(target).menu().0[index].1)
                .collect::<Vec<_>>()
        };
        let tree_labels = |root: bool| {
            labels(&MenuTarget::Tree {
                path: PathBuf::new(),
                root,
            })
        };
        for dropped in ["Rename", "Move to Trash", "Delete Immediately"] {
            assert!(
                !tree_labels(true).contains(&dropped),
                "{dropped} on the root"
            );
            assert!(
                tree_labels(false).contains(&dropped),
                "{dropped} inside a folder"
            );
        }
        // The changes view offers the way out and nothing the tree has.
        let changes = labels(&MenuTarget::Changes {
            path: PathBuf::new(),
        });
        assert_eq!(changes, vec!["Hide File Changes"]);
        assert!(!tree_labels(false).contains(&"Hide File Changes"));
        view.update_in(cx, |app, window, cx| {
            open_tree_menu(app, &root, true, window, cx);
            assert_eq!(
                app.menu.as_ref().unwrap().target,
                MenuTarget::Tree {
                    path: root.clone(),
                    root: true
                }
            );
            open_tree_menu(app, &src, false, window, cx);
            assert_eq!(
                app.menu.as_ref().unwrap().target,
                MenuTarget::Tree {
                    path: src.clone(),
                    root: false
                }
            );
            app.close_menu(cx);
        });

        // Rename moves the entry and takes the caches keyed off its path with
        // it, including an open buffer.
        let library = src.join("lib.rs");
        view.update_in(cx, |app, window, cx| {
            app.open_file(library.clone(), window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            open_tree_menu(app, &library, false, window, cx);
            app.run_menu_item(MenuItem::Rename, window, cx);
            let edit = app.editing.as_ref().expect("the row is being renamed");
            assert_eq!(edit.renaming.as_deref(), Some(library.as_path()));
            // The field takes over the entry's own row instead of adding one.
            assert_eq!(app.project.rows[edit.row].entry.path, library);
            let input = edit.input.clone();
            input.update(cx, |input, cx| input.set_value("core.rs", window, cx));
            app.commit_edit(window, cx);
        });
        cx.run_until_parked();
        let core = src.join("core.rs");
        assert!(core.is_file() && !library.exists());
        // The open buffer moved with it under its new key, and the entries that
        // were not renamed stayed where they were.
        view.update_in(cx, |app, _, _| {
            assert!(app.project.documents.contains_key(&core));
            assert_eq!(app.project.active.as_ref(), Some(&core));
            assert!(app.project.documents.contains_key(&src.join("笔记.md")));
        });

        // Trashing is recoverable, so it only stops when it would drop edits
        // that are not on disk. Cancelling must leave the folder alone.
        view.update_in(cx, |app, window, cx| {
            // The test harness cannot drive the editor's change events, so put
            // the open buffer into the state a real edit would leave it in.
            app.project.documents.get_mut(&core).unwrap().dirty = true;
            assert_eq!(app.unsaved_under(&src), 1);
            open_tree_menu(app, &src, false, window, cx);
            app.run_menu_item(MenuItem::Trash, window, cx);
        });
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert!(src.is_dir() && core.is_file());

        // Delete always asks, because nothing about it can be undone.
        let doomed = root.join("doomed");
        std::fs::create_dir(&doomed).unwrap();
        std::fs::write(doomed.join("inner.txt"), "x").unwrap();
        view.update_in(cx, |app, window, cx| {
            open_tree_menu(app, &doomed, false, window, cx);
            app.run_menu_item(MenuItem::Delete, window, cx);
        });
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Delete");
        cx.run_until_parked();
        assert!(!doomed.exists(), "the whole subtree must go");
        view.update_in(cx, |app, _, _| {
            assert!(
                app.message
                    .as_deref()
                    .is_some_and(|message| message.contains("Deleted")),
                "a successful delete reports back instead of erroring"
            );
        });
    }

    #[gpui::test]
    fn external_edits_reload_clean_buffers_and_prompt_dirty_ones(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let file = root.join("main.rs");
        std::fs::write(&file, "fn first() {}\n").unwrap();
        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let view = cx.update(|window, cx| {
            cx.new(|cx| {
                let mut app = Folio::new(window, cx);
                app.recent_task = None;
                app.recent_file = root.join("recent.json");
                app.settings_file = root.join("settings.json");
                app.window_file = root.join("window.json");
                app.settings = Settings::default();
                app.applied_ignored = app.settings.ignored.clone();
                app
            })
        });
        view.update_in(cx, |app, window, cx| {
            app.request(Next::Open(root.clone()), window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            app.open_file(file.clone(), window, cx)
        });
        cx.run_until_parked();

        std::fs::write(&file, "fn second() {}\n").unwrap();
        view.update_in(cx, |app, window, cx| {
            app.apply_watch_paths(vec![file.clone()], window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |app, _, cx| {
            let document = &app.project.documents[&file];
            assert_eq!(document.editor.read(cx).value(), "fn second() {}\n");
            assert!(!document.dirty);
            assert!(app.project.disk_notice.is_empty());
        });

        view.update_in(cx, |app, window, cx| {
            let editor = app.project.documents[&file].editor.clone();
            editor.read(cx).base_state().clone().update(cx, |base, cx| {
                base.replace_all("fn local() {}\n", window, cx)
            });
            app.project.documents.get_mut(&file).unwrap().dirty = true;
        });
        std::fs::write(&file, "fn third() {}\n").unwrap();
        view.update_in(cx, |app, window, cx| {
            app.apply_watch_paths(vec![file.clone()], window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            let document = &app.project.documents[&file];
            assert_eq!(document.editor.read(cx).value(), "fn local() {}\n");
            assert_eq!(
                app.project.disk_notice.get(&file),
                Some(&DiskNotice::Changed)
            );
            app.project.disk_notice.remove(&file);
            app.reload_from_disk(file.clone(), window, cx);
        });
        cx.run_until_parked();
        view.update_in(cx, |app, _, cx| {
            let document = &app.project.documents[&file];
            assert_eq!(document.editor.read(cx).value(), "fn third() {}\n");
            assert!(!document.dirty);
            assert!(app.project.disk_notice.is_empty());
        });
    }

    #[gpui::test]
    fn dropping_a_tree_row_moves_the_file_and_its_buffer(cx: &mut TestAppContext) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let src = root.join("src");
        std::fs::create_dir(&src).unwrap();
        let file = root.join("main.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        cx.update(gpui_component::init);
        let cx = cx.add_empty_window();
        let view = cx.update(|window, cx| {
            cx.new(|cx| {
                let mut app = Folio::new(window, cx);
                app.recent_task = None;
                app.recent_file = root.join("recent.json");
                app.settings_file = root.join("settings.json");
                app.window_file = root.join("window.json");
                app.settings = Settings::default();
                app.applied_ignored = app.settings.ignored.clone();
                app
            })
        });
        view.update_in(cx, |app, window, cx| {
            app.request(Next::Open(root.clone()), window, cx)
        });
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| app.open_file(file.clone(), window, cx));
        cx.run_until_parked();
        view.update_in(cx, |app, window, cx| {
            app.project.documents[&file]
                .editor
                .read(cx)
                .base_state()
                .clone()
                .update(cx, |base, cx| base.replace_all("fn moved() {}\n", window, cx));
            app.project.documents.get_mut(&file).unwrap().dirty = true;
            app.drop_tree_entry(file.clone(), src.clone(), window, cx);
        });
        cx.run_until_parked();
        let dest = src.join("main.rs");
        assert!(dest.is_file());
        assert!(!file.exists());
        view.update_in(cx, |app, _, cx| {
            let document = app
                .project
                .documents
                .get(&dest)
                .expect("the open buffer follows the file");
            assert_eq!(document.editor.read(cx).value(), "fn moved() {}\n");
            assert!(document.dirty);
            assert!(!app.project.documents.contains_key(&file));
        });
    }
}
