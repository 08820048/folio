mod diff_ui;
mod editor_ui;
mod menu_ui;
mod notices;
mod project_ui;
mod render;
mod search_view;
mod settings_view;
mod shell_view;
mod tabs_view;
mod terminal_ui;
mod workspace_view;

use settings_view::{AboutView, SettingsView};
#[cfg(test)]
use settings_view::{GROUPS, Page};

use crate::assets::FolioIcon;
use crate::file_icons;
use crate::preview::{self, Content};
use crate::syntax;
use crate::terminal_view::{StopStep, TerminalBusy, TerminalView};
#[cfg(any(target_os = "macos", target_os = "windows"))]
use folio::windows_update;
use folio::{
    blame, brackets, buffer, diff, editorconfig, fs_op, git,
    recent::{self, RecentProject},
    run, search, session,
    settings::{self, Settings},
    tree::{self, Entry, EntryKind},
    watch,
    workspace::Workspace,
};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, Root, Sizable, Theme, TitleBar,
    button::{Button, ButtonVariants},
    input::{
        self, EditorState, Input, InputBaseState, InputEvent, InputState, Position, Rope, RopeExt,
        TabSize, TextDecoration, TextDecorationCollection,
    },
    select::{SearchableVec, Select, SelectEvent, SelectState},
    text::{TextView, TextViewStyle},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    io,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// Process start, for `FOLIO_PERF=1` first-frame timing.
static LAUNCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// Call once at the top of `main` so first-frame time includes window setup.
pub fn note_launch() {
    let _ = LAUNCH.get_or_init(Instant::now);
}

fn perf_on() -> bool {
    std::env::var_os("FOLIO_PERF").is_some()
}

fn perf_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

/// Resident set in megabytes. `ps` is good enough for a FOLIO_PERF line.
fn rss_mb() -> Option<f64> {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    let kb: f64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    Some(kb / 1024.0)
}

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

pub const BUNDLED_FONTS: &[(&str, &[u8])] = &[
    (
        "JetBrains Mono",
        include_bytes!("../../assets/JetBrainsMono-Regular.ttf"),
    ),
    (
        "Fira Code",
        include_bytes!("../../assets/fonts/FiraCode-Regular.ttf"),
    ),
    (
        "Source Code Pro",
        include_bytes!("../../assets/fonts/SourceCodePro-Regular.ttf"),
    ),
];

type FontSelect = SelectState<SearchableVec<String>>;

/// Bundled faces first, then installed fonts. Keep an old custom choice even
/// when it is unavailable on this machine, so another setting cannot erase it.
fn font_options(mut installed: Vec<String>, current: Option<&str>, code: bool) -> Vec<String> {
    installed.retain(|name| !name.starts_with('.') && !name.trim().is_empty());
    installed.extend(current.map(str::to_owned));
    installed.sort_unstable();
    installed.dedup();
    let mut options = Vec::new();
    if !code {
        options.push("System default".to_owned());
    }
    options.extend(BUNDLED_FONTS.iter().map(|(name, _)| (*name).to_owned()));
    for name in installed {
        if !options.contains(&name) {
            options.push(name);
        }
    }
    options
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
/// Floating notice card. Narrower than the old toast so it reads as chrome,
/// not a panel. Slides in from just past the right edge.
const NOTICE_WIDTH: f32 = 320.;
const NOTICE_INSET: f32 = 16.;
const NOTICE_ENTER: Duration = Duration::from_millis(280);
const NOTICE_LEAVE: Duration = Duration::from_millis(180);
/// File-tree row height. `uniform_list` needs a fixed size.
const TREE_ROW_HEIGHT: f32 = 27.;
/// Indent of a tree row per nesting level.
const TREE_ROW_INDENT: f32 = 16.;
/// Where the project header's folder icon sits inside the list: its `px_3`.
/// Top-level rows are `depth == 1`, so their icon lands one indent to the
/// right of it and every deeper level steps in by the same amount.
const TREE_ROW_PAD: f32 = 12.;
/// Width of a `small` icon, used to center guides under the parent icon.
const TREE_ICON_SIZE: f32 = 14.;
/// Project root header. Taller than a file row so the chevron and folder
/// read as a section, not another entry.
const PROJECT_HEADER_HEIGHT: f32 = 42.;
/// Vertical padding on the file list (`.p_1()` is 4px each side).
const TREE_LIST_PAD: f32 = 8.;

fn run_stop_color(cx: &App) -> Hsla {
    rgb(if cx.theme().is_dark() {
        0xC98982
    } else {
        0xA35B52
    })
    .into()
}

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

/// Left padding of a file-tree row.
fn tree_row_indent(depth: usize) -> Pixels {
    px(TREE_ROW_PAD + depth as f32 * TREE_ROW_INDENT)
}

/// How tall an expanded project may grow. `flex_1` would otherwise eat the
/// leftover sidebar and shove the next project to the bottom even when this
/// one only has a handful of rows.
fn expanded_project_max_height(rows: usize) -> f32 {
    PROJECT_HEADER_HEIGHT + TREE_ROW_HEIGHT * rows as f32 + TREE_LIST_PAD
}

/// Selected file-tree row. Lighter than `list_active`, so the pill does
/// not read as a grey slab on the white sidebar.
/// Scan a rope without always allocating a `String`. A single-chunk rope
/// is borrowed; a fragmented one is flattened once.
fn rope_pairs(rope: &Rope) -> Vec<brackets::Pair> {
    let mut chunks = rope.chunks();
    match (chunks.next(), chunks.next()) {
        (None, _) => Vec::new(),
        (Some(only), None) => brackets::pairs(only),
        (Some(first), Some(second)) => {
            let mut owned = String::with_capacity(rope.len());
            owned.push_str(first);
            owned.push_str(second);
            for chunk in chunks {
                owned.push_str(chunk);
            }
            brackets::pairs(&owned)
        }
    }
}

fn bracket_color(depth: usize, dark: bool) -> Hsla {
    // Sage, steel, ochre, plum, brick, teal — the same register as the rest
    // of the chrome, rotating so nested pairs stay distinct.
    const HUES: [f32; 6] = [0.38, 0.58, 0.12, 0.82, 0.02, 0.48];
    let hue = HUES[depth % HUES.len()];
    if dark {
        hsla(hue, 0.34, 0.72, 1.)
    } else {
        hsla(hue, 0.40, 0.42, 1.)
    }
}

fn tree_row_selected(cx: &App) -> Hsla {
    rgb(if cx.theme().is_dark() {
        0x24282A
    } else {
        0xF6F6F6
    })
    .into()
}

/// Hairline around a selected tree row. Close to the fill, so it keeps
/// the rounded shape without a second, darker grey.
fn tree_row_selected_border(cx: &App) -> Hsla {
    rgb(if cx.theme().is_dark() {
        0x2E3335
    } else {
        0xEFEFEF
    })
    .into()
}

/// Horizontal position of the indent guide for nesting `level`, centered
/// under the icon of the row that owns it: level 0 is the project header's
/// folder icon, level `n` is a depth-`n` row's icon.
fn tree_guide_x(level: usize) -> Pixels {
    px(TREE_ROW_PAD + TREE_ICON_SIZE / 2. + level as f32 * TREE_ROW_INDENT)
}

/// The guide that marks the folder containing `index`: first child row,
/// end (exclusive), and indent level. The parent row itself does not
/// carry this line — same as Zed. The project root is drawn as a header
/// outside these rows, so its children have no parent row; their guide
/// still runs through the flattened list.
fn tree_active_guide(rows: &[Row], index: usize) -> Option<(usize, usize, usize)> {
    let depth = rows.get(index)?.depth;
    if depth == 0 {
        return None;
    }
    let level = depth - 1;
    if let Some(parent) = (0..=index).rev().find(|&i| rows[i].depth == level) {
        let mut end = parent + 1;
        while end < rows.len() && rows[end].depth > level {
            end += 1;
        }
        Some((parent + 1, end, level))
    } else if level == 0 {
        let mut end = 0;
        while end < rows.len() && rows[end].depth > 0 {
            end += 1;
        }
        Some((0, end, 0))
    } else {
        None
    }
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
        assert_eq!(tree_guide_x(0), px(TREE_ROW_PAD + TREE_ICON_SIZE / 2.));
        assert_eq!(
            tree_guide_x(1),
            px(TREE_ROW_PAD + TREE_ICON_SIZE / 2. + TREE_ROW_INDENT)
        );
    }

    #[core::prelude::v1::test]
    fn each_level_steps_in_by_one_indent_from_the_header_icon() {
        // The header draws its folder icon right after `px_3`; a top-level
        // row (depth 1) must start one indent past that, not on top of it.
        let header_icon = 12.;
        assert_eq!(tree_row_indent(1), px(header_icon + TREE_ROW_INDENT));
        assert_eq!(tree_row_indent(2) - tree_row_indent(1), px(TREE_ROW_INDENT));
        // The level-0 guide runs under the header icon's center.
        assert_eq!(tree_guide_x(0), px(header_icon + TREE_ICON_SIZE / 2.));
    }

    #[core::prelude::v1::test]
    fn tree_active_guide_covers_children_of_the_project_header() {
        // The project root is a separate header, so flattening starts at 1:
        // src/       1
        //   app.rs   2
        // README     1
        let rows: Vec<Row> = [1, 2, 1].into_iter().map(row_at).collect();
        assert_eq!(tree_active_guide(&rows, 0), Some((0, 3, 0)));
        assert_eq!(tree_active_guide(&rows, 1), Some((1, 2, 1)));
        assert_eq!(tree_active_guide(&rows, 2), Some((0, 3, 0)));
    }

    #[core::prelude::v1::test]
    fn an_expanded_project_is_only_as_tall_as_its_rows() {
        let one = expanded_project_max_height(1);
        let three = expanded_project_max_height(3);
        assert_eq!(three - one, TREE_ROW_HEIGHT * 2.);
        assert!(one > PROJECT_HEADER_HEIGHT);
        assert!(three < 200., "a short tree must not fill a typical sidebar");
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
        NewProject,
        CloneFromGithub,
        CloseProject,
        Save,
        SaveAll,
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
        ToggleChanges,
        NextHunk,
        PrevHunk,
        NextChangeFile,
        PrevChangeFile,
        NextTab,
        PrevTab,
        ReopenClosedTab,
        SplitEditor,
        SplitEditorLeft,
        SplitEditorUp,
        SplitEditorDown,
        JoinEditor,
        FocusLeftPane,
        FocusRightPane,
        ToggleBlame,
        ToggleMarkdownPreview,
        RevealInFinder,
        OpenInTerminal,
        CopyFilePath,
        CopyRelativeFilePath,
        CheckForUpdates,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        Run,
        Rerun,
        StopRun,
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
    NewProject,
    CloneFromGithub,
    Open(PathBuf),
    Close,
    /// Close these open files. One for a single tab; several for Close
    /// Others and its neighbours.
    CloseTabs(Vec<PathBuf>),
    /// Save every dirty buffer in every open project, then stay put.
    SaveAll,
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
    /// Bumped on every change to the buffer. Recovery compares these instead
    /// of walking every dirty Rope every few seconds.
    revision: u64,
    large: bool,
    /// Markdown opens rendered. Source is the same editor.
    preview: bool,
    /// The title-bar Play is shown when this file has a command. Deciding
    /// that looks at the filesystem (`Cargo.toml`, `go.mod`), so it is
    /// answered once per document, not per keystroke; `None` is "not yet".
    can_run: Option<bool>,
    brackets: BracketPaint,
    _subscription: Subscription,
}

/// Rainbow colours on every matched pair. The caret's match is drawn by
/// the editor from the same pair list, so it updates as the caret moves.
struct BracketPaint {
    rainbow: TextDecorationCollection,
    last_rainbow: Vec<TextDecoration>,
}
#[derive(Clone)]
struct Row {
    entry: Entry,
    depth: usize,
}

/// What the workspace sidebar is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum SidebarKind {
    #[default]
    Tree,
    Changes,
}

