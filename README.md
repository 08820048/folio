# Folio

一个 Rust + GPUI 的本地代码阅读编辑器。浅色 / 深色跟随系统，单窗口、多项目，没有 WebView、终端或 AI 服务。

## 运行

macOS，安装 Xcode Command Line Tools 后：

```sh
cargo run --locked
```

项目的 `rust-toolchain.toml` 固定 Rust 1.95.0，rustup 会按需安装。首次构建需要下载依赖，应用运行不需要联网。代码字体 JetBrains Mono 已嵌入二进制，许可证见 `assets/OFL.txt`；界面图标来自 gpui-component 内置的 Lucide 图标集，另外两个不从 Component 库内的图标（`panel-left-dashed` / `panel-right-dashed`） vendored 在 `assets/icons/`，许可证见该目录下的 `LICENSE.txt`。

生成本地 `.app`：

```sh
./scripts/package.sh
open target/Folio.app
```

开发包可用 `./scripts/package.sh --debug`。打包脚本进行本机 ad-hoc 签名，尚未办理分发签名与公证。

## 使用

打开项目后，在树中选择文件，或按 `⌘P` 模糊查找文件名。输入 `:123` 跳转行。切换文件保留光标、内容、撤销记录；关闭项目释放该项目缓冲。点击侧栏“添加项目”可加入更多文件夹，切换项目保留各自的编辑内容和展开状态；当前多项目列表不跨重启恢复。

| 操作 | macOS | Windows / Linux |
| --- | --- | --- |
| 打开项目 | ⌘O | Ctrl+O |
| 快速打开 | ⌘P | Ctrl+P |
| 在项目中搜索 | ⇧⌘F | Ctrl+Shift+F |
| 在项目中替换 | ⇧⌘H | Ctrl+Shift+H |
| 保存当前文件 | ⌘S | Ctrl+S |
| 文件内查找 | ⌘F | Ctrl+F |
| 跳转到行 | ⌘G | Ctrl+G |
| 切换侧栏 | ⌘B | Ctrl+B |
| 关闭项目 | ⌘W | Ctrl+W |
| 退出 | ⌘Q | Ctrl+Q |

标题栏与内容区随系统外观同步切换，侧栏展开 / 收起按钮、项目名与文件相对路径依次显示在 macOS 红绿灯右侧；原来的居中标题与第二行路径栏已移除。侧栏支持拖动宽度；顶部显示项目文件夹名，点击可收起 / 展开整棵树并保留子目录状态，也支持 Enter、空格和左右方向键。文件树支持方向键和 Enter。编辑功能使用 gpui-component 的 Rope 编辑器和 Tree-sitter 白名单语法；默认字号 14、行高 1.6、4 空格缩进、不软换行。

图片直接在编辑区按比例预览，支持 PNG、JPEG、GIF、WebP、BMP、TIFF、ICO 和 SVG；GIF / WebP 显示首帧，图片不会进入文本编辑或保存流程。应用图标统一使用 Lucide，图标按钮不加背景框。标题栏左上角的侧栏开关使用 `panel-left-dashed` / `panel-right-dashed`；这两个图标 gpui-component 不自带，由 `src/assets.rs` 包装内置资源源后从 `assets/icons/` 读取 —— 其余 99 个图标继续走组件库，两个来源在同一个 `icons/<name>.svg` 命名空间下。

保存会检查磁盘是否仍与打开时的版本一致，再写入同目录临时文件并原子替换；失败保持脏状态，外部变更不会直接被覆盖。最近项目是本地 JSON，最多 8 项；删除记录不删除项目文件。macOS 数据目录为 `~/Library/Application Support/Folio/`。

## 全局搜索与替换

`⌘P` 面板顶部的「文件名 / 内容」两个标签在同一处切换两种查找，`⇧⌘F` 直接打开内容查找，`⇧⌘H` 额外展开替换行。因为两种查找已经同属一个面板，标题栏只保留一个「全局搜索」按钮作为入口，不再单设「快速打开」按钮；`⌘P` 和菜单项仍然可以从文件名那一侧打开。

