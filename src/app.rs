use crate::assets::FolioIcon;
use crate::preview::{self, Content};
use folio::{
    buffer, git,
    recent::{self, RecentProject},
    search,
    tree::{self, Entry, EntryKind},
    workspace::Workspace,
};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable, Theme, TitleBar,
    button::{Button, ButtonVariants},
    input::{Editor, EditorState, Input, InputEvent, InputState, Position, TabSize},
};
use std::{
    collections::{HashMap, HashSet},
    io,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

fn sync_appearance(appearance: WindowAppearance, window: &mut Window, cx: &mut App) {
    Theme::change(appearance, Some(window), cx);
    let theme = Theme::global_mut(cx);
    let dark = theme.is_dark();
    theme.mono_font_family = "JetBrains Mono".into();
    theme.mono_font_size = px(14.);
    theme.font_size = px(13.);
    theme.background = rgb(if dark { 0x181A1C } else { 0xFAFAF8 }).into();
    theme.foreground = rgb(if dark { 0xDCDDD8 } else { 0x282D2B }).into();
    theme.sidebar = rgb(if dark { 0x1D1F21 } else { 0xF0F1ED }).into();
    theme.popover = theme.sidebar;
    theme.muted_foreground = rgb(if dark { 0x929792 } else { 0x626A64 }).into();
    theme.border = rgb(if dark { 0x2B2E30 } else { 0xDADDD6 }).into();
    theme.accent_foreground = rgb(if dark { 0xBECBAD } else { 0x4C6341 }).into();
    theme.list_active = rgb(if dark { 0x2B3031 } else { 0xDDE5D8 }).into();
    theme.list_hover = rgb(if dark { 0x25292B } else { 0xE6EAE2 }).into();
    theme.title_bar = theme.background;
    theme.title_bar_border = theme.border;
    let background = theme.background;
    let foreground = theme.foreground;
    let muted = theme.muted_foreground;
    let highlight = std::sync::Arc::make_mut(&mut theme.highlight_theme);
    highlight.style.editor_background = Some(background);
    highlight.style.editor_foreground = Some(foreground);
    highlight.style.editor_gutter_background = Some(background);
    highlight.style.editor_active_line = Some(rgb(if dark { 0x212628 } else { 0xEFF2EB }).into());
    highlight.style.editor_line_number = Some(muted);
    window.refresh();
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
        Quit
    ]
);