/// How the changes view lays out a hunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum DiffLayout {
    #[default]
    Unified,
    Split,
}

/// What a file's changes are compared to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum DiffAgainst {
    #[default]
    Head,
    /// The repo's main branch, resolved when the view asks.
    Branch,
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
    RemoveProject,
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
    // The editor.
    SelectAll,
    GoToLine,
    ShowFileChanges,
    Blame,
    // The tab-bar split button, the way Zed's pane menu is.
    SplitRight,
    SplitLeft,
    SplitUp,
    SplitDown,
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
    /// The file open in the editor.
    Editor { path: PathBuf },
    /// The split button on a pane's tab bar.
    Split { pane: usize },
}

impl MenuTarget {
    /// The file the menu acts on.
    fn path(&self) -> &Path {
        match self {
            MenuTarget::Tree { path, .. }
            | MenuTarget::Changes { path }
            | MenuTarget::Tab { path, .. }
            | MenuTarget::Editor { path } => path,
            MenuTarget::Split { .. } => Path::new(""),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Surface {
    Tree,
    Changes,
    Tab,
    Editor,
    Split,
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
    (MenuItem::RemoveProject, "Remove Project", "⌘W"),
];
const TREE_SEPARATORS: &[usize] = &[2, 5, 6, 10, 11, 13];

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

const EDITOR_MENU: &[(MenuItem, &str, &str)] = &[
    (MenuItem::Cut, "Cut", "⌘X"),
    (MenuItem::Copy, "Copy", "⌘C"),
    (MenuItem::Paste, "Paste", "⌘V"),
    (MenuItem::SelectAll, "Select All", "⌘A"),
    (MenuItem::GoToLine, "Go to Line…", "⌘G"),
    (MenuItem::Reveal, "Reveal in Finder", "⌘⇧R"),
    (MenuItem::OpenTerminal, "Open in Terminal", ""),
    (MenuItem::CopyPath, "Copy Path", "⌥⌘C"),
    (MenuItem::CopyRelativePath, "Copy Relative Path", "⌥⌘⇧C"),
    (MenuItem::ShowFileChanges, "Show File Changes", "⌘⇧D"),
    (MenuItem::Blame, "Blame", "⌥⌘B"),
];
const EDITOR_SEPARATORS: &[usize] = &[3, 5, 9];

const SPLIT_MENU: &[(MenuItem, &str, &str)] = &[
    (MenuItem::SplitRight, "Split Right", "⌘K →"),
    (MenuItem::SplitLeft, "Split Left", "⌘K ←"),
    (MenuItem::SplitUp, "Split Up", "⌘K ↑"),
    (MenuItem::SplitDown, "Split Down", "⌘K ↓"),
];
const SPLIT_SEPARATORS: &[usize] = &[];

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
            Surface::Editor => (EDITOR_MENU, EDITOR_SEPARATORS),
            Surface::Split => (SPLIT_MENU, SPLIT_SEPARATORS),
        }
    }
}

fn surface_of(target: &MenuTarget) -> Surface {
    match target {
        MenuTarget::Tree { .. } => Surface::Tree,
        MenuTarget::Changes { .. } => Surface::Changes,
        MenuTarget::Tab { .. } => Surface::Tab,
        MenuTarget::Editor { .. } => Surface::Editor,
        MenuTarget::Split { .. } => Surface::Split,
    }
}

