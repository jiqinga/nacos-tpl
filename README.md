# nacos-tpl

一个用于 Nacos 配置模板化与导入的 CLI。当前已支持：
- 配置发现/合并（用户/项目/显式）与部分环境变量覆盖
- `render` 渲染（Tera + 兼容 `${VAR}` 语法）
- `--stdout` + `--print` 单文件直出，或 `-o` 目录输出
- `init`（目录/zip 输入）按规则替换并生成 `variables.example.yaml`
- `apply`（OpenAPI 发布）与 `package`（zip 打包）基础流程
- `diff-remote`（远端对比 same/changed/added + 可选统一 diff）

## 安装与下载（GitHub Release） 📦

你可以直接从 [Release](https://github.com/jiqinga/nacos-tpl/releases) 页面下载各平台预编译二进制：


快速安装示例：
nacos-tpl-v0.1.0-x86_64-pc-windows-msvc.zip
```bash
# Linux (glibc)
VERSION=vX.Y.Z
curl -L -o nacos-tpl.tar.gz \
  https://github.com/jiqinga/nacos-tpl/releases/download/$VERSION/nacos-tpl-$VERSION-x86_64-unknown-linux-gnu.tar.gz
mkdir -p ~/bin 
tar -C /tmp/ -xzf nacos-tpl.tar.gz
mv /tmp/nacos-tpl-$VERSION-x86_64-unknown-linux-gnu/nacos-tpl ~/bin
chmod +x ~/bin/nacos-tpl && export PATH=~/bin:$PATH
nacos-tpl --help

# Linux (musl 静态, x86_64) —— 更便于分发
VERSION=vX.Y.Z
curl -L -o nacos-tpl.tar.gz \
  https://github.com/jiqinga/nacos-tpl/releases/download/$VERSION/nacos-tpl-$VERSION-x86_64-unknown-linux-musl.tar.gz
mkdir -p ~/bin 
tar -C /tmp/ -xzf nacos-tpl.tar.gz
mv /tmp/nacos-tpl-$VERSION-x86_64-unknown-linux-gnu/nacos-tpl ~/bin
chmod +x ~/bin/nacos-tpl && export PATH=~/bin:$PATH
nacos-tpl --help

# macOS（ aarch64 或 x86_64）
VERSION=vX.Y.Z
curl -L -o nacos-tpl.tar.gz \
  https://github.com/jiqinga/nacos-tpl/releases/download/$VERSION/nacos-tpl-$VERSION-x86_64-apple-darwin.tar.gz
sudo tar -C /tmp/ -xzf nacos-tpl.tar.gz
sudo mv  nacos-tpl-$VERSION-x86_64-unknown-linux-gnu/nacos-tpl /usr/local/bin
sudo chmod +x /usr/local/bin/nacos-tpl
nacos-tpl --help
```


### 从源码构建（可选） 🛠️

```bash
cargo build --release
# 生成的二进制：target/release/nacos-tpl
```



## Build & Run
```
cargo run -- render -t examples/template -v examples/vars.yaml --print app.yaml --stdout

# 将单文件渲染到指定路径 📄➡️📁
cargo run -- render -t examples/template -v examples/vars.yaml --print app.yaml --output-file build/app.yaml

# 按 dataId 渲染单个文件（模板目录需包含 manifest.yaml）🆔
cargo run -- render -t template -v variables.yaml --print-id app.yaml --stdout
cargo run -- render -t template -v variables.yaml --print-id app.yaml --group DEFAULT_GROUP --output-file build/app.yaml
# 若缺少 manifest.yaml，则 print-id 回退到文件 YAML 头部（--- dataId/group ---）📜

# 通过通配符选择子集（目录输出）🗂️
cargo run -- render -t template -v variables.yaml -o build/dev --include "**/*.yml,**/*.properties" --exclude "**/test/**"

# 目录渲染后写出 manifest.yaml（供后续按 dataId 选择）📝
cargo run -- render -t template -v variables.yaml -o build/dev --write-manifest

# diff-remote：对比本地与远端（文本级）🔍
cargo run -- diff-remote -s http://127.0.0.1:8848 -n public -d build/dev --json --report dist/diff-report.json
# JSON 报告包含 overall 计数、按组汇总（groups）以及按组的 changed/added 明细（groups_detail）

# diff-remote 支持 include/exclude 过滤器 🔎
cargo run -- diff-remote -s http://127.0.0.1:8848 -n public -d build/dev --include "**/*.yml" --exclude "**/test/**"

# 展示统一 diff 的变更项 📘
cargo run -- diff-remote -s http://127.0.0.1:8848 -n public -d build/dev --show-changed --context 5

# 仅输出变更/新增（省略汇总）📝
cargo run -- diff-remote -s http://127.0.0.1:8848 -n public -d build/dev --only-changed

# 仅输出新增项 ➕
cargo run -- diff-remote -s http://127.0.0.1:8848 -n public -d build/dev --only-added

# 按组列出清单 📂
cargo run -- diff-remote -s http://127.0.0.1:8848 -n public -d build/dev --grouped

# 初始化（目录或 zip 输入）🧭
cargo run -- init -i examples/export_dir -o template/ -r mapping.rules.yaml
cargo run -- init -i export.zip -o template/ -r mapping.rules.yaml

# 规则（JSON/YAML/properties 路径替换 + properties 正则）📐
# 匹配示例（YAML）：
# - when:
#     ext: yaml
#     path_glob: "**/application*.yml"
#   replace:
#     spring.datasource.url: "${DB_URL}"
# 匹配示例（properties + 正则）：
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

# 打包为 zip（控制台导入）📦
cargo run -- package -t template/ -v examples/vars.yaml -o dist/dev.zip

# 通过 OpenAPI 执行发布（重试/超时/并发 + JSON 报告）🚀
cargo run -- apply -d build/dev -s http://127.0.0.1:8848 -n public \
  --skip-unchanged --retries 3 --timeout-ms 10000 --concurrency 5 \
  --json --report dist/apply-report.json --report-mode overwrite --fail-on-error
  # 追加或时间戳模式：
  # --report dist/apply-report.json --report-mode append
  # --report dist/apply-report.json --report-mode timestamp
  # 使用 --dry-run 仅计算变更、不实际发布 🧪
  # 示例：nacos-tpl apply ... --dry-run

# 非 JSON 输出：显示按组汇总与失败清单 📊

# 校验（严格模式 + 必填）✅
cargo run -- validate -t template/ -v variables.dev.yaml --strict -r mapping.rules.yaml

# 校验并生成 JSON 报告 🧾
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
 