#[derive(Clone)]
enum Next {
    Picker,
    Open(PathBuf),
    Close,
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
#[derive(Clone, Copy, PartialEq, Eq)]
enum Panel {
    /// `⌘P`: fuzzy file-name search.
    Files,
    /// `⇧⌘F`: project-wide content search, optionally with replace.
    Search,
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
    active: Option<PathBuf>,
    git_status: HashMap<String, char>,
    files: Vec<PathBuf>,
    indexing: bool,
}

pub struct Folio {
    project: Project,
    parked: Vec<Project>,
    project_order: Vec<PathBuf>,
    recent: Vec<RecentProject>,
    recent_file: PathBuf,
    recent_task: Option<Task<()>>,
    tree_focus: FocusHandle,
    quick_scroll: UniformListScrollHandle,
    sidebar: bool,
    sidebar_width: f32,
    resizing: bool,
    generation: u64,
    open_request: u64,
    query: Entity<InputState>,
    panel: Option<Panel>,
    matches: Vec<PathBuf>,
    match_selected: usize,
    search_query: Entity<InputState>,
    replace_query: Entity<InputState>,
    search: SearchState,
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
    _subscriptions: Vec<Subscription>,
}

impl Folio {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        sync_appearance(window.appearance(), window, cx);
        let appearance_subscription = cx.observe_window_appearance(window, |_, window, cx| {
            sync_appearance(window.appearance(), window, cx);
            cx.notify();
        });
        let query =
            cx.new(|cx| InputState::new(window, cx).placeholder("搜索文件名，或输入 :行号"));
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
                this.sidebar_width = this
                    .sidebar_width
                    .min((f32::from(window.viewport_size().width) * 0.4).max(160.));
                cx.notify();
            });
        let search_query = cx.new(|cx| InputState::new(window, cx).placeholder("搜索内容"));
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
        let replace_query = cx.new(|cx| InputState::new(window, cx).placeholder("替换为"));
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
        let mut this = Self {
            project: Project::default(),
            parked: vec![],
            project_order: vec![],
            recent: vec![],
            recent_file,
            recent_task: None,
            tree_focus: cx.focus_handle(),
            quick_scroll: UniformListScrollHandle::new(),
            sidebar: true,
            sidebar_width: 240.,
            resizing: false,
            generation: 0,
            open_request: 0,
            query,
            panel: None,
            matches: vec![],
            match_selected: 0,
            search_query,
            replace_query,
            search: SearchState::default(),
            search_request: Arc::new(AtomicU64::new(0)),
            goto: None,
            message: None,
            loading: false,
            project_loading: false,
            saving: false,
            prompting: false,
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

    fn request(&mut self, next: Next, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || self.prompting || self.project_loading {
            return;
        }
        // Invalidate a pending file read before changing project or opening a modal.
        self.open_request += 1;
        self.loading = false;
        let dirty = match next {
            Next::Picker | Next::Open(_) => 0,
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
        self.prompting = true;
        cx.notify();
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("有 {dirty} 个文件尚未保存"),
            Some(if matches!(next, Next::Quit) {
                "退出之前，是否保存所有项目的修改？"
            } else {
                "关闭当前项目之前，是否保存修改？"
            }),
            &["全部保存", "不保存", "取消"],
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

    fn perform(&mut self, next: Next, window: &mut Window, cx: &mut Context<Self>) {
        match next {
            Next::Quit => {
                let bounds = window.window_bounds().get_bounds();
                let values = [
                    f32::from(bounds.origin.x),
                    f32::from(bounds.origin.y),
                    f32::from(bounds.size.width),
                    f32::from(bounds.size.height),
                ];
                let save_window = || -> std::io::Result<()> {
                    std::fs::create_dir_all(config_dir())?;
                    std::fs::write(
                        config_dir().join("window.json"),
                        serde_json::to_vec(&values)?,
                    )
                };
                if let Err(error) = save_window() {
                    eprintln!("窗口位置未保存：{error}");
                }
                cx.quit();
            }
            Next::Close => {
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
                cx.notify();
            }
            Next::Open(path) => self.open_project(path, window, cx),
            Next::Picker => {
                let picker = cx.prompt_for_paths(PathPromptOptions {
                    files: false,
                    directories: true,
                    multiple: false,
                    prompt: Some("打开项目".into()),
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
        let task = cx.background_executor().spawn(async move {
            let workspace = Workspace::open(&path)?;
            let children = tree::children(&workspace.root)?;
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
                        this.tree_focus.focus(window, cx);
                    }
                    Err(e) => this.error(format!("无法打开项目：{e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn refresh_project(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = &self.project.workspace else {
            return;
        };
        self.project.indexing = true;
        let project_id = self.project.id;
        let root = workspace.root.clone();
        self.refresh_recent(RecentAction::Open(root.clone()), cx);
        let task = cx
            .background_executor()
            .spawn(async move { tree::index(&root) });
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
        self.refresh_git(cx);
    }

    fn refresh_git(&mut self, cx: &mut Context<Self>) {
        let Some(workspace) = &self.project.workspace else {
            return;
        };
        let project_id = self.project.id;
        let root = workspace.root.clone();
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
                    Err(e) => this.error(format!("Git 状态读取失败：{e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn rebuild_rows(&mut self) {
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
                let task = cx
                    .background_executor()
                    .spawn(async move { tree::children(&dir) });
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

    fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving || self.project_loading || self.prompting {
            return;
        }
        self.panel = None;
        // A pending jump only belongs to the file it was queued for.
        if self
            .goto
            .as_ref()
            .is_some_and(|(target, _)| target != &path)
        {
            self.goto = None;
        }
        self.open_request += 1;
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
            cx.notify();
            return;
        }
        if let Some(doc) = self.project.documents.get(&path) {
            let editor = doc.editor.clone();
            self.project.active = Some(path.clone());
            self.loading = false;
            editor.focus_handle(cx).focus(window, cx);
            self.apply_goto(&path, window, cx);
            self.update_title(window);
            cx.notify();
            return;
        }
        let Some(workspace) = self.project.workspace.clone() else {
            return;
        };
        let generation = self.generation;
        let request = self.open_request;
        self.loading = true;
        cx.notify();
        let task = cx.background_executor().spawn(async move {
            let path = workspace.resolve(&path)?;
            let content = preview::read(&path)?;
            Ok::<_, std::io::Error>((path, content))
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.generation != generation || this.open_request != request {
                    return;
                }
                this.loading = false;
                match result {
                    Ok((path, Content::Image(image))) => {
                        this.project.image = Some((path.clone(), image));
                        this.project.active = Some(path);
                        this.message = None;
                        this.tree_focus.focus(window, cx);
                        this.update_title(window);
                    }
                    Ok((path, Content::Text(text))) => {
                        let large = text.len() > buffer::HIGHLIGHT_LIMIT;
                        let language = if large {
                            // "text" is gpui-component's grammardless language.
                            "text"
                        } else {
                            buffer::language(&path)
                        };
                        let saved: SharedString = text.into();
                        let editor = cx.new(|cx| {
                            EditorState::new(language, window, cx)
                                .default_value(saved.clone())
                                .line_number(true)
                                .folding(false)
                                .tab_size(TabSize {
                                    tab_size: 4,
                                    hard_tabs: false,
                                })
                        });
                        editor.update(cx, |state, cx| {
                            state.prepare(window, cx);
                            state
                                .base_state()
                                .update(cx, |base, cx| base.set_soft_wrap(false, window, cx));
                        });
                        let key = path.clone();
                        let project_id = this.project.id;
                        let subscription = cx.subscribe_in(
                            &editor,
                            window,
                            move |this, editor, event: &InputEvent, window, cx| {
                                if matches!(event, InputEvent::Change) {
                                    if let Some(doc) = this
                                        .project_mut(project_id)
                                        .and_then(|p| p.documents.get_mut(&key))
                                    {
                                        doc.dirty = editor.read(cx).value() != doc.saved;
                                    }
                                    this.update_title(window);
                                    cx.notify();
                                }
                            },
                        );
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
                        this.project.active = Some(path);
                        this.message = None;
                        this.update_title(window);
                    }
                    Err(e) => this.error(e.to_string(), cx),
                }
                cx.notify();
            });
        })
        .detach();
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
        let saves = std::iter::once(&self.project)
            .chain(self.parked.iter().filter(|_| all_projects))
            .flat_map(|project| {
                project
                    .documents
                    .iter()
                    .filter(|(path, doc)| {
                        doc.dirty && (next.is_some() || project.active.as_ref() == Some(path))
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
                        .ok_or_else(|| std::io::Error::other("项目已关闭"))
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
                            if let Some(doc) = this
                                .project_mut(project_id)
                                .and_then(|p| p.documents.get_mut(&path))
                            {
                                doc.saved = text;
                                doc.dirty = doc.editor.read(cx).value() != doc.saved;
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
                        this.toast("已保存", cx);
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
                    Err(e) => this.error(format!("最近项目更新失败：{e}"), cx),
                }
                cx.notify();
            });
        }));
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
                self.search_query
                    .update(cx, |query, cx| query.focus(window, cx));
                self.start_search(cx);
            }
        }
        cx.notify();
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
                    let files = this.project.files.clone();
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
            "全部替换",
            Some(&format!(
                "将在 {files} 个文件中替换 {hits} 处。未打开的文件会立即写盘，此操作不可撤销。"
            )),
            &["全部替换", "取消"],
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
                            .ok_or_else(|| io::Error::other("项目已关闭"))
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
                            format!("已替换 {replaced} 处 · {files} 个文件")
                        } else {
                            format!(
                                "已替换 {replaced} 处 · {files} 个文件（{open_files} 个已打开文件待保存）"
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
            self.error("请输入有效行号，例如 :123".into(), cx);
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
                                    .child(div().text_size(px(32.)).child("Folio"))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .text_color(cx.theme().muted_foreground)
                                            .child("读懂代码，改好几行。"),
                                    ),
                            ),
                    )
                    .child(
                        Button::new("open-project")
                            .text()
                            .icon(IconName::FolderOpen)
                            .label(if self.loading {
                                "正在打开…"
                            } else {
                                "打开项目…"
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
                                    .text_size(px(11.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child("最近项目")
                                    .child("⌘ O"),
                            )
                            .when(self.recent.is_empty(), |el| {
                                el.child(
                                    div()
                                        .py_4()
                                        .text_size(px(13.))
                                        .text_color(cx.theme().muted_foreground)
                                        .child("从一个本地文件夹开始。"),
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
                                            .aria_label(format!("打开项目 {}", name(&path)))
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
                                                            " · 路径失效"
                                                        }
                                                    ))
                                                    .child(
                                                        div()
                                                            .text_size(px(11.))
                                                            .text_color(cx.theme().muted_foreground)
                                                            .child(relative_time(item.last_opened)),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(11.))
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
                                        "从最近项目移除",
                                        move |this, _, cx| this.remove_recent(remove.clone(), cx),
                                        cx,
                                    ))
                            })),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(cx.theme().muted_foreground)
                            .child("也可以将文件夹拖到这里"),
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
            .px_3()
            .flex()
            .items_center()
            .gap_2()
            .text_size(px(12.))
            .text_color(if current {
                cx.theme().foreground
            } else {
                cx.theme().muted_foreground
            })
            .cursor_default()
            .hover(|el| el.text_color(cx.theme().accent_foreground))
            .on_click(
                cx.listener(move |this, _, window, cx| this.select_project(&root, window, cx)),
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
            .aria_label("项目")
            .track_focus(&self.tree_focus)
            .tab_index(0)
            .key_context("FolioTree")
            .w(px(self.sidebar_width))
            .h_full()
            .flex_shrink_0()
            .bg(cx.theme().sidebar)
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
                        .label("添加项目")
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
                        let row = this.project.rows[i].clone();
                        let path = row.entry.path.clone();
                        let selected = this.project.active.as_ref() == Some(&path);
                        let status = this
                            .project
                            .workspace
                            .as_ref()
                            .and_then(|w| path.strip_prefix(&w.root).ok())
                            .and_then(|p| this.project.git_status.get(p.to_string_lossy().as_ref()))
                            .copied();
                        let icon = if row.entry.kind == EntryKind::Directory {
                            if this.project.expanded.contains(&path) {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            }
                        } else {
                            IconName::File
                        };
                        div()
                            .id(("row", i))
                            .role(Role::TreeItem)
                            .aria_label(row.entry.name.clone())
                            .w_full()
                            .h(px(27.))
                            .pl(px(14. + row.depth as f32 * 14.))
                            .pr_3()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_size(px(12.))
                            .cursor_default()
                            .when(selected || i == this.project.selected_row, |el| {
                                el.bg(cx.theme().list_active)
                            })
                            .hover(|el| el.bg(cx.theme().list_hover))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.project.selected_row = i;
                                this.tree_focus.focus(window, cx);
                                match row.entry.kind {
                                    EntryKind::Directory => this.toggle_directory(path.clone(), cx),
                                    EntryKind::File => this.open_file(path.clone(), window, cx),
                                }
                            }))
                            .child(
                                Icon::new(icon)
                                    .xsmall()
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
                    })
                    .collect()
            }),
        )
        .track_scroll(&self.project.tree_scroll)
        .flex_1()
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
            .child(self.panel_tab("panel-tab-files", "文件名", Panel::Files, cx))
            .child(self.panel_tab("panel-tab-search", "内容", Panel::Search, cx))
            .child(div().flex_1())
            .when(panel == Panel::Search, |el| {
                el.child(self.option_toggle(
                    "search-case",
                    "Aa",
                    "区分大小写",
                    self.search.options.case_sensitive,
                    |options| options.case_sensitive = !options.case_sensitive,
                    cx,
                ))
                .child(self.option_toggle(
                    "search-word",
                    "ab",
                    "全字匹配",
                    self.search.options.whole_word,
                    |options| options.whole_word = !options.whole_word,
                    cx,
                ))
                .child(self.option_toggle(
                    "search-regex",
                    ".*",
                    "正则表达式，替换时可用 $1 引用捕获组",
                    self.search.options.regex,
                    |options| options.regex = !options.regex,
                    cx,
                ))
                .child(Self::icon_button(
                    "search-replace-toggle",
                    IconName::Replace,
                    "替换",
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
                "关闭",
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
            .text_size(px(11.))
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
                format!("{tooltip}（已开启）")
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
            .text_size(px(11.))
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
                    .text_size(px(11.))
                    .text_color(cx.theme().muted_foreground)
                    .child(if self.project.indexing {
                        "正在索引文件…"
                    } else if jumping {
                        "Enter 跳转到行 · Esc 关闭"
                    } else {
                        "↑ ↓ 选择 · Enter 打开 · Esc 关闭"
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
                                        .text_size(px(12.))
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
                                .label("全部替换")
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
            "正在索引文件…".into()
        } else if self.search.running {
            "正在搜索…".into()
        } else if self.search.query.trim().is_empty() {
            "输入内容以在项目中搜索".into()
        } else if self.search.results.is_empty() {
            "无结果".into()
        } else {
            format!(
                "{} 个匹配 · {} 个文件{}",
                self.total_hits(),
                self.search.results.len(),
                if self.search.truncated {
                    " · 结果已截断"
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
            .text_size(px(11.))
            .text_color(if error {
                cx.theme().warning
            } else {
                cx.theme().muted_foreground
            })
            .child(div().flex_1().min_w_0().child(text))
            .when(!error && !self.search.rows.is_empty(), |el| {
                el.child("↑ ↓ 选择 · Enter 打开 · Esc 关闭")
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
            .text_size(px(12.))
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
                            .text_size(px(11.))
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
                            .text_size(px(11.))
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
            .bg(cx.theme().background)
            .border_color(cx.theme().border)
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
                            "sidebar-toggle",
                            if self.sidebar {
                                FolioIcon::PanelLeftDashed
                            } else {
                                FolioIcon::PanelRightDashed
                            },
                            "切换侧栏",
                            |this, _, cx| {
                                this.sidebar = !this.sidebar;
                                cx.notify();
                            },
                            cx,
                        ))
                    })
                    .child(div().text_size(px(12.)).child(project))
                    .when(relative.is_some(), |el| {
                        el.child(div().text_color(cx.theme().muted_foreground).child("/"))
                    })
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(12.))
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
                                .text_size(px(11.))
                                .text_color(cx.theme().muted_foreground)
                                .child("读取中…"),
                        )
                    })
                    .when(self.project.workspace.is_some(), |el| {
                        // One button for the whole panel: it opens on the content
                        // tab, and the panel's own tabs reach the file finder
                        // (also `⌘P`).
                        //
                        // These are `text` buttons: they draw no background in any
                        // state and carry no padding of their own. At the 13px rem
                        // this theme uses, the row's own gap was only ~10px, so the
                        // labels ran together; `px_2` per button plus this gap gives
                        // ~23px between them.
                        el.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(
                                    Button::new("project-search")
                                        .text()
                                        .icon(IconName::Search)
                                        .label("搜索")
                                        .xsmall()
                                        .px_2()
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.show_search(false, window, cx)
                                        })),
                                )
                                .child(
                                    Button::new("close-project")
                                        .text()
                                        .icon(IconName::Close)
                                        .label("关闭项目")
                                        .xsmall()
                                        .px_2()
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.request(Next::Close, window, cx)
                                        })),
                                ),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_workspace(&self, cx: &mut Context<Self>) -> AnyElement {
        let doc = self
            .project
            .active
            .as_ref()
            .and_then(|p| self.project.documents.get(p));
        let image = self
            .project
            .image
            .as_ref()
            .filter(|(path, _)| self.project.active.as_ref() == Some(path));
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .when(self.sidebar, |el| {
                        el.child(self.render_tree(cx)).child(
                            div()
                                .id("sidebar-resize")
                                .w(px(3.))
                                .h_full()
                                .bg(cx.theme().border)
                                .cursor_col_resize()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _, _, _| this.resizing = true),
                                ),
                        )
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .flex_col()
                            .when(!self.sidebar, |el| el.px_6())
                            .when_some(doc, |el, doc| {
                                el.when(doc.large, |el| {
                                    el.child(
                                        div()
                                            .px_4()
                                            .py_1()
                                            .text_size(px(11.))
                                            .text_color(cx.theme().muted_foreground)
                                            .child("大文件 · 已关闭语法高亮"),
                                    )
                                })
                                .child(
                                    Editor::new(&doc.editor)
                                        .h_full()
                                        .bordered(false)
                                        .readonly(self.saving || self.loading || self.prompting)
                                        .text_size(px(14.))
                                        .font_family("JetBrains Mono")
                                        .line_height(gpui::relative(1.6))
                                        .rounded_none(),
                                )
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
                                        .text_size(px(11.))
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!(
                                            "{} · {} × {} · 静态预览",
                                            name(path),
                                            u32::from(image.size(0).width),
                                            u32::from(image.size(0).height)
                                        )),
                                )
                            })
                            .when(doc.is_none() && image.is_none(), |el| {
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
                                                .text_size(px(24.))
                                                .text_color(cx.theme().accent_foreground)
                                                .child("留一点空间，读一段代码。"),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.))
                                                .text_color(cx.theme().muted_foreground)
                                                .child("从左侧选择文件，或按 ⌘ P 快速打开"),
                                        ),
                                )
                            }),
                    ),
            )
            .into_any_element()
    }
}

