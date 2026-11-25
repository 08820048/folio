# Folio 界面约定

- 主要界面（标题栏、左侧面板分段、Run、终端标签、专注模式停靠栏、启动页、快速打开）使用玻璃图标，同一套图形两种材质：`assets/icons/glass/`（Black Glass，黑）和 `assets/icons/glass-blue/`（Glass，蓝），96px PNG，经 `GlassIcon` / `Tone`（`src/assets.rs`）用 `gpui::img` 原色绘制，不着色。用哪种由 `glass_tone` 决定：浅色下静止为黑、激活或主操作（Run）为蓝；深色下一律蓝。未激活的活动栏图标灰度加降低不透明度。图标包里没有对应图标的小操作（关闭、分屏、折叠、新建文件等）继续用 Lucide 线性图标。文件树里的文件类型标识（`src/file_icons.rs`）取自 Devicon（MIT），放在 `assets/icons/file-types/`，按类型单色着色。
- 配色按 Glass 设计稿（Doop 画布「Folio · Black Glass 重设计」）：浅色底 `#E9E9E7`、纸面 `#F8F8F6`、墨色 `#1D1D1F`；全界面只有一个强调色，取自蓝玻璃图标（浅色 `#2B63D0`，深色 `#6EA4F2`），用于光标、选区、焦点、链接。深色是同一张看片台关了灯，见 `sync_appearance`。
- 图标直接显示本身，不添加背景框、边框或阴影；悬停和按下也不显示背景块。保留可点击区域、键盘焦点和无障碍名称。
- 布局按 Doop 画布「方向探索 · 结构」的 B+C 稿（2026-11-25 通过）：标题栏一行放侧栏开关、标签、命令栏（项目 › 分支 ●改动数 ⌘P）、Focus 和 Run；没有最左侧图标菜单条。红绿灯右侧的按钮（以及 `⌘B`）显示／隐藏左侧面板，面板顶部用 Files / Changes / Search 分段切换。专注模式（`⌘K Z`，Esc 退出）隐藏面板和终端但不关闭它们，入口收进底部悬浮停靠栏。
- 组件库允许 fork。功能需要而 gpui-component 公开 API 够不到时（例如多光标 ——
  它固定的输入内核里根本没有多光标模型），可以 vendored 对应 crate 并用
  `[patch]` 指过去，不要因此砍掉功能。钉住的版本就是这个 fork 的基线，
  升级等于对着一个具名 commit 做 rebase。当前改了哪些文件见
  `vendor/README.md`。
