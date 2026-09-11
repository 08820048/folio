mod app;
mod assets;
mod preview;
mod syntax;
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
                KeyBinding::new(&format!("{modifier}-shift-d"), ToggleDiff, Some("Folio")),
                KeyBinding::new(&format!("{modifier}-,"), OpenSettings, Some("Folio")),
                KeyBinding::new(&format!("{modifier}-q"), Quit, Some("Folio")),
                // The settings window has its own context: `⌘W` closes that
                // window rather than the project behind it.
                KeyBinding::new(
                    &format!("{modifier}-w"),
                    CloseSettings,
                    Some("FolioSettings"),
                ),
                KeyBinding::new(&format!("{modifier}-q"), Quit, Some("FolioSettings")),
            ]);
            cx.set_menus([
                Menu::new("Folio").items([
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
                    MenuItem::action("Find", input::Search),
                ]),
                Menu::new("View").items([
                    MenuItem::action("Toggle Sidebar", ToggleSidebar),
                    MenuItem::action("Go to Line…", GoToLine),
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