impl Render for Folio {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("folio")
            .key_context("Folio")
            .when(self.project.workspace.is_none(), |el| {
                el.track_focus(&self.tree_focus)
            })
            .size_full()
            .flex()
            .flex_col()
            .relative()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .text_size(px(13.))
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
                this.sidebar = !this.sidebar;
                cx.notify();
            }))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                this.resizing &= event.dragging();
                if this.resizing {
                    this.sidebar_width = f32::from(event.position.x).clamp(
                        160.,
                        (f32::from(window.viewport_size().width) * 0.4).max(160.),
                    );
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.resizing = false),
            )
            .on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
                if let Some(path) = paths.0.first() {
                    this.request(Next::Open(path.clone()), window, cx);
                }
            }))
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let Some(panel) = this.panel else {
                    return;
                };
                match event.keystroke.key.as_str() {
                    "escape" => this.close_panel(window, cx),
                    "down" | "up" => {
                        let down = event.keystroke.key == "down";
                        if panel == Panel::Search {
                            this.move_search_selection(down);
                        } else {
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
                        self.render_workspace(cx)
                    } else {
                        self.render_launcher(cx)
                    }),
            )
            .when_some(self.panel, |el, panel| {
                el.child(self.render_panel(panel, cx))
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
                        .child(div().flex_1().text_size(px(12.)).child(message))
                        .child(Self::icon_button(
                            "dismiss-message",
                            IconName::Close,
                            "关闭提示",
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
        0..3600 => "刚刚".into(),
        3600..86400 => format!("{} 小时前", age / 3600),
        _ => format!("{} 天前", age / 86400),
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
        cx.simulate_prompt_answer("取消");
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
        cx.simulate_prompt_answer("取消");
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
            app
        });
        for appearance in [
            WindowAppearance::Dark,
            WindowAppearance::Light,
            WindowAppearance::Dark,
        ] {
            window
                .update(cx, |_, window, cx| {
                    sync_appearance(appearance, window, cx);
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
                app
            })
        });
        view.update_in(cx, |app, _, cx| {
            app.project.workspace = Some(Workspace::open(&root).unwrap());
            app.project
                .directories
                .insert(root.clone(), tree::children(&root).unwrap());
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
        cx.simulate_prompt_answer("取消");
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

        // Opening a hit closes the panel and leaves the cursor on the match.
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
            assert!(app.panel.is_none(), "opening a hit closes the panel");
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
        cx.simulate_prompt_answer("全部替换");
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
}
