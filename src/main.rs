mod app;
mod assets;
mod preview;
mod syntax;
mod terminal_view;
use app::*;
use gpui::*;
use gpui_component::{Root, TitleBar, input};

fn main() {
    gpui_platform::application()
        .with_assets(assets::Assets)
        .run(|cx| {
            gpui_component::init(cx);
            syntax::register_extra_languages();
            gpui_component::set_locale("en");
            let _ = cx
                .text_system()
                .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
                    "../assets/JetBrainsMono-Regular.ttf"
                ))]);
            let modifier = if cfg!(target_os = "macos") {
                "cmd"
            } else {
                "ctrl"
            };
            cx.bind_keys([
                KeyBinding::new(&format!("{modifier}-o"), OpenProject, Some("Folio")),
                KeyBinding::new(&format!("{modifier}-w"), CloseProject, Some("Folio")),
                KeyBinding::new(&format!("{modifier}-s"), Save, Some("Folio")),
                KeyBinding::new(&format!("{modifier}-p"), QuickOpen, Some("Folio")),
                KeyBinding::new(&format!("{modifier}-shift-f"), ProjectSearch, Some("Folio")),
                KeyBinding::new(
                    &format!("{modifier}-shift-h"),
                    ProjectReplace,
                    Some("Folio"),
                ),
                KeyBinding::new(&format!("{modifier}-g"), GoToLine, Some("Folio")),
                KeyBinding::new(&format!("{modifier}-b"), ToggleSidebar, Some("Folio")),
                // The dock, like the sidebar a toggle: hiding it keeps the
                // child running.
                KeyBinding::new(&format!("{modifier}-j"), ToggleTerminal, Some("Folio")),
                // These three answer only while the terminal holds the
                // keyboard; elsewhere ⌘C and ⌘V stay the editor's, and ⌘W
                // keeps closing the project.
                KeyBinding::new(
                    &format!("{modifier}-c"),
                    TerminalCopy,
                    Some("FolioTerminal"),
                ),
                KeyBinding::new(
                    &format!("{modifier}-v"),
                    TerminalPaste,
                    Some("FolioTerminal"),
                ),
                KeyBinding::new(
                    &format!("{modifier}-w"),
                    CloseTerminal,
                    Some("FolioTerminal"),
                ),
                KeyBinding::new(&format!("{modifier}-shift-d"), ToggleDiff, Some("Folio")),
                KeyBinding::new(&format!("{modifier}-alt-b"), ToggleBlame, Some("Folio")),
                // The editor's own ⌘⇧F, moved off it so `⇧⌘F` can be the
                // project search everywhere. The action is the component's;
                // only the key it answers to changes.
                KeyBinding::new(&format!("{modifier}-alt-f"), input::Replace, Some("Folio")),
                KeyBinding::new(&format!("{modifier}-/"), ToggleComment, Some("Folio")),
                // Folding, beside the indent keys the editor binds itself:
                // `⌘[` and `⌘]` are indentation, so folding takes the option
                // key as well, which is where VS Code puts it.
                KeyBinding::new(&format!("{modifier}-alt-["), Fold, Some("FolioEditor")),
                KeyBinding::new(&format!("{modifier}-alt-]"), Unfold, Some("FolioEditor")),
                // More than one cursor: also the code editor's alone. In the
                // search box and the settings fields `⌘D` and `⌥⌘↑` mean
                // whatever the platform means by them.
                KeyBinding::new(
                    &format!("{modifier}-d"),
                    SelectNextOccurrence,
                    Some("FolioEditor"),
                ),
                KeyBinding::new(
                    &format!("{modifier}-shift-l"),
                    SelectAllOccurrences,
                    Some("FolioEditor"),
                ),
                KeyBinding::new(
                    &format!("{modifier}-alt-up"),
                    AddCursorAbove,
                    Some("FolioEditor"),
                ),
                KeyBinding::new(
                    &format!("{modifier}-alt-down"),
                    AddCursorBelow,
                    Some("FolioEditor"),
                ),
                // Column selection adds a line to a rectangle rather than a
                // cursor: the same key with shift, the way the mouse gets a
                // rectangle by holding option.
                KeyBinding::new(
                    &format!("{modifier}-alt-shift-up"),
                    SelectColumnUp,
                    Some("FolioEditor"),
                ),
                KeyBinding::new(
                    &format!("{modifier}-alt-shift-down"),
                    SelectColumnDown,
                    Some("FolioEditor"),
                ),
                // Bound to the code editor's key context rather than the app's,
                // so brackets still type normally in the search box and the
                // settings fields. These replace the editor's own handling of
                // the key, which is the only way to insert a pair as one edit.
                KeyBinding::new("(", PairParen, Some("FolioEditor")),
                KeyBinding::new("[", PairBracket, Some("FolioEditor")),
                KeyBinding::new("{", PairBrace, Some("FolioEditor")),
                KeyBinding::new("\"", PairQuote, Some("FolioEditor")),
                KeyBinding::new("'", PairApostrophe, Some("FolioEditor")),
                KeyBinding::new("`", PairBacktick, Some("FolioEditor")),
                KeyBinding::new(")", SkipParen, Some("FolioEditor")),
                KeyBinding::new("]", SkipBracket, Some("FolioEditor")),
                KeyBinding::new("}", SkipBrace, Some("FolioEditor")),
                KeyBinding::new(&format!("{modifier}-,"), OpenSettings, Some("Folio")),
                KeyBinding::new(&format!("{modifier}-q"), Quit, Some("Folio")),
                // The settings window has its own context: `⌘W` closes that
                // window rather than the project behind it.
                KeyBinding::new(&format!("{modifier}-w"), CloseWindow, Some("FolioSettings")),
                KeyBinding::new(&format!("{modifier}-q"), Quit, Some("FolioSettings")),
                KeyBinding::new(&format!("{modifier}-w"), CloseWindow, Some("FolioAbout")),
                KeyBinding::new(&format!("{modifier}-q"), Quit, Some("FolioAbout")),
            ]);
            cx.set_menus([
                Menu::new("Folio").items([
                    MenuItem::action("About Folio", OpenAbout),
                    MenuItem::separator(),
                    MenuItem::action("Check for Updates…", CheckForUpdates),
                    MenuItem::separator(),
                    MenuItem::action("Settings…", OpenSettings),
                    MenuItem::separator(),
                    MenuItem::action("Quit Folio", Quit),
                ]),
                Menu::new("File").items([
                    MenuItem::action("Open Project…", OpenProject),
                    MenuItem::action("Quick Open…", QuickOpen),
                    MenuItem::action("Find in Project…", ProjectSearch),
                    MenuItem::action("Replace in Project…", ProjectReplace),
                    MenuItem::separator(),
                    MenuItem::action("Save", Save),
                    MenuItem::action("Close Project", CloseProject),
                ]),
                Menu::new("Edit").items([
                    MenuItem::action("Undo", input::Undo),
                    MenuItem::action("Redo", input::Redo),
                    MenuItem::separator(),
                    MenuItem::action("Cut", input::Cut),
                    MenuItem::action("Copy", input::Copy),
                    MenuItem::action("Paste", input::Paste),
                    MenuItem::action("Select All", input::SelectAll),
                    MenuItem::separator(),
                    MenuItem::action("Toggle Comment", ToggleComment),
                    MenuItem::action("Fold", Fold),
                    MenuItem::action("Unfold", Unfold),
                    MenuItem::separator(),
                    MenuItem::action("Select Next Occurrence", SelectNextOccurrence),
                    MenuItem::action("Select All Occurrences", SelectAllOccurrences),
                    MenuItem::separator(),
                    MenuItem::action("Add Cursor Above", AddCursorAbove),
                    MenuItem::action("Add Cursor Below", AddCursorBelow),
                    MenuItem::action("Select Column Up", SelectColumnUp),
                    MenuItem::action("Select Column Down", SelectColumnDown),
                    MenuItem::separator(),
                    MenuItem::action("Find", input::Search),
                    MenuItem::action("Replace in File", input::Replace),
                ]),
                Menu::new("View").items([
                    MenuItem::action("Toggle Activity Bar", ToggleActivityBar),
                    MenuItem::action("Toggle Sidebar", ToggleSidebar),
                    MenuItem::action("Terminal", ToggleTerminal),
                    MenuItem::action("Go to Line…", GoToLine),
                    MenuItem::separator(),
                    MenuItem::action("Blame", ToggleBlame),
                ]),
            ]);
            let state = WindowState::load(&config_dir().join("window.json"));
            let bounds = state
                .main
                .and_then(|values| restore_bounds(values, MAIN_WINDOW_MIN))
                .unwrap_or_else(|| WindowBounds::centered(size(px(960.), px(680.)), cx));
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(bounds),
                    window_min_size: Some(MAIN_WINDOW_MIN),
                    ..TitleBar::window_options()
                },
                |window, cx| {
                    let view = cx.new(|cx| Folio::new(window, cx));
                    view.update(cx, |app, cx| app.start_session(window, cx));
                    let weak = view.downgrade();
                    window.on_window_should_close(cx, move |window, cx| {
                        let _ = weak.update(cx, |this, cx| this.close_window(window, cx));
                        false
                    });
                    cx.new(|cx| Root::new(view, window, cx))
                },
            )
            .expect("Failed to open the Folio window");
            cx.activate(true);
        });
}