impl MenuItem {
    /// The project root is the workspace's identity: `project_order` and every
    /// cached path key off it, so the tree will not rename or delete that
    /// folder. Removing the project from the list is the one exception.
    fn applies_to_root(self) -> bool {
        !matches!(self, MenuItem::Rename | MenuItem::Trash | MenuItem::Delete)
    }

    /// Folders inside a project keep the disk actions; taking a project off
    /// the list only belongs on the project header.
    fn applies_to_entry(self) -> bool {
        !matches!(self, MenuItem::RemoveProject)
    }
}

/// The menu is a fixed grid so its height can be measured before it is built.
const MENU_WIDTH: f32 = 228.;
const MENU_ROW: f32 = 26.;

/// The indices into the target's own menu that it shows. The project root
/// drops the entries that would rename or delete it; a folder drops the
/// entry that would take a project off the list.
fn visible_menu_items(target: &MenuTarget) -> Vec<usize> {
    let root = matches!(target, MenuTarget::Tree { root: true, .. });
    surface_of(target)
        .menu()
        .0
        .iter()
        .enumerate()
        .filter(|(_, (item, _, _))| {
            if root {
                item.applies_to_root()
            } else {
                item.applies_to_entry()
            }
        })
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
        '→' => "right".into(),
        '←' => "left".into(),
        '↑' => "up".into(),
        '↓' => "down".into(),
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
    /// Parallel to `results`: the file's index in the scanned snapshot. The
    /// workers finish out of order, so streamed files are inserted by this.
    indices: Vec<usize>,
    rows: Vec<SearchRow>,
    /// Index into `rows`; always a `SearchRow::Hit` when the list is not empty.
    selected: usize,
    running: bool,
    /// The user stopped the scan; partial results stay on screen.
    stopped: bool,
    truncated: bool,
    error: Option<String>,
    show_replace: bool,
    scroll: UniformListScrollHandle,
}

/// Wait for typing to settle before scanning. Long enough to avoid scanning on
/// every keystroke, short enough to still feel live.
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(140);

/// Whether a path passes the project-search filter.
///
/// The field is a list of space-separated fragments matched against the path
/// relative to the project root, case-insensitively. Include fragments are
/// ORed; a fragment beginning with `!` excludes. Only excludes means "all but
/// these". This is deliberately smaller than a glob language: the field edits
/// on every keystroke and a path fragment covers the everyday case.
fn search_filter_matches(path: &Path, root: Option<&Path>, filter: &str) -> bool {
    let tokens: Vec<&str> = filter.split_whitespace().collect();
    if tokens.is_empty() {
        return true;
    }
    let relative = root
        .and_then(|root| path.strip_prefix(root).ok())
        .unwrap_or(path);
    let text = relative.to_string_lossy().to_lowercase();
    let mut included = false;
    let mut has_include = false;
    for token in tokens {
        let token = token.to_lowercase();
        if let Some(excluded) = token.strip_prefix('!') {
            if !excluded.is_empty() && text.contains(excluded) {
                return false;
            }
        } else {
            has_include = true;
            if text.contains(&token) {
                included = true;
            }
        }
    }
    !has_include || included
}

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
    split: Vec<diff::SplitRow>,
    untracked: bool,
    /// `None` once git has answered. Loading and failed are different from
    /// "no changes" and have to say so rather than showing an empty list.
    error: Option<String>,
    loading: bool,
    scroll: UniformListScrollHandle,
    /// Which hunk `F7` is sitting on.
    hunk: usize,
    /// Bumped per request, so a slow `git` for a file the user has moved on
    /// from does not land on top of the newer one.
    request: u64,
    /// Working tree and `base` texts, kept so a theme change can recolor
    /// without asking git again.
    old_text: Option<Arc<str>>,
    new_text: Option<Arc<str>>,
    color_request: u64,
    /// 1-based line → highlight runs, relative to that line.
    old_styles: HashMap<u32, Vec<(Range<usize>, HighlightStyle)>>,
    new_styles: HashMap<u32, Vec<(Range<usize>, HighlightStyle)>>,
}

/// Which way two panes sit. One axis at a time: Folio is still two panes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SplitAxis {
    #[default]
    Horizontal,
    Vertical,
}

/// Where the new pane goes, matching Zed's four split actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SplitDirection {
    Left,
    Right,
    Up,
    Down,
}

impl SplitDirection {
    fn axis(self) -> SplitAxis {
        match self {
            Self::Left | Self::Right => SplitAxis::Horizontal,
            Self::Up | Self::Down => SplitAxis::Vertical,
        }
    }

    /// The new pane is the first one (left or top).
    fn leading(self) -> bool {
        matches!(self, Self::Left | Self::Up)
    }
}

/// One editor column: its own strip and the file it is showing.
#[derive(Clone, Default)]
struct Pane {
    tabs: Vec<PathBuf>,
    active: Option<PathBuf>,
    /// Horizontal scroll of the strip. A pane keeps it so opening a tab can
    /// bring it into view without resetting where the user scrolled.
    tab_scroll: ScrollHandle,
}

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
    /// One or two editor columns. A file lives in at most one of them.
    panes: Vec<Pane>,
    focused: usize,
    /// Ignored while there is only one pane.
    split_axis: SplitAxis,
    /// Tabs the user has pinned, and buffers they have locked. Both are view
    /// state — neither reaches the disk — and both are per project, so
    /// switching projects keeps them.
    pinned: HashSet<PathBuf>,
    read_only: HashSet<PathBuf>,
    /// Files marked as read in the changes view. Purely view state; it never
    /// touches the file, and it survives a restart through the session.
    reviewed: HashSet<PathBuf>,
    git_status: HashMap<String, char>,
    files: Vec<PathBuf>,
    /// Files opened from quick open, the tree or a search, newest first. Used
    /// to order an empty quick-open list; it is view state, not disk state.
    recent_files: Vec<PathBuf>,
    /// Tabs this project has closed, newest last, with the pane they were in.
    /// `⌘⇧T` pops one; the history is view state and never reaches the disk.
    closed_tabs: Vec<(usize, PathBuf)>,
    /// `files`, lowercased and stripped to the file name, aligned by index.
    /// Quick open walks this on every keystroke; lowercasing 100,000 paths
    /// there would allocate once per candidate per key.
    file_names_lower: Vec<String>,
    indexing: bool,
    index_again: bool,
    git_running: bool,
    git_again: bool,
    diff_request: u64,
    /// Set while the active file is being shown as changes rather than code.
    diff: Option<DiffView>,
    /// Open buffers whose disk copy no longer matches `saved`. Clean buffers
    /// reload on their own; these are the dirty ones waiting for Reload / Keep.
    disk_notice: HashMap<PathBuf, DiskNotice>,
}

impl Default for Project {
    fn default() -> Self {
        Self {
            id: 0,
            image: None,
            workspace: None,
            directories: HashMap::new(),
            expanded: HashSet::new(),
            rows: Vec::new(),
            selected_row: 0,
            tree_scroll: UniformListScrollHandle::new(),
            documents: HashMap::new(),
            panes: vec![Pane::default()],
            focused: 0,
            split_axis: SplitAxis::Horizontal,
            pinned: HashSet::new(),
            read_only: HashSet::new(),
            reviewed: HashSet::new(),
            git_status: HashMap::new(),
            files: Vec::new(),
            recent_files: Vec::new(),
            closed_tabs: Vec::new(),
            file_names_lower: Vec::new(),
            indexing: false,
            index_again: false,
            git_running: false,
            git_again: false,
            diff_request: 0,
            diff: None,
            disk_notice: HashMap::new(),
        }
    }
}

impl Project {
    fn pane(&self) -> &Pane {
        self.panes
            .get(self.focused)
            .or_else(|| self.panes.first())
            .expect("a project always has a pane")
    }

