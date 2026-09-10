mod app;
mod preview;
use app::*;
use gpui::*;
use gpui_component::{Root, TitleBar, input};

fn main() {
    gpui_platform::application()
        .with_assets(gpui_component_assets::Assets)
        .run(|cx| {
            gpui_component::init(cx);
            gpui_component::set_locale("zh-CN");
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
                KeyBinding::new(&format!("{modifier}-g"), GoToLine, Some("Folio")),
                KeyBinding::new(&format!("{modifier}-b"), ToggleSidebar, Some("Folio")),
                KeyBinding::new(&format!("{modifier}-q"), Quit, Some("Folio")),
            ]);
            cx.set_menus([
                Menu::new("Folio").items([MenuItem::action("退出 Folio", Quit)]),
                Menu::new("文件").items([
                    MenuItem::action("打开项目…", OpenProject),
                    MenuItem::action("快速打开…", QuickOpen),
                    MenuItem::separator(),
                    MenuItem::action("保存", Save),
                    MenuItem::action("关闭项目", CloseProject),
                ]),
                Menu::new("编辑").items([
                    MenuItem::action("撤销", input::Undo),
                    MenuItem::action("重做", input::Redo),
                    MenuItem::separator(),
                    MenuItem::action("剪切", input::Cut),
                    MenuItem::action("复制", input::Copy),
                    MenuItem::action("粘贴", input::Paste),
                    MenuItem::action("全选", input::SelectAll),
                    MenuItem::separator(),
                    MenuItem::action("查找", input::Search),
                ]),
                Menu::new("视图").items([
                    MenuItem::action("切换侧栏", ToggleSidebar),
                    MenuItem::action("跳转到行…", GoToLine),
                ]),
            ]);
            let bounds = std::fs::read(config_dir().join("window.json"))
                .ok()
                .and_then(|raw| serde_json::from_slice::<[f32; 4]>(&raw).ok())
                .filter(|v| v.iter().all(|x| x.is_finite()) && v[2] >= 640. && v[3] >= 480.)
                .map(|[x, y, w, h]| {
                    WindowBounds::Windowed(Bounds::new(point(px(x), px(y)), size(px(w), px(h))))
                })
                .unwrap_or_else(|| WindowBounds::centered(size(px(960.), px(680.)), cx));
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(bounds),
                    window_min_size: Some(size(px(640.), px(480.))),
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
            .expect("无法打开 Folio 窗口");
            cx.activate(true);
        });
}
