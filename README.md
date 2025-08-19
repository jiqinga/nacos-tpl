# nacos-tpl（最小可用版）

一个用于 Nacos 配置模板化与导入的 CLI。当前已支持：
- 配置发现/合并（用户/项目/显式）与部分环境变量覆盖
- `render` 渲染（Tera + 兼容 `${VAR}` 语法）
- `--stdout` + `--print` 单文件直出，或 `-o` 目录输出
- `init`（目录/zip 输入）按规则替换并生成 `variables.example.yaml`
- `apply`（OpenAPI 发布）与 `package`（zip 打包）基础流程
- `diff-remote`（远端对比 same/changed/added + 可选统一 diff）

## 安装与下载（GitHub Release） 📦

你可以直接从 Release 页面下载各平台预编译二进制（已配好 CI 自动构建与发布）：

- Linux (glibc)：`nacos-tpl-vX.Y.Z-linux-x86_64.tar.gz` 🐧
- Linux 静态（musl, x86_64）：`nacos-tpl-vX.Y.Z-linux-musl-x86_64.tar.gz` 🧱
- Linux 静态（musl, arm64）：`nacos-tpl-vX.Y.Z-linux-musl-arm64.tar.gz` 🧱
- macOS（根据 Runner 自动选择 arm64/x86_64）：`nacos-tpl-vX.Y.Z-darwin-<arch>.tar.gz` 🍎
- Windows：`nacos-tpl-vX.Y.Z-windows-x86_64.zip` 🪟

快速安装示例：

```bash
# Linux (glibc)
VERSION=vX.Y.Z
curl -L -o nacos-tpl.tar.gz \
  https://github.com/<your-org>/<your-repo>/releases/download/$VERSION/nacos-tpl-$VERSION-linux-x86_64.tar.gz
mkdir -p ~/bin && tar -C ~/bin -xzf nacos-tpl.tar.gz
chmod +x ~/bin/nacos-tpl && export PATH=~/bin:$PATH
nacos-tpl --help

# Linux (musl 静态, x86_64) —— 更便于分发
curl -L -o nacos-tpl.tar.gz \
  https://github.com/<your-org>/<your-repo>/releases/download/$VERSION/nacos-tpl-$VERSION-linux-musl-x86_64.tar.gz
mkdir -p ~/bin && tar -C ~/bin -xzf nacos-tpl.tar.gz
chmod +x ~/bin/nacos-tpl && export PATH=~/bin:$PATH

# macOS（将 <arch> 替换为 arm64 或 x86_64）
curl -L -o nacos-tpl.tar.gz \
  https://github.com/<your-org>/<your-repo>/releases/download/$VERSION/nacos-tpl-$VERSION-darwin-<arch>.tar.gz
sudo tar -C /usr/local/bin -xzf nacos-tpl.tar.gz
nacos-tpl --help

# Windows（PowerShell）
$VERSION = "vX.Y.Z"
Invoke-WebRequest -Uri "https://github.com/<your-org>/<your-repo>/releases/download/$VERSION/nacos-tpl-$VERSION-windows-x86_64.zip" -OutFile "nacos-tpl.zip"
Expand-Archive -Path "nacos-tpl.zip" -DestinationPath "$env:USERPROFILE\bin" -Force
$env:Path += ";$env:USERPROFILE\bin"; nacos-tpl.exe --help
```

> 提示：以上示例中的 `<your-org>/<your-repo>` 请替换为你在 GitHub 的实际仓库路径。🚀

### 从源码构建（可选） 🛠️

```bash
cargo build --release
# 生成的二进制：target/release/nacos-tpl
```

### CI/Release 说明 🧰

- GitHub Actions 工作流位于 `.github/workflows/build.yml`：
  - Push/PR：自动在 Linux/macOS/Windows 构建并上传构建工件（Artifacts）。
  - 打标签（`v*`）：自动创建 Release，并上传各平台归档：
    - Linux glibc：`linux-x86_64`
    - Linux 静态：`linux-musl-x86_64`、`linux-musl-arm64`
    - macOS：`darwin-<arch>`
    - Windows：`windows-x86_64`
  - 归档命名：`nacos-tpl-<版本>-<平台>.{tar.gz|zip}`。
  - 版本来源：打标签则使用标签名，否则使用 `v<Cargo.toml版本>-dev-<短SHA>`。

