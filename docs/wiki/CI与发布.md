# CI 与发布 🧰🚀

本项目内置 GitHub Actions 工作流用于自动构建与发布多平台二进制。你可以在 `.github/workflows/build.yml` 中查看与自定义。

## 触发方式
- Push 到 `main`/`master`：自动构建三平台（Linux/macOS/Windows）并上传构建工件（Artifacts）。
- 打标签 `v*`（如 `v0.1.0`）：在上述构建完成后，自动创建 Release 并上传各平台归档。🏷️

## 产物与命名
- Linux (glibc)：`nacos-tpl-<版本>-linux-x86_64.tar.gz`
- Linux 静态（musl）：
  - `nacos-tpl-<版本>-linux-musl-x86_64.tar.gz`
  - `nacos-tpl-<版本>-linux-musl-arm64.tar.gz`
- macOS：`nacos-tpl-<版本>-darwin-<arch>.tar.gz`
- Windows：`nacos-tpl-<版本>-windows-x86_64.zip`

版本来源：
- 打标签构建：使用标签名（如 `v0.1.0`）。
- 非标签构建：`v<Cargo.toml版本>-dev-<短SHA>`。

## 技术实现摘要
- 构建：`cargo build --release`。
- 缓存：`Swatinem/rust-cache` 加速依赖与产物缓存。
- musl x86_64：安装 `musl-tools` 并构建 `x86_64-unknown-linux-musl`。
- musl arm64：`cross build --target aarch64-unknown-linux-musl`（自动使用容器环境交叉编译）。
- 打包：Linux/macOS 使用 `tar.gz`；Windows 使用 `zip`。
- 发布：`softprops/action-gh-release` 读取下载的 Artifact 并创建 Release 附件。

## 自定义建议 💡
- 增加 SHA256 校验：可在打包后执行 `shasum -a 256` 生成校验文件并一并上传。
- 增加 Apple Silicon 原生产物：在 macOS 运行器上构建 `aarch64-apple-darwin` 并归档上传。
- 增加测试步骤：在构建前后运行 `cargo fmt --all -- --check`、`cargo clippy -- -D warnings` 与 `cargo test`。
- 为 Release 自动生成变更日志：集成 `release-please` 或 `git-cliff` 等工具。📜