    fn pane_mut(&mut self) -> &mut Pane {
        let index = self.focused.min(self.panes.len().saturating_sub(1));
        &mut self.panes[index]
    }

    /// Replace the quick-open index and the lowercase names that go with it.
    fn set_files(&mut self, files: Vec<PathBuf>) {
        self.file_names_lower = files.iter().map(|path| name(path).to_lowercase()).collect();
        self.files = files;
    }

    fn tabs(&self) -> &[PathBuf] {
        &self.pane().tabs
    }

    fn active(&self) -> Option<&PathBuf> {
        self.pane().active.as_ref()
    }

    fn pane_index_of(&self, path: &Path) -> Option<usize> {
        self.panes
            .iter()
            .position(|pane| pane.tabs.iter().any(|tab| tab == path))
    }

    fn showing(&self, path: &Path) -> bool {
        self.panes.iter().any(|pane| {
            pane.active
                .as_ref()
                .is_some_and(|active| active.as_path() == path)
        })
    }

    /// Put this file on screen. If another column already has it, look there
    /// instead of copying the tab.
    fn show_path(&mut self, path: PathBuf) {
        if let Some(index) = self.pane_index_of(&path) {
            self.focused = index;
            self.pane_mut().active = Some(path);
            return;
        }
        let pane = self.pane_mut();
        if !pane.tabs.contains(&path) {
            pane.tabs.push(path.clone());
        }
        pane.active = Some(path);
    }
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
    /// What `⇧⌘R` repeats. Kept whole, working directory included, so a
    /// rerun does not depend on the file still being open.
    last_run: Option<run::Command>,
    /// Play in the title bar lives on Folio, so a busy flip has to notify
    /// this window, not only the terminal view. One per view, in step.
    _watches: Vec<Subscription>,
}

/// A release newer than the running build. On Windows it carries the feed
/// entry so the badge can go straight to the installer prompt; on macOS
/// Sparkle holds the details and the badge just opens its window.
#[derive(Clone, Debug, PartialEq, Eq)]
struct AvailableUpdate {
    version: String,
    #[cfg(target_os = "windows")]
    feed: windows_update::Feed,
}

/// One floating status card. Drawn in Folio's own chrome — the same surface
/// and hairline as a tab preview — not the component-kit Alert palette.
#[derive(Clone, PartialEq, Eq)]
struct Notice {
    kind: NoticeKind,
    title: Option<&'static str>,
    text: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NoticeKind {
    Info,
    Success,
    Warning,
    Error,
}

impl Notice {
    fn error(text: impl Into<String>) -> Self {
        Self {
            kind: NoticeKind::Error,
            title: Some("Error"),
            text: text.into(),
        }
    }

    fn success(text: impl Into<String>) -> Self {
        Self {
            kind: NoticeKind::Success,
            title: None,
            text: text.into(),
        }
    }

    fn warning(text: impl Into<String>) -> Self {
        Self {
            kind: NoticeKind::Warning,
            title: Some("Warning"),
            text: text.into(),
        }
    }

    fn info(title: &'static str, text: impl Into<String>) -> Self {
        Self {
            kind: NoticeKind::Info,
            title: Some(title),
            text: text.into(),
        }
    }

    fn icon(&self) -> IconName {
        match self.kind {
            NoticeKind::Info => IconName::Info,
            NoticeKind::Success => IconName::CircleCheck,
            NoticeKind::Warning => IconName::TriangleAlert,
            NoticeKind::Error => IconName::CircleX,
        }
    }

    /// Icon colour. Sage, brick, ochre — the same register as the rest of
    /// the chrome, not the kit's neons.
    fn mark(&self, cx: &App) -> Hsla {
        let dark = cx.theme().is_dark();
        match self.kind {
            NoticeKind::Success => cx.theme().accent_foreground,
            NoticeKind::Error => rgb(if dark { 0xC98982 } else { 0xA35B52 }).into(),
            NoticeKind::Warning => rgb(if dark { 0xC4B07A } else { 0x8A7340 }).into(),
            NoticeKind::Info => cx.theme().muted_foreground,
        }
    }
}

/// A way for a Markdown link click to reach the view that owns the preview.
/// `TextView::on_link_click` runs with only `&mut App`, so the weak view is
/// parked in a global instead of being captured by the (Send + Sync) handler.
struct MarkdownLinkTarget(WeakEntity<Folio>);

impl Global for MarkdownLinkTarget {}

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
    sidebar_kind: SidebarKind,
    /// Highlighted row in the changes list.
    changes_selected: usize,
    diff_layout: DiffLayout,
    diff_against: DiffAgainst,
    /// Cached name of the branch `DiffAgainst::Branch` uses.
    diff_branch: Option<String>,
    /// The far-left icon rail. Independent of the file tree: hiding the
    /// rail does not close the tree, and `⌘B` does not hide the rail.
    activity_bar: bool,
    sidebar_width: f32,
    resizing: bool,
    /// Share of the editor column the first pane takes when split.
    pane_ratio: f32,
    resizing_pane: bool,
    /// The terminal drawer: whether it shows, how tall, whether its top
    /// edge is being dragged, and each project's terminal tabs.
    terminals: HashMap<PathBuf, TerminalTabs>,
    terminal_open: bool,
    terminal_height: f32,
    /// The dock fills a column to the editor's right instead of a drawer
    /// under it. View state: the children keep running either way.
    terminal_right: bool,
    terminal_width: f32,
    resizing_terminal: bool,
    run_after_save: Option<Option<usize>>,
    generation: u64,
    open_request: u64,
    query: Entity<InputState>,
    /// Shown over the launcher (or the workspace) to paste a GitHub URL.
    clone_url: Entity<InputState>,
    clone_open: bool,
    cloning: bool,
    panel: Option<Panel>,
    matches: Vec<PathBuf>,
    match_selected: usize,
    /// The open right-click menu, if any.
    menu: Option<ContextMenu>,
    /// Zed's `⌘K` then an arrow. Cleared by the next key.
    split_chord: bool,
    /// Where a dragged tab would land, while one is in flight. Drawn as a
    /// caret between the tabs.
    /// Which pane and which slot a dragged tab is hovering.
    tab_drop: Option<(usize, usize)>,
    /// Where Cut / Copy put the entry, and whether it was a cut.
    clipboard: Option<FileClipboard>,
    /// The tree row being named, if any.
    editing: Option<InlineEdit>,
    search_query: Entity<InputState>,
    /// Path fragments that include or exclude files from a project search.
    search_filter: Entity<InputState>,
    replace_query: Entity<InputState>,
    search: SearchState,
    /// Loaded once at startup and written back whenever the panel changes it.
    settings: Settings,
    /// Last wrap mode pushed into open editors, so a settings change can
    /// catch up on the next frame when we have a window.
    applied_wrap: bool,
    /// First frame after launch has been logged, if `FOLIO_PERF` is set.
    logged_ready: bool,
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
    notice: Option<Notice>,
    /// Bumped per announcement so a new card can replay the slide-in.
    notice_id: u64,
    /// The card is sliding back off the right edge and should unmount after.
    notice_leaving: bool,
    /// A newer release the feed advertised. The title bar shows it until
    /// the user opens the updater from there.
    available_update: Option<AvailableUpdate>,
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
    written_unsaved: UnsavedSnapshot,
    /// The revision of each dirty buffer at the time `written_unsaved` was
    /// captured. A matching pair means there is nothing new to write.
    written_revisions: BTreeMap<PathBuf, u64>,
    recovery_error: Option<String>,
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

/// How many closed tabs a project remembers for `⌘⇧T`.
const CLOSED_TABS_LIMIT: usize = 32;

/// What the caret's line was last written by, for the file being read.
struct BlameView {
    path: PathBuf,
    blame: blame::Blame,
}

type UnsavedSnapshot = BTreeMap<PathBuf, Rope>;

/// The two files a session lives in, and what has changed in them.
struct Writes {
    session_file: PathBuf,
    unsaved_file: PathBuf,
    session: Option<session::Session>,
    unsaved: Option<UnsavedSnapshot>,
    /// The revisions that produced `unsaved`, recorded once the write lands.
    revisions: BTreeMap<PathBuf, u64>,
}

impl Writes {
    fn is_empty(&self) -> bool {
        self.session.is_none() && self.unsaved.is_none()
    }