## Build & Run
```
cargo run -- render -t examples/template -v examples/vars.yaml --print app.yaml --stdout

# render single file to path
cargo run -- render -t examples/template -v examples/vars.yaml --print app.yaml --output-file build/app.yaml

# render single by dataId (requires manifest.yaml in template dir)
cargo run -- render -t template -v variables.yaml --print-id app.yaml --stdout
cargo run -- render -t template -v variables.yaml --print-id app.yaml --group DEFAULT_GROUP --output-file build/app.yaml
# If manifest.yaml is absent, print-id falls back to YAML front-matter in files (--- dataId/group ---)

# render subset by globs (directory output)
cargo run -- render -t template -v variables.yaml -o build/dev --include "**/*.yml,**/*.properties" --exclude "**/test/**"

# write manifest.yaml after directory render (for later selection by dataId)
cargo run -- render -t template -v variables.yaml -o build/dev --write-manifest

# diff-remote: compare local vs remote (text-level)
cargo run -- diff-remote -s http://127.0.0.1:8848 -n public -d build/dev --json --report dist/diff-report.json
# JSON 报告包含 overall 计数、按组汇总（groups）以及按组的 changed/added 明细（groups_detail）

# diff-remote with include/exclude filters
cargo run -- diff-remote -s http://127.0.0.1:8848 -n public -d build/dev --include "**/*.yml" --exclude "**/test/**"

# diff-remote show unified diffs for changed items
cargo run -- diff-remote -s http://127.0.0.1:8848 -n public -d build/dev --show-changed --context 5

# diff-remote only print changed/added (omit summary)
cargo run -- diff-remote -s http://127.0.0.1:8848 -n public -d build/dev --only-changed

# diff-remote only print added items
cargo run -- diff-remote -s http://127.0.0.1:8848 -n public -d build/dev --only-added

# diff-remote grouped listing
cargo run -- diff-remote -s http://127.0.0.1:8848 -n public -d build/dev --grouped

# init (dir or zip input)
cargo run -- init -i examples/export_dir -o template/ -r mapping.rules.yaml
cargo run -- init -i export.zip -o template/ -r mapping.rules.yaml

# rules (JSON/YAML/properties path replace + regex for properties)
# matcher example (YAML):
# - when:
#     ext: yaml
#     path_glob: "**/application*.yml"
#   replace:
#     spring.datasource.url: "${DB_URL}"
# matcher example (properties + regex):
# - when:
#     ext: properties
#   replace:
#     kafka.bootstrap.servers: "${KAFKA_BOOTSTRAP}"
#   regex_replace:
#     - pattern: "(?i)password=([^;&\n]+)"
#       to: "password=${DB_PASS}"
# JSON 也支持 regex_replace（整体文本层面）：
# - when:
#     ext: json
#   regex_replace:
#     - pattern: "(?i)\"password\"\s*:\s*\"[^\"]+\""
#       to: "\"password\": \"${DB_PASS}\""
# 掩码关键字（可在 rules 顶层配置 mask_keywords）命中时，示例变量值将写为 ******
# 说明：示例变量会自动生成；带敏感关键词（PASSWORD/SECRET/TOKEN/AK/SK/KEY）的变量将被掩码为 ******。

# package -> zip for console import
cargo run -- package -t template/ -v examples/vars.yaml -o dist/dev.zip

# apply via OpenAPI (retries/timeout/concurrency + JSON report)
cargo run -- apply -d build/dev -s http://127.0.0.1:8848 -n public \
  --skip-unchanged --retries 3 --timeout-ms 10000 --concurrency 5 \
  --json --report dist/apply-report.json --report-mode overwrite --fail-on-error
  # append or timestamp modes:
  # --report dist/apply-report.json --report-mode append
  # --report dist/apply-report.json --report-mode timestamp
  # add --dry-run to only compute changes without posting
  # nacos-tpl apply ... --dry-run

# non-JSON output shows per-group summary and failed items list

# validate (strict + required)
cargo run -- validate -t template/ -v variables.dev.yaml --strict -r mapping.rules.yaml

# validate with JSON/report
cargo run -- validate -t template/ -v variables.dev.yaml --json --report dist/validate-report.json
```

