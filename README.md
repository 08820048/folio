# Folio

一个 Rust + GPUI 的本地代码阅读编辑器。浅色 / 深色跟随系统，单窗口、多项目，没有 WebView、终端或 AI 服务。

## 运行

macOS，安装 Xcode Command Line Tools 后：

```sh
cargo run --locked
```

项目的 `rust-toolchain.toml` 固定 Rust 1.95.0，rustup 会按需安装。首次构建需要下载依赖，应用运行不需要联网。代码字体 JetBrains Mono 已嵌入二进制，许可证见 `assets/OFL.txt`。

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
| 保存当前文件 | ⌘S | Ctrl+S |
| 文件内查找 | ⌘F | Ctrl+F |
| 跳转到行 | ⌘G | Ctrl+G |
| 切换侧栏 | ⌘B | Ctrl+B |
| 关闭项目 | ⌘W | Ctrl+W |
| 退出 | ⌘Q | Ctrl+Q |

标题栏与内容区随系统外观同步切换，侧栏展开 / 收起按钮、项目名与文件相对路径依次显示在 macOS 红绿灯右侧；原来的居中标题与第二行路径栏已移除。侧栏支持拖动宽度；顶部显示项目文件夹名，点击可收起 / 展开整棵树并保留子目录状态，也支持 Enter、空格和左右方向键。文件树支持方向键和 Enter。编辑功能使用 gpui-component 的 Rope 编辑器和 Tree-sitter 白名单语法；默认字号 14、行高 1.6、4 空格缩进、不软换行。

图片直接在编辑区按比例预览，支持 PNG、JPEG、GIF、WebP、BMP、TIFF、ICO 和 SVG；GIF / WebP 显示首帧，图片不会进入文本编辑或保存流程。应用图标统一使用 Lucide，图标按钮不加背景框。

保存会检查磁盘是否仍与打开时的版本一致，再写入同目录临时文件并原子替换；失败保持脏状态，外部变更不会直接被覆盖。最近项目是本地 JSON，最多 8 项；删除记录不删除项目文件。macOS 数据目录为 `~/Library/Application Support/Folio/`。

## 检查

```sh
cargo fmt --check
cargo test --locked --no-default-features
cargo test --locked --features desktop-tests
cargo clippy --locked --all-targets --features desktop-tests -- -D warnings
cargo build --locked
```

测试覆盖中文路径、最近项目 JSON、忽略目录、路径越界、UTF-8 和二进制校验、原子保存、外部修改冲突、权限保留、Git 未跟踪和重命名状态。`desktop-tests` 另用 GPUI 测试执行器覆盖连续展开/收起、最近项目按序写入、跨项目过期回调、选择器互斥，跨项目脏缓冲保留和退出保存冲突、图片解码及图片与脏文本切换，以及保存期间新编辑的保留；不代替原生输入法或渲染验收。

## 固定依赖

- 官方 GPUI `0.2.2` + `gpui_platform 0.1.0`，**源码提交** `cc053a4a6fa2fd0e8793201ed9099466af1be0b1`。
- gpui-component `0.5.2`，提交 `f3ba893bd6a996ab0699266ba774b5bbb7f0ca1c`，以及同提交的 assets。
- GPUI 系列来自同一 Git source，统一由 `Cargo.lock` 锁定，避免组件库与主程序出现两份不兼容的 GPUI。始终用 `--locked` 构建，不直接运行 `cargo update`。

只编译 GPUI 框架及所需组件，不依赖 Zed 的 editor、language、workspace 应用模块。Cargo 的 Git source 下载仍会包含上游仓库 checkout。

## 当前边界

这是可运行的开发版本，尚未完成首发性能和跨平台验收。单文件超过 5 MiB 关闭高亮，超过 32 MiB 拒绝打开；文本只支持 UTF-8，跳过符号链接。快速打开最多索引 100,000 个文件、展示 100 个匹配项。位图最多 32M 像素、单边 16,384 像素，SVG 栅格化由 GPUI 限制尺寸。Git 状态在打开项目、切换项目和保存后刷新。

完整进度和待验收项见 [开发进度](docs/开发进度.md)，原始需求见 [PRD](docs/Folio需求文档.md)。
