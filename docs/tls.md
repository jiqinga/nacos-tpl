# TLS 与 OpenSSL 依赖说明 🔒

本项目已全面切换至 rustls 作为 TLS 后端，禁用 native-tls/hyper-tls，避免对系统 OpenSSL 的链接依赖，从而在 Linux/Windows/macOS 上获得更一致、更稳定的构建与运行体验。✅

## 具体实现
- HTTP 客户端：`reqwest = { version = "0.12", default-features = false, features = ["json","gzip","brotli","rustls-tls-native-roots","blocking"] }`
- 证书根：优先使用“系统根证书（native roots）”。在极简容器或无系统根的环境，可通过环境变量提供 CA 证书或临时关闭校验。

## 运行时配置（环境变量）
- `NACOS_TPL_TLS_INSECURE`: `true|false` 是否跳过 TLS 校验（默认关闭）。⚠️ 仅在内网临时排障使用。
- `NACOS_TPL_TLS_CA_CERT`: 自定义 CA 证书路径（PEM/DER 自动尝试）。

## 自检与验证
- 依赖树应无 OpenSSL/Native-TLS：
  - `cargo tree -i openssl`（无输出即通过）
  - `cargo tree -i native-tls`（无输出即通过）
- 如需更详细的关键字扫描：`pwsh scripts/tls_audit.ps1` 🔍

## 常见问题
- 容器内无系统根证书：优先挂载系统根，或切换到 `NACOS_TPL_TLS_CA_CERT` 指定自有 CA；不建议长期开启 `NACOS_TPL_TLS_INSECURE`。
- 旧二进制仍提示缺少 OpenSSL：请清理工作目录并重新构建/下载新版本（`cargo clean && cargo build --release`）。