## Diff 风格与颜色高亮 🌈
- 统一 diff（默认，删除=红，新增=绿，未变=灰）：
  - `cargo run -- diff-remote -s http://127.0.0.1:8848 -d build/dev --only-changed`
- 左右对比（side-by-side，左=远端，右=本地，均带颜色）：
  - `cargo run -- diff-remote -s http://127.0.0.1:8848 -d build/dev --only-changed --diff-style side-by-side`
- 控制上下文：`--context 5`（默认 3）
- 如需仅打印清单（不展示差异）：`--no-show-changed`

提示：`-s` 支持不带协议的地址（如 `127.0.0.1:8848`），会自动补全为 `http://...`；工具会自动登录换取 `accessToken` 并附加到后续请求。🔐

## Validate 规则文件（-r/--rules）🧭
`--rules` 用于声明“必须在变量中显式提供”的键，帮助在渲染前卡口必填项（与 `--strict` 相辅相成）。

示例 `rules.yaml`：
```yaml
required:
  - DB_HOST
  - DB_PORT
  - REDIS_URL
  - SPRING_PROFILES_ACTIVE
```

变量键名如何生成（扁平化规则）：
- 将 YAML 的层级路径用下划线拼接并转大写：`db.host` -> `DB_HOST`，`spring.profiles.active` -> `SPRING_PROFILES_ACTIVE`。
- 数组会转成 JSON 字符串保存，键名仍按上面规则生成。
- 同名环境变量（大写）会覆盖变量文件中的同名键。✅

常用命令：
- 基础校验 + 规则：`cargo run -- validate -t ./templates -v ./vars/dev.yaml -r ./rules.yaml`
- 严格校验 + JSON 报告：`cargo run -- validate -t ./templates -v ./vars/prod.yaml -r ./rules.yaml --strict --json --report ./build/validate_report.json`

## 常见问题（FAQ）❓
- 为什么 `diff-remote` 返回 403？
  - 多数是未携带 `accessToken`。本工具会先登录获取 token，并自动拼接到请求。请确认 `-u/-p` 正确，或在配置里提供 `token`。
- 为什么只显示“相同/变更/新增”，但没看到变更内容？
  - 默认已启用变更差异展示（`--show-changed`）。如需关闭可加 `--no-show-changed`。也可用 `--diff-style side-by-side` 改为左右对比。
- 颜色显示异常？
  - 某些终端需开启 ANSI 支持；或在日志重定向时颜色会被终端去除。可考虑提供 `--no-color`（未来版本）。
- `--help` 时报“参数错误”？
  - 已修复，现用 `Cli::parse()` 由 clap 内建处理，`--help/--version` 正常 0 退出。✅

设计与规划文档见 `plan/` 目录。📂

## 最小可跑示例（本地快速开始） 🚀

以下示例均使用仓库内示例模板与变量文件，输出与日志为中文：

1) 渲染到目录（并生成 manifest.yaml）
```
cargo run -- render -t examples/template -v examples/vars.yaml -o build/dev --write-manifest
```

2) 单文件渲染到标准输出（从相对路径选择）
```
cargo run -- render -t examples/template -v examples/vars.yaml --print DEFAULT_GROUP/app.yaml --stdout
```

3) 严格校验（若仍有占位符则失败）
```
cargo run -- validate -t examples/template -v examples/vars.yaml --strict
```

4) 打包为可导入 zip（包含 manifest.yaml）
```
cargo run -- package -t examples/template -v examples/vars.yaml -o dist/dev.zip
```

5) 远端对比（需本地/远端 Nacos 可用）
```
cargo run -- diff-remote -s http://127.0.0.1:8848 -n public -d build/dev --show-changed --context 3
```

6) 发布到远端（需 Nacos 与命名空间有效）
```
cargo run -- apply -s http://127.0.0.1:8848 -n public -d build/dev \
  --skip-unchanged --retries 3 --concurrency 5 --timeout-ms 10000 \
  --normalize-lf --max-bytes 1048576 \
  --json --report dist/apply-report.json
```

