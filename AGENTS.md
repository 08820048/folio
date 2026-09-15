# Folio 界面约定

- 所有应用内图标统一使用 Lucide。唯一例外是文件树里的文件类型标识（`src/file_icons.rs`）：语言 logo 取自 Devicon（MIT），放在 `assets/icons/file-types/`，按类型单色着色。
- 图标直接显示本身，不添加背景框、边框或阴影；悬停和按下也不显示背景块。保留可点击区域、键盘焦点和无障碍名称。
- 标题栏左上角、macOS 红绿灯右侧的按钮控制最左侧图标菜单条的展开／收起。文件树由菜单条第一项（以及 `⌘B`）控制。
- 组件库允许 fork。功能需要而 gpui-component 公开 API 够不到时（例如多光标 ——
  它固定的输入内核里根本没有多光标模型），可以 vendored 对应 crate 并用
  `[patch]` 指过去，不要因此砍掉功能。钉住的版本就是这个 fork 的基线，
  升级等于对着一个具名 commit 做 rebase。当前改了哪些文件见
  `vendor/README.md`。
