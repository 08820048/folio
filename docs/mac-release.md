# Folio macOS 发版

本机构建、Developer ID 签名、Apple 公证，Sparkle 自动更新，安装包和 appcast 上传到 Cloudflare R2。**不创建 GitHub Release。**

## 常量

```text
VERSION / BUILD     packaging/release.env（VERSION 必须等于 Cargo.toml）
R2_BUCKET           folio-releases
PUBLIC_BASE         https://pub-307aebc4dfa4468d80e05e193c8f9c5b.r2.dev
CODESIGN_IDENTITY   B5B549B0DE0CDF5F843A2756EFE5CCE412D2CF43
                    Developer ID Application: Yi Xu (9XAXTT7H29)
NOTARY_PROFILE      hoolo-notary（同一 Apple 团队的钥匙串 profile）
Sparkle 私钥        登录钥匙串，account = Folio Sparkle
```

第一次在这台机器上发版，先跑一次 `./scripts/sparkle-keys.sh`。公钥写入 `packaging/sparkle_public.txt`，私钥只进钥匙串。

## 发一版

1. 改 `Cargo.toml` 的 `version` 和 `packaging/release.env` 里的 `VERSION` / `BUILD`。`BUILD` 必须比上一版大（Sparkle 比的是它）。
2. 本机：

```sh
./scripts/release.sh --notes "这一版用户能看见的变化。"
```

只要打包、公证、不上传：`--skip-upload`。

脚本会：Release 编译 → 嵌入 Sparkle.framework → Developer ID + hardened runtime → 做 DMG → `notarytool` + staple → 从已 staple 的 `.app` 打 Sparkle zip → `sign_update` → 写 `appcast.xml` → 先上传 zip / DMG / 说明，最后上传 appcast。

Windows **不是**这条路径。同一笔 `VERSION` 的 commit 推上 `v<VERSION>` tag 之后，GitHub Actions 在 `windows-latest` 编译并上传 R2。Mac 脚本不要打 tag、不要改。详见 `docs/windows.md`。

## 用户拿到的地址

```text
https://pub-307aebc4dfa4468d80e05e193c8f9c5b.r2.dev/appcast.xml
https://pub-307aebc4dfa4468d80e05e193c8f9c5b.r2.dev/Folio-<version>-<build>.dmg
https://pub-307aebc4dfa4468d80e05e193c8f9c5b.r2.dev/Folio-latest.dmg
https://pub-307aebc4dfa4468d80e05e193c8f9c5b.r2.dev/Folio-<version>.zip
```

网站下载用 **DMG**，不要把 Sparkle 的 zip 当安装包发。DMG 窗口用 `assets/app-icon/back.jpg`（1808×1104，按 2× 做成 TIFF）：左边 Folio.app，右边 Applications，中间箭头。

## 日常开发

```sh
./scripts/package.sh          # Release，无 CODESIGN_IDENTITY 时 ad-hoc
./scripts/package.sh --debug
cargo run --locked            # 没有 Sparkle，菜单里检查更新会 toast
```

## 不要

- 不要 `gh release create`（Windows 的 tag 只触发 Actions，也不发 GitHub Release）
- 不要把未公证的 DMG 传到带 `immutable` 缓存头的旧文件名上；换 `Folio-<version>-<build>.dmg`
- 不要把 Sparkle 私钥提交进仓库