提示：
- 多服务器容错：`--server` 支持逗号分隔多个地址，自动轮询降级。🧩
- 退出码：参数=1、渲染/校验=2、IO/解析=3、网络/认证=4。🧭
- 全量中文：日志、错误、非 JSON 输出均为中文表述。🈶️
- 质量选项：`--normalize-lf` 统一换行、`--max-bytes` 限制单项大小（字节）。📏
 - 报告增强：`--include-md5` 在 JSON 报告的条目中包含 `localMd5` 与 `sizeBytes`。🧮

### 本地启动 Nacos（可选）
- 使用 Docker Compose：`docker compose -f scripts/compose/local.yaml up -d`
- 控制台：`http://127.0.0.1:8848/nacos`（默认账号/密码：`nacos/nacos`）
- 环境变量：`NACOS_TPL_SERVER=http://127.0.0.1:8848`，`NACOS_TPL_NAMESPACE=public`

## 环境变量（覆盖优先级说明） 🔧
- 覆盖链：命令行 > 环境变量（本节） > 项目配置 > 用户配置 > 内置默认
- 常用变量：
  - `NACOS_TPL_PROFILE`：激活的 profile 名称（覆盖配置 `active_profile`）。
  - `NACOS_TPL_SERVER`：单个服务地址，如 `http://127.0.0.1:8848`。
  - `NACOS_TPL_SERVERS`：逗号分隔的多个服务地址，用于容错，如 `https://a,https://b`。
  - `NACOS_TPL_NAMESPACE`：命名空间（tenant）ID。
  - `NACOS_TPL_USERNAME` / `NACOS_TPL_PASSWORD`：登录用户名/密码（用于换取 `accessToken`）。
  - `NACOS_TPL_ACCESS_KEY` / `NACOS_TPL_SECRET_KEY` / `NACOS_TPL_TOKEN`：可选凭证集（视服务能力）。
  - `NACOS_TPL_TIMEOUT_MS`：请求超时毫秒数。
  - `NACOS_TPL_TLS_INSECURE`：`true|false` 是否跳过 TLS 校验。
  - `NACOS_TPL_TLS_CA_CERT`：自定义 CA 证书路径。
  - `NACOS_TPL_RENDER_VARIABLES_FILE`：`render` 的默认变量文件路径。
  - `NACOS_TPL_RENDER_STDOUT`：单文件渲染默认输出到 stdout（`true|false`）。
 - `NACOS_TPL_INIT_EXAMPLE_FILE`：`init` 生成示例变量文件名。

## 本地上传/对比筛选规则（.metadata.yml）🧩
- 放置位置：渲染输出目录根（例如 `build/dev/.metadata.yml`）。
- 作用范围：`apply` 与 `diff-remote` 共同复用统一的“上传筛选”模块，对同一套文件应用相同的选择逻辑。
- 基本结构（示例）：

```
metadata:
  - dataId: application-nocprodrun.yml
    group: euoap
    type: yaml        # 可选，供 apply 作为类型提示；未指定时按扩展名推断
    tenant: public    # 可选，仅用于辅助推断命名空间
    desc: "某环境专用配置"  # 可选，仅用于说明
```

- 行为说明：
  - 若检测到 `.metadata.yml` 且包含 `metadata` 列表，则仅处理其中列出的 `(group, dataId)` 条目（白名单）。✅
  - 始终跳过控制文件：`.metadata.yml/.yaml`、`manifest.yml/.yaml`。🚫
  - `diff-remote` 除遵循上述白名单外，仍可额外叠加 `--include/--exclude` 进行二次过滤（相对目录的 glob）。🎯
  - `apply` 不提供命令行 include/exclude，专注遵循 `.metadata.yml` 和默认忽略规则（KISS）。

## 公共筛选模块（开发者说明）🧱
- 模块位置：`src/common/selector.rs`
- 主要接口：
  - `collect_upload_candidates(dir, includes, excludes, use_allow_set) -> Vec<PathBuf>`：收集需要参与上传/对比的文件；`use_allow_set=true` 时启用 `.metadata.yml` 白名单。
  - `derive_group_and_dataid(root, file) -> (String, String)`：从路径推导 `group/dataId`（`<root>/<GROUP>/<DATAID>`）。
- 复用点：
  - `apply`：统一使用 `collect_upload_candidates(dir, [], [], true)` 收集候选后再并发发布。
  - `diff-remote`：使用 `collect_upload_candidates(dir, includes, excludes, true)` 收集候选后对比远端文本。