查找是实时的：输入停下来 140ms 后开始在后台扫描整个项目，结果按文件分组列出，命中行带行号和上下文预览，匹配片段用强调色标出。长行会以匹配处为中心截取并标上省略号，前导缩进会裁掉，Tab 展开成 4 个空格，所以预览能对齐。`↑ ↓` 选择、`Enter` 打开并把光标落在命中处、`Esc` 关闭。

三个过滤器沿用编辑器惯例，同时作用于查找与替换：

- **Aa** 区分大小写，默认关（不区分）
- **ab** 全字匹配：两侧都不能是字母、数字或下划线。用匹配后的边界判断实现，因为 `regex` crate 没有前后查找，而 `\b` 在查询以非单词字符开头或结尾时会绑错一侧
- `.*` 正则表达式，替换时 `$1` / `${name}` / `$$` 按 `regex` crate 的规则展开；关闭时查询被转义，替换串原样插入

「全部替换」会先确认再执行，而且**区分文件是否已经打开**：

- **已打开的文件**在内存中修改，保留撤销记录，变成未保存状态，等你按 `⌘S`；不会绕过你直接写盘
- **未打开的文件**在后台读、替换、原子写回。写盘用 `buffer::save`，它会先确认磁盘内容仍等于刚读出的内容，所以「读取 → 写入」这个窗口里的外部改动不会被覆盖；在读取之前发生的改动则会被读进来，替换作用在最新内容上（这是有意的：全局替换不应因为你另一处无关的编辑而整个失败）

搜索会用**内存中的内容**而不是磁盘内容来匹配已打开的文件，否则未保存的编辑会让所有行列号错位。大于 2 MiB 的文件、二进制文件和非 UTF-8 文件会被静默跳过，匹配总数上限 1000 处 / 200 个文件（命中上限时状态栏会注明「结果已截断」）。

## 语法高亮

高亮来自 Tree-sitter。gpui-component 内置 37 种语法，`src/syntax.rs` 通过它公开的 `LanguageRegistry` 再注册 23 种，不需要改动或 fork 组件库：

| 来源 | 语言 |
| --- | --- |
| 组件库内置 | Rust、C、C++、C#、Go、Zig、Swift、Java、Kotlin、Scala、Python、Ruby、PHP、Lua、Bash、Elixir、TypeScript、TSX、JavaScript、HTML、CSS、Astro、Svelte、EJS、ERB、JSON、TOML、YAML、CMake、Make、Markdown、SQL、GraphQL、Protocol Buffers、Diff、JsDoc |
| 本仓库新增 | XML、DTD、INI、Dockerfile、Nix、PowerShell、Fish、Vim script、Git 提交信息、正则、Vue、Dart、R、汇编、Solidity、Haskell、OCaml、Erlang、Elm、Gleam、SCSS、Less |

扩展名到语言的映射在 `src/buffer.rs::language`，整名匹配优先于扩展名，覆盖 `Dockerfile` / `Dockerfile.dev`、`Containerfile`、`Makefile`、`CMakeLists.txt`、`Gemfile` / `Rakefile` / `Vagrantfile`、`.zshrc` 系列、`.editorconfig`、`.vimrc`、`.Rprofile`、`Cargo.lock` / `Pipfile`、`COMMIT_EDITMSG`、`.env` / `.env.local` 等无扩展名或易误解的文件。大写 `.C` / `.H` 仍按约定视为 C++。未知扩展名返回 `text`，不高亮也不报错。