    /// Only successful writes are acknowledged; failed snapshots remain pending.
    fn write(mut self) -> (Writes, Option<String>) {
        let mut errors = Vec::new();
        if let Some(session) = &self.session
            && let Err(error) = session::Session::save(session, &self.session_file)
        {
            errors.push(format!("Session: {error}"));
            self.session = None;
        }
        if let Some(unsaved) = &self.unsaved {
            let text = unsaved
                .iter()
                .map(|(path, text)| (path.clone(), text.to_string()))
                .collect();
            if let Err(error) = session::save_unsaved(&self.unsaved_file, &text) {
                errors.push(format!("Recovery: {error}"));
                self.unsaved = None;
            }
        }
        let error = (!errors.is_empty()).then(|| errors.join("; "));
        (self, error)
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
    /// Files that were already reviewed in the last session.
    Reviewed(Vec<PathBuf>),
    /// The project's quick-open history from the last session.
    RecentFiles(Vec<PathBuf>),
    /// Open an empty second pane so the next files land there.
    Split { vertical: bool },
    /// After the right pane is restored, look at the left one again.
    FocusPane(usize),
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
    if let Some((image, render)) = project.image.take() {
        project.image = Some((moved(&image, from, to).unwrap_or(image), render));
    }
    for pane in &mut project.panes {
        if let Some(active) = pane.active.take() {
            pane.active = Some(moved(&active, from, to).unwrap_or(active));
        }
        for tab in &mut pane.tabs {
            if let Some(renamed) = moved(tab, from, to) {
                *tab = renamed;
            }
        }
    }
    let remap_set = |set: HashSet<PathBuf>| {
        set.into_iter()
            .map(|path| moved(&path, from, to).unwrap_or(path))
            .collect()
    };
    project.closed_tabs = std::mem::take(&mut project.closed_tabs)
        .into_iter()
        .map(|(pane, path)| (pane, moved(&path, from, to).unwrap_or(path)))
        .collect();
    project.expanded = remap_set(std::mem::take(&mut project.expanded));
    project.pinned = remap_set(std::mem::take(&mut project.pinned));
    project.read_only = remap_set(std::mem::take(&mut project.read_only));
    project.reviewed = remap_set(std::mem::take(&mut project.reviewed));
    project.recent_files = std::mem::take(&mut project.recent_files)
        .into_iter()
        .map(|path| moved(&path, from, to).unwrap_or(path))
        .collect();
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
                Some(Notice::error(format!("Could not read settings: {error}"))),
            ),
        };
        let appearance = window.appearance();
        let main_bounds = window.window_bounds().get_bounds();
        sync_appearance(appearance, &settings, Some(window), cx);
        let appearance_subscription =
            cx.observe_window_appearance(window, |this: &mut Self, window, cx| {
                this.appearance = window.appearance();
                sync_appearance(this.appearance, &this.settings, Some(window), cx);
                this.recolor_diff(cx);
                this.refresh_all_brackets(cx);
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
        let search_filter =
            cx.new(|cx| InputState::new(window, cx).placeholder("Files to include, !exclude"));
        let search_filter_subscription = cx.subscribe_in(
            &search_filter,
            window,
            |this: &mut Self, _, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) && this.panel == Some(Panel::Search) {
                    this.start_search(cx);
                    cx.notify();
                }
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
        let clone_url = cx.new(|cx| {
            InputState::new(window, cx).placeholder("owner/repo or https://github.com/…")
        });
        let clone_subscription = cx.subscribe_in(
            &clone_url,
            window,
            |this: &mut Self, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.confirm_clone_url(window, cx);
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
            sidebar_kind: SidebarKind::Tree,
            changes_selected: 0,
            diff_layout: DiffLayout::Unified,
            diff_against: DiffAgainst::Head,
            diff_branch: None,
            activity_bar: settings.activity_bar,
            sidebar_width: 240.,
            resizing: false,
            pane_ratio: 0.5,
            resizing_pane: false,
            terminals: HashMap::new(),
            terminal_open: false,
            terminal_height: 260.,
            terminal_right: false,
            terminal_width: 420.,
            resizing_terminal: false,
            run_after_save: None,
            generation: 0,
            open_request: 0,
            query,
            clone_url,
            clone_open: false,
            cloning: false,
            panel: None,
            matches: vec![],
            match_selected: 0,
            menu: None,
            split_chord: false,
            tab_drop: None,
            clipboard: None,
            editing: None,
            search_query,
            search_filter,
            replace_query,
            search: SearchState::default(),
            settings,
            applied_wrap: false,
            logged_ready: false,
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
            notice: settings_error,
            notice_id: 0,
            notice_leaving: false,
            available_update: None,
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
            written_unsaved: UnsavedSnapshot::new(),
            written_revisions: BTreeMap::new(),
            recovery_error: None,
            written_session: session::Session::default(),
            disk_watch: watch::DiskWatch::default(),
            quiet_writes: HashMap::new(),
            _subscriptions: vec![
                subscription,
                search_subscription,
                search_filter_subscription,
                replace_subscription,
                clone_subscription,
                bounds_subscription,
                appearance_subscription,
            ],
        };
        cx.set_global(MarkdownLinkTarget(cx.entity().downgrade()));
        this.refresh_recent(RecentAction::Load, cx);
        this.tree_focus.focus(window, cx);
        // Packaged builds load Sparkle from the app bundle. `cargo run`
        // has no framework, so this is a no-op there.
        crate::sparkle::start();
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
        self.schedule_update_check(window, cx);
    }

    fn request(&mut self, next: Next, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || self.prompting || self.project_loading || self.cloning {
            return;
        }
        self.clone_open = false;
        // Invalidate a pending file read before changing project or opening a modal.
        self.open_request += 1;
        self.loading = false;
        // By reference: one arm needs the path, and `next` is still wanted
        // for the prompt and the follow-up.
        let dirty = match &next {
            Next::Picker | Next::NewProject | Next::CloneFromGithub | Next::Open(_) => 0,
            // Only the files being closed are at stake, so only their own
            // edits are counted.
            Next::CloseTabs(paths) => paths.iter().filter(|path| self.is_dirty(path)).count(),
            Next::Close => self.project.documents.values().filter(|d| d.dirty).count(),
            // SaveAll is dispatched straight to `save_documents`; it never
            // needs a close prompt, so it is not counted here.
            Next::SaveAll => 0,
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
            .active()
            .is_some_and(|active| !closing.contains(active));
        // Where the view lands if the active tab is one of them: the first tab
        // after the set that survives, else the last one before it.
        let replacement = self.project.active().cloned().and_then(|active| {
            let tabs = self.project.tabs();
            let index = tabs.iter().position(|tab| tab == &active)?;
            tabs[index..]
                .iter()
                .find(|tab| !closing.contains(tab))
                .or_else(|| {
                    tabs[..index]
                        .iter()
                        .rev()
                        .find(|tab| !closing.contains(tab))
                })
                .cloned()
        });
        for path in &closing {
            if let Some((pane, _)) = self
                .project
                .panes
                .iter()
                .enumerate()
                .find(|(_, pane)| pane.tabs.iter().any(|tab| tab == path))
            {
                self.project
                    .closed_tabs
                    .retain(|(_, closed)| closed != path);
                self.project.closed_tabs.push((pane, path.clone()));
                if self.project.closed_tabs.len() > CLOSED_TABS_LIMIT {
                    self.project.closed_tabs.remove(0);
                }
            }
            for pane in &mut self.project.panes {
                pane.tabs.retain(|tab| tab != path);
                if pane.active.as_ref() == Some(path) {
                    pane.active = None;
                }
            }
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
        self.prune_empty_panes();
        if active_survives {
            cx.notify();
            return;
        }
        match replacement {
            Some(next) => self.open_file(next, window, cx),
            None => {
                self.project.pane_mut().active = None;
                self.open_request += 1;
                self.loading = false;
                self.focus_editor(window, cx);
                self.update_title(window);
                cx.notify();
            }
        }
    }

    /// Drop a pane that has no tabs, and look at the one that remains.
    fn prune_empty_panes(&mut self) {
        if self.project.panes.len() <= 1 {
            if self.project.panes.is_empty() {
                self.project.panes.push(Pane::default());
            }
            self.project.focused = 0;
            self.project.split_axis = SplitAxis::Horizontal;
            return;
        }
        let focused_empty = self
            .project
            .panes
            .get(self.project.focused)
            .is_some_and(|pane| pane.tabs.is_empty());
        self.project.panes.retain(|pane| !pane.tabs.is_empty());
        if self.project.panes.is_empty() {
            self.project.panes.push(Pane::default());
        }
        if focused_empty || self.project.focused >= self.project.panes.len() {
            self.project.focused = self.project.panes.len() - 1;
        }
        if self.project.panes.len() <= 1 {
            self.project.split_axis = SplitAxis::Horizontal;
        }
    }

    /// Cut a second pane in `direction`. Two tabs become one each; one file
    /// leaves the new pane empty. Already split: turn the pair that way so
    /// the other pane sits on that side of the focused one.
    fn split_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.split_in(SplitDirection::Right, window, cx);
    }

    fn split_in(&mut self, direction: SplitDirection, window: &mut Window, cx: &mut Context<Self>) {
        if self.project.workspace.is_none() {
            return;
        }
        self.project.split_axis = direction.axis();
        if self.project.panes.len() > 1 {
            let focused = self.project.focused.min(1);
            let other_after = focused == 0;
            let want_other_after = !direction.leading();
            if other_after != want_other_after {
                self.project.panes.swap(0, 1);
                self.project.focused = 1 - focused;
            }
            self.focus_editor(window, cx);
            cx.notify();
            return;
        }
        let other = {
            let pane = self.project.pane();
            let Some(active) = pane.active.clone() else {
                if direction.leading() {
                    self.project.panes.insert(0, Pane::default());
                    self.project.focused = 0;
                } else {
                    self.project.panes.push(Pane::default());
                    self.project.focused = 1;
                }
                cx.notify();
                return;
            };
            let index = pane.tabs.iter().position(|tab| tab == &active);
            index.and_then(|index| {
                pane.tabs
                    .get(index.saturating_sub(1))
                    .filter(|tab| *tab != &active)
                    .cloned()
                    .or_else(|| {
                        pane.tabs
                            .get(index + 1)
                            .filter(|tab| *tab != &active)
                            .cloned()
                    })
            })
        };
        let mut created = Pane::default();
        if let Some(path) = other {
            let source = self.project.pane_mut();
            source.tabs.retain(|tab| tab != &path);
            created.tabs.push(path.clone());
            created.active = Some(path);
        }
        if direction.leading() {
            self.project.panes.insert(0, created);
            self.project.focused = 1;
            if self.project.panes[0].tabs.is_empty() {
                self.project.focused = 0;
            }
        } else {
            self.project.panes.push(created);
            if self.project.panes[1].tabs.is_empty() {
                self.project.focused = 1;
            }
        }
        self.focus_editor(window, cx);
        cx.notify();
    }

    fn join_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.project.panes.len() < 2 {
            return;
        }
        let right = self.project.panes.remove(1);
        let left = &mut self.project.panes[0];
        for path in right.tabs {
            if !left.tabs.contains(&path) {
                left.tabs.push(path);
            }
        }
        self.project.focused = 0;
        self.project.split_axis = SplitAxis::Horizontal;
        self.reorder_tabs();
        self.focus_editor(window, cx);
        cx.notify();
    }

    fn focus_pane(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.project.panes.len() {
            return;
        }
        self.project.focused = index;
        self.scroll_to_active_tab();
        self.focus_editor(window, cx);
        self.update_title(window);
        self.follow_diff(window, cx);
        cx.notify();
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
            Next::SaveAll => {}
            Next::Open(path) => self.open_project(path, window, cx),
            Next::CloneFromGithub => self.show_clone_prompt(window, cx),
            Next::NewProject => {
                // A save panel, so the name and the parent folder are one
                // choice. Folio only `mkdir`s what comes back; it does not
                // write a template or run `git init`.
                let picker = cx.prompt_for_new_path(&home_dir(), Some("untitled"));
                self.prompting = true;
                cx.notify();
                cx.spawn_in(window, async move |this, cx| {
                    let result = picker.await;
                    let _ = this.update_in(cx, |this, window, cx| {
                        this.prompting = false;
                        match result {
                            Ok(Ok(Some(path))) => this.create_empty_project(path, window, cx),
                            Ok(Err(e)) => this.error(e.to_string(), cx),
                            _ => {}
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
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
        self.clone_open = false;
        self.cloning = false;
        self.resizing = false;
        self.resizing_pane = false;
        self.split_chord = false;
        self.clear_notice();
    }

    /// Supersede any running scan and drop the results it produced.
    fn cancel_search(&mut self) {
        self.search_request.fetch_add(1, Ordering::Relaxed);
        self.search.running = false;
        self.search.truncated = false;
        self.search.error = None;
        self.search.results.clear();
        self.search.indices.clear();
        self.search.rows.clear();
        self.search.selected = 0;
        self.search.stopped = false;
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
        let Some(document) = self.project.documents.get_mut(&target) else {
            return;
        };
        // A jump is a place in the source, so drop the rendered view.
        document.preview = false;
        let editor = document.editor.clone();
        editor.update(cx, |state, cx| {
            state.base_state().clone().update(cx, |base, cx| {
                base.set_cursor_position(position, window, cx)
            });
        });
    }

    /// Preview keeps the editor off the focus path so typing cannot land in
    /// a buffer that is not on screen.
    fn focus_open_file(
        &mut self,
        path: &Path,
        editor: &Entity<EditorState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .project
            .documents
            .get(path)
            .is_some_and(|document| document.preview)
        {
            self.tree_focus.focus(window, cx);
        } else {
            editor.focus_handle(cx).focus(window, cx);
        }
    }

    fn set_markdown_preview(&mut self, preview: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.project.active().cloned() else {
            return;
        };
        let Some(document) = self.project.documents.get_mut(&path) else {
            return;
        };
        if document.large || !buffer::is_markdown(&path) {
            return;
        }
        document.preview = preview;
        let editor = document.editor.clone();
        self.focus_open_file(&path, &editor, window, cx);
        cx.notify();
    }

    fn toggle_markdown_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.project.active().cloned() else {
            return;
        };
        let Some(preview) = self
            .project
            .documents
            .get(&path)
            .map(|document| document.preview)
        else {
            return;
        };
        self.set_markdown_preview(!preview, window, cx);
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

    fn is_tree_directory(&self, path: &Path) -> bool {
        std::iter::once(&self.project)
            .chain(self.parked.iter())
            .any(|project| {
                project.directories.contains_key(path)
                    || project
                        .rows
                        .iter()
                        .any(|row| row.entry.path == path && row.entry.kind == EntryKind::Directory)
                    || project
                        .workspace
                        .as_ref()
                        .is_some_and(|workspace| workspace.root == path)
            })
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

    /// Sparkle's update window on a packaged Mac app. On Windows, the
    /// installer published to R2. `cargo run` has neither, so it says so.
    fn check_for_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(target_os = "macos")]
        {
            if crate::sparkle::check() {
                return;
            }
            let _ = window;
            self.inform("Updates", "Updates ship in the packaged Mac app.", cx);
        }
        #[cfg(target_os = "windows")]
        self.check_windows_update(true, window, cx);
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = window;
            self.inform("Updates", "Updates ship in the packaged Mac app.", cx);
        }
    }

    /// Quiet check a little after launch, and once a day after that. A hit
    /// only lights the title-bar badge; nothing pops up until it is clicked.
    fn schedule_update_check(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        #[cfg(target_os = "windows")]
        if !windows_update::installed() {
            return;
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (window, cx);
            return;
        }
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(8)).await;
            loop {
                let keep_going = this
                    .update_in(cx, |this, window, cx| {
                        this.check_update_silently(window, cx);
                    })
                    .is_ok();
                if !keep_going {
                    return;
                }
                cx.background_executor()
                    .timer(Duration::from_secs(24 * 60 * 60))
                    .await;
            }
        })
        .detach();
    }

    #[cfg(target_os = "windows")]
    fn check_update_silently(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.check_windows_update(false, window, cx);
    }

    /// Ask Sparkle for the feed without its UI, then watch for the answer.
    #[cfg(target_os = "macos")]
    fn check_update_silently(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.available_update.is_some() || !crate::sparkle::begin_background_check() {
            return;
        }
        let running = windows_update::current_build();
        cx.spawn_in(window, async move |this, cx| {
            // Sparkle answers on the main thread; a check that never reports
            // (offline, or a session already open) is given up on after a minute.
            for _ in 0..120 {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                let Some(result) = crate::sparkle::background_result() else {
                    continue;
                };
                if let Some(found) = result
                    && found.build > running
                {
                    let _ = this.update_in(cx, |this, _, cx| {
                        this.available_update = Some(AvailableUpdate {
                            version: found.version,
                        });
                        cx.notify();
                    });
                }
                return;
            }
        })
        .detach();
    }

    /// The title-bar badge: hand over to the platform's updater.
    fn open_available_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(update) = self.available_update.clone() else {
            return;
        };
        #[cfg(target_os = "macos")]
        {
            let _ = (update, window);
            if !crate::sparkle::check() {
                self.inform("Updates", "Updates ship in the packaged Mac app.", cx);
            }
        }
        #[cfg(target_os = "windows")]
        self.prompt_windows_update(update.feed, window, cx);
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let _ = (update, window);
            self.inform("Updates", "Updates ship in the packaged Mac app.", cx);
        }
    }

