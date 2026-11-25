# Folio 界面约定

- 主要界面（活动栏、标题栏、Run、终端标签、启动页、快速打开）使用 Black Glass 玻璃图标：`assets/icons/glass/` 里的 96px PNG，经 `GlassIcon`（`src/assets.rs`）用 `gpui::img` 原色绘制，不着色；未激活时灰度加降低不透明度。图标包里没有对应图标的小操作（关闭、分屏、折叠、新建文件等）继续用 Lucide 线性图标。文件树里的文件类型标识（`src/file_icons.rs`）取自 Devicon（MIT），放在 `assets/icons/file-types/`，按类型单色着色。
- 浅色界面按 Black Glass 设计稿（Doop 画布「Folio · Black Glass 重设计」）：底色 `#E9E9E7`、纸面 `#F8F8F6`、墨色 `#1D1D1F`，全界面只有一个强调色钢蓝 `#3A6690`（光标、选区、焦点、链接）。深色模式暂沿用旧配色。
- 图标直接显示本身，不添加背景框、边框或阴影；悬停和按下也不显示背景块。保留可点击区域、键盘焦点和无障碍名称。
- 标题栏左上角、macOS 红绿灯右侧的按钮控制最左侧图标菜单条的展开／收起。文件树由菜单条第一项（以及 `⌘B`）控制。
- 组件库允许 fork。功能需要而 gpui-component 公开 API 够不到时（例如多光标 ——
  它固定的输入内核里根本没有多光标模型），可以 vendored 对应 crate 并用
  `[patch]` 指过去，不要因此砍掉功能。钉住的版本就是这个 fork 的基线，
  升级等于对着一个具名 commit 做 rebase。当前改了哪些文件见
  `vendor/README.md`。