新增语言的判定条件写在 `src/syntax.rs` 的模块注释里：候选 crate 必须与 gpui-component 锁定的 tree-sitter 0.26 共用同一 `tree-sitter-language` 0.1 ABI、把高亮查询导出为常量、并且 build 脚本真的启用了门控该常量的 cfg。`tree-sitter-dockerfile`、`tree-sitter-vue`、`tree-sitter-wgsl`、`tree-sitter-cue` 卡在第一条，`tree-sitter-glsl`、`tree-sitter-nickel`、`tree-sitter-groovy`、`tree-sitter-perl` 和 `tree-sitter-vue-next` 卡在后面两条，都被排除。

语法表静态编译进二进制，只在打开对应文件时解析，对启动、内存和滚动没有影响；磁盘代价是 Release 二进制从约 21 MB 增至约 73 MB。最贵的四套是 OCaml 13.8 MB、Objective-C 11.4 MB、Haskell 8.1 MB、Git 提交信息 4.2 MB，其余每套都在 3.4 MB 以下；不需要某套时删掉 `src/syntax.rs` 的条目和 `Cargo.toml` 里的依赖即可，`syntax` feature 也能整体关闭（此时所有扩展名回退为纯文本）。

## 检查

```sh
cargo fmt --check
cargo test --locked --no-default-features
cargo test --locked --features desktop-tests
cargo clippy --locked --all-targets --features desktop-tests -- -D warnings
cargo build --locked
```

测试覆盖中文路径、最近项目 JSON、忽略目录、路径越界、UTF-8 和二进制校验、原子保存、外部修改冲突、权限保留、Git 未跟踪和重命名状态、扩展名到语法名的映射，以及项目搜索（大小写 / 全字 / 正则、长行窗口化、CRLF、多字节列号、捕获组替换、跳过二进制与超大文件）。`desktop-tests` 另用 GPUI 测试执行器覆盖连续展开/收起、最近项目按序写入、跨项目过期回调、选择器互斥，跨项目脏缓冲保留和退出保存冲突、图片解码及图片与脏文本切换、保存期间新编辑的保留，以及全局搜索读到未保存内容、命中后光标落位、替换对「已打开缓冲改内存 / 未打开文件写盘」的分流；不代替原生输入法或渲染验收。

每个新增语法都会真正编译一次高亮查询并断言不回退为纯文本 —— `SyntaxHighlighter::new` 在查询编译失败时会静默降级，只有断言才能发现配置写错的语法。

## 固定依赖

- 官方 GPUI `0.2.2` + `gpui_platform 0.1.0`，**源码提交** `cc053a4a6fa2fd0e8793201ed9099466af1be0b1`。
- gpui-component `0.5.2`，提交 `f3ba893bd6a996ab0699266ba774b5bbb7f0ca1c`，以及同提交的 assets。
- GPUI 系列来自同一 Git source，统一由 `Cargo.lock` 锁定，避免组件库与主程序出现两份不兼容的 GPUI。始终用 `--locked` 构建，不直接运行 `cargo update`。
- `src/syntax.rs` 的额外语法来自 crates.io，锁在 `Cargo.lock` 里，全部由 `syntax` feature 门控（`desktop` 会启用它）。它们只被二进制使用，`cargo test --locked` 这类仅库构建不会编译它们。

只编译 GPUI 框架及所需组件，不依赖 Zed 的 editor、language、workspace 应用模块。Cargo 的 Git source 下载仍会包含上游仓库 checkout。

## 当前边界

这是可运行的开发版本，尚未完成首发性能和跨平台验收。单文件超过 5 MiB 关闭高亮，超过 32 MiB 拒绝打开；文本只支持 UTF-8，跳过符号链接。快速打开最多索引 100,000 个文件、展示 100 个匹配项。全局搜索跳过超过 2 MiB 的文件，上限 1000 处 / 200 个文件，没有结果流式返回和取消按钮。位图最多 32M 像素、单边 16,384 像素，SVG 栅格化由 GPUI 限制尺寸。Git 状态在打开项目、切换项目和保存后刷新。

完整进度和待验收项见 [开发进度](docs/开发进度.md)，原始需求见 [PRD](docs/Folio需求文档.md)。