    #[cfg(target_os = "windows")]
    fn check_windows_update(&mut self, user: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !windows_update::installed() {
            if user {
                self.inform(
                    "Updates",
                    "Updates ship in the packaged Windows installer.",
                    cx,
                );
            }
            return;
        }
        if self.saving || self.prompting || self.project_loading {
            return;
        }
        if user {
            self.status("Updates", "Checking for updates.", cx);
        }
        let running = windows_update::current_build();
        let task = cx.background_executor().spawn(async move {
            windows_update::fetch_feed()
                .map(|feed| windows_update::is_newer(&feed, running).then_some(feed))
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(None) => {
                    if user {
                        this.inform("Updates", "You're up to date.", cx);
                    } else {
                        this.clear_notice();
                    }
                }
                Ok(Some(feed)) => {
                    this.clear_notice();
                    this.available_update = Some(AvailableUpdate {
                        version: feed.version.clone(),
                        feed: feed.clone(),
                    });
                    cx.notify();
                    if user {
                        this.prompt_windows_update(feed, window, cx);
                    }
                }
                Err(error) => {
                    if user {
                        this.error(format!("Could not check for updates: {error}"), cx);
                    }
                }
            });
        })
        .detach();
    }

    #[cfg(target_os = "windows")]
    fn prompt_windows_update(
        &mut self,
        feed: windows_update::Feed,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.prompting {
            return;
        }
        self.prompting = true;
        cx.notify();
        let title = format!("Folio {} is available", feed.version);
        let detail = format!(
            "Build {} can be installed now. Folio will close, then reopen after the installer finishes.",
            feed.build
        );
        let answer = window.prompt(
            PromptLevel::Info,
            &title,
            Some(&detail),
            &["Install", "Later"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = answer.await.unwrap_or(1);
            let _ = this.update_in(cx, |this, window, cx| {
                this.prompting = false;
                if answer != 0 {
                    return;
                }
                this.download_windows_update(feed, window, cx);
            });
        })
        .detach();
    }

    #[cfg(target_os = "windows")]
    fn download_windows_update(
        &mut self,
        feed: windows_update::Feed,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dirty = std::iter::once(&self.project)
            .chain(self.parked.iter())
            .flat_map(|p| p.documents.values())
            .filter(|d| d.dirty)
            .count();
        if dirty > 0 {
            self.inform(
                "Updates",
                "Save your files before installing the update.",
                cx,
            );
            return;
        }
        if self.saving || self.prompting || self.project_loading {
            return;
        }
        self.status("Updates", "Downloading the installer.", cx);
        let task = cx
            .background_executor()
            .spawn(async move { windows_update::download_installer(&feed) });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(path) => {
                    if let Err(error) = windows_update::start_installer(&path) {
                        this.error(format!("Could not start the installer: {error}"), cx);
                        return;
                    }
                    this.perform(Next::Quit, window, cx);
                }
                Err(error) => this.error(format!("Could not download the update: {error}"), cx),
            });
        })
        .detach();
    }

    fn show_about(&mut self, cx: &mut Context<Self>) {
        if let Some(window) = self.about_window
            && window
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return;
        }
        let bounds = WindowBounds::centered(size(px(360.), px(248.)), cx);
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
        self.refresh_all_brackets(cx);
        self.sync_cursor_animation(cx);
        // These two settings are the stored state the toggles write back.
        self.sidebar = self.settings.sidebar;
        self.activity_bar = self.settings.activity_bar;
        // Editors pick the new wrap up on the next frame of the main window.
        if ignored_changed {
            self.sync_disk_watch();
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

    /// `⌘+`, `⌘-` and `⌘0` on the code face. The theme carries the new size
    /// and every open editor repaints with it on the next frame.
    fn zoom_code(&mut self, step: f32, reset: bool, cx: &mut Context<Self>) {
        let next = if reset {
            Settings::default().code_font_size
        } else {
            self.settings.code_font_size + step
        };
        let next = next.clamp(settings::CODE_FONT_SIZE.0, settings::CODE_FONT_SIZE.1);
        if (next - self.settings.code_font_size).abs() < f32::EPSILON {
            return;
        }
        self.settings.code_font_size = next;
        self.apply_settings(cx);
    }

    fn sync_soft_wrap(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let wrap = self.settings.soft_wrap;
        let editors: Vec<_> = std::iter::once(&self.project)
            .chain(self.parked.iter())
            .flat_map(|project| {
                project
                    .documents
                    .values()
                    .map(|document| document.editor.clone())
            })
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

    fn sync_cursor_animation(&mut self, cx: &mut Context<Self>) {
        let enabled = self.settings.cursor_animation;
        let editors: Vec<_> = std::iter::once(&self.project)
            .chain(self.parked.iter())
            .flat_map(|project| {
                project
                    .documents
                    .values()
                    .map(|document| document.editor.clone())
            })
            .collect();
        for editor in editors {
            editor.update(cx, |state, cx| {
                state
                    .base_state()
                    .update(cx, |base, cx| base.set_cursor_animation(enabled, cx));
            });
        }
    }

    /// The ignore rules changed, so every cached listing is suspect. One
    /// background pass re-reads the folders the tree is currently holding.
    fn reload_tree(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.project.workspace.as_ref().map(|w| w.root.clone()) else {
            return;
        };
        let project_id = self.project.id;
        let mut wanted: Vec<PathBuf> = self.project.expanded.iter().cloned().collect();
        wanted.push(root.clone());
        wanted.sort();
        wanted.dedup();
        let ignored = self.settings.ignored.clone();
        let task = cx.background_executor().spawn(async move {
            let mut cache = HashMap::new();
            for dir in wanted {
                if let Ok(children) = tree::children_in(&root, &dir, &ignored) {
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

    /// Move `path` to the front of this project's quick-open history. The
    /// history is capped so it stays a handful of entries, not a second index.
    fn touch_recent_file(&mut self, path: &Path) {
        const LIMIT: usize = 20;
        self.project.recent_files.retain(|recent| recent != path);
        self.project.recent_files.insert(0, path.to_path_buf());
        self.project.recent_files.truncate(LIMIT);
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
        // A file already in the other column moves the focus there.
        self.touch_recent_file(&path);
        self.project.show_path(path.clone());
        self.scroll_to_active_tab();
        if self
            .project
            .image
            .as_ref()
            .is_some_and(|(image_path, _)| image_path == &path)
        {
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
            self.loading = false;
            self.apply_goto(&path, window, cx);
            self.apply_recovered(&path, window, cx);
            self.focus_open_file(&path, &editor, window, cx);
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
        let started = Instant::now();
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
                        if perf_on() {
                            eprintln!(
                                "folio_open_file_ms={:.1} kind=image rss_mb={:.1}",
                                perf_ms(started),
                                rss_mb().unwrap_or(f64::NAN)
                            );
                        }
                        this.project.image = Some((path.clone(), image));
                        this.project.show_path(path);
                        this.clear_notice();
                        this.tree_focus.focus(window, cx);
                        this.update_title(window);
                        this.follow_diff(window, cx);
                    }
                    Ok((path, Content::Text(text), indent)) => {
                        if perf_on() {
                            eprintln!(
                                "folio_open_file_ms={:.1} bytes={} rss_mb={:.1}",
                                perf_ms(started),
                                text.len(),
                                rss_mb().unwrap_or(f64::NAN)
                            );
                        }
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
                            let typeface = this.code_typeface();
                            let size = px(this.settings.code_font_size);
                            let wrap = this.settings.soft_wrap;
                            let cursor_animation = this.settings.cursor_animation;
                            state.base_state().update(cx, |base, cx| {
                                base.set_soft_wrap(wrap, window, cx);
                                base.set_font(typeface, size);
                                base.set_cursor_animation(cursor_animation, cx);
                            });
                        });
                        let subscription = Self::subscribe_document(&editor, window, cx);
                        let brackets = Self::attach_brackets(&editor, cx);
                        let jumping = this
                            .goto
                            .as_ref()
                            .is_some_and(|(target, _)| target == &path);
                        let preview = !large && !jumping && buffer::is_markdown(&path);
                        this.project.documents.insert(
                            path.clone(),
                            Document {
                                editor: editor.clone(),
                                saved,
                                dirty: false,
                                revision: 0,
                                large,
                                preview,
                                can_run: None,
                                brackets,
                                _subscription: subscription,
                            },
                        );
                        this.refresh_brackets(&editor, cx);
                        this.refresh_runnables(&editor, cx);
                        this.apply_goto(&path, window, cx);
                        this.focus_open_file(&path, &editor, window, cx);
                        this.project.show_path(path.clone());
                        this.clear_notice();
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
            document.revision = document.revision.wrapping_add(1);
        }
        self.refresh_brackets(&editor, cx);
        self.refresh_runnables(&editor, cx);
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
        let title = if let Some(path) = self.project.active() {
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
        let all_projects = matches!(next, Some(Next::Quit) | Some(Next::SaveAll));
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
                            && (next.is_some() || project.active() == Some(path))
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
                                doc.dirty = doc.editor.read(cx).is_modified(&doc.saved);
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
                    if let Some(line) = this.run_after_save.take() {
                        this.start_run(line, window, cx);
                    } else if let Some(next) = next {
                        this.request(next, window, cx);
                    } else {
                        this.toast("Saved", cx);
                        this.refresh_git(cx);
                    }
                } else {
                    this.run_after_save = None;
                    this.error(errors.join("\n"), cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn filter(&mut self, cx: &App) {
        let query = self.query.read(cx).value();
        let query = query.trim();
        // `:123` is a line jump, not a file search; the result list is hidden
        // while it is on, so there is nothing to filter.
        if query.starts_with(':') {
            self.matches.clear();
        } else if query.is_empty() {
            // No query is the recent-files list, then the index in path order
            // for the rest. A recent path that has been deleted is dropped.
            let recent = self
                .project
                .recent_files
                .iter()
                .filter(|path| self.project.files.binary_search(path).is_ok())
                .take(100)
                .cloned()
                .collect::<Vec<_>>();
            let mut seen = recent.iter().cloned().collect::<HashSet<_>>();
            self.matches = recent;
            for path in &self.project.files {
                if self.matches.len() >= 100 {
                    break;
                }
                if seen.insert(path.clone()) {
                    self.matches.push(path.clone());
                }
            }
        } else {
            // The candidate names were lowercased when the index was built, so
            // this allocates once for the query rather than once per file.
            let wanted = query.to_lowercase();
            debug_assert_eq!(
                self.project.files.len(),
                self.project.file_names_lower.len()
            );
            let recent = self
                .project
                .recent_files
                .iter()
                .enumerate()
                .map(|(rank, path)| (path.as_path(), rank as i32))
                .collect::<HashMap<_, _>>();
            // A bounded linear scan: every file gets a name score, plus a mild
            // preference for shallow paths and for files just opened. Sorting
            // the whole index is fine at 100k entries; the result is capped.
            let mut scored = Vec::new();
            for (index, path) in self.project.files.iter().enumerate() {
                let Some(candidate) = self.project.file_names_lower.get(index) else {
                    continue;
                };
                let Some(base) = tree::fuzzy_score(&wanted, candidate) else {
                    continue;
                };
                let depth = path.components().count() as i32;
                let recent_bonus = recent
                    .get(path.as_path())
                    .map(|rank| 240 - rank * 12)
                    .unwrap_or(0);
                scored.push((base - depth * 4 + recent_bonus, path.clone()));
            }
            scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            self.matches = scored.into_iter().take(100).map(|(_, path)| path).collect();
        }
        self.match_selected = 0;
        self.quick_scroll.scroll_to_item(0, ScrollStrategy::Top);
    }

    fn show_quick_open(&mut self, line: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.project.workspace.is_none() || self.project_loading || self.prompting {
            return;
        }
        self.clone_open = false;
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
        self.clone_open = false;
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
            .active()
            .and_then(|p| self.project.documents.get(p))
        {
            doc.editor.focus_handle(cx).focus(window, cx);
        } else {
            self.tree_focus.focus(window, cx);
        }
        cx.notify();
    }

    /// `⌘B` and the first rail icon: show or hide the file tree.
    /// If the sidebar is on the changes list, this just comes back to the tree.
    pub fn close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.request(Next::Quit, window, cx);
    }

    fn tree_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // While a row is being named the keyboard belongs to its text field.
        if self.editing.is_some() {
            return;
        }
        if self.sidebar_kind == SidebarKind::Changes {
            self.changes_key(event, window, cx);
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

fn home_dir() -> PathBuf {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

pub fn config_dir() -> PathBuf {
    let home = home_dir();
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
mod tests;
