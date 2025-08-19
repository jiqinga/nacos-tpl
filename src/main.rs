#![allow(clippy::items_after_test_module)]
mod apply;
mod cfg;
mod common;
mod diff;
mod init;
mod nacos;
mod package;
mod render;
mod validate;

use crate::nacos::NacosHttp;
use anyhow::{bail, Context, Result};
use clap::{ArgAction, Parser, Subcommand};
use colored::Colorize;
use similar::TextDiff;
use tracing_subscriber::EnvFilter;

#[derive(Parser, Debug)]
#[command(
    name = "nacos-tpl",
    version,
    about = "Nacos 配置模板化与导入 CLI",
    long_about = r#"nacos-tpl —— Nacos 配置模板化与导入 CLI 🧰

快速示例：
- 渲染单文件到标准输出：
  nacos-tpl render -t examples/template -v examples/vars.yaml --print DEFAULT_GROUP/app.yaml --stdout
- 渲染到目录（并生成 manifest.yaml）：
  nacos-tpl render -t examples/template -v examples/vars.yaml -o build/dev --write-manifest
- 严格校验（残留占位符即失败）：
  nacos-tpl validate -t examples/template -v examples/vars.yaml --strict -r rules.yaml
- 远端对比（默认展示差异，统一 diff）：
  nacos-tpl diff-remote -s http://127.0.0.1:8848 -n public -d build/dev --only-changed
- 远端对比（左右对比风格）：
  nacos-tpl diff-remote -s http://127.0.0.1:8848 -d build/dev --only-changed --diff-style side-by-side
- 发布（仅示例，谨慎操作生产）：
  nacos-tpl apply -s http://127.0.0.1:8848 -n public -d build/dev --skip-unchanged --json --report dist/apply-report.json

提示：-s 支持不带协议（会自动补全 http://）；会先登录拿 accessToken 再对比/发布。"#,
    after_help = r#"示例（概览）👇
  渲染到目录：
    nacos-tpl render -t examples/template -v examples/vars.yaml -o build/dev --write-manifest
  校验（严格 + 规则）：
    nacos-tpl validate -t examples/template -v examples/vars.yaml --strict -r rules.yaml
  远端对比（左右对比）：
    nacos-tpl diff-remote -s http://127.0.0.1:8848 -d build/dev --only-changed --diff-style side-by-side
  发布（JSON 报告）：
    nacos-tpl apply -s http://127.0.0.1:8848 -n public -d build/dev --skip-unchanged --json --report dist/apply-report.json"#
)]
struct Cli {
    /// 配置文件路径（覆盖默认发现路径）
    #[arg(long)]
    config: Option<String>,

    /// 激活的配置 profile 名称
    #[arg(long)]
    profile: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// 使用变量渲染模板，输出到目录或标准输出
    #[command(
        about = "使用变量渲染模板，输出到目录或标准输出",
        long_about = r#"渲染模板（Tera + 兼容 ${VAR} 语法）🧩

示例：
- 单文件到标准输出：
  nacos-tpl render -t examples/template -v examples/vars.yaml --print DEFAULT_GROUP/app.yaml --stdout
- 单文件到路径：
  nacos-tpl render -t examples/template -v examples/vars.yaml --print DEFAULT_GROUP/app.yaml --output-file build/app.yaml
- 目录渲染（并写 manifest.yaml）：
  nacos-tpl render -t examples/template -v examples/vars.yaml -o build/dev --write-manifest
- 子集渲染（include/exclude）：
  nacos-tpl render -t template -v variables.yaml -o build/dev --include "**/*.yml,**/*.properties" --exclude "**/test/**"
"#,
        after_help = r#"示例👇
  单文件到标准输出：
    nacos-tpl render -t examples/template -v examples/vars.yaml --print DEFAULT_GROUP/app.yaml --stdout
  目录渲染并写 manifest.yaml：
    nacos-tpl render -t examples/template -v examples/vars.yaml -o build/dev --write-manifest"#
    )]
    Render {
        /// 模板目录
        #[arg(short = 't', long = "template")]
        template: String,

        /// 变量 YAML 文件
        #[arg(short = 'v', long = "vars")]
        vars: Option<String>,

        /// 渲染输出目录
        #[arg(short = 'o', long = "output")]
        output: Option<String>,

        /// 将单个渲染结果输出到该文件（需配合 --print）
        #[arg(long = "output-file")]
        output_file: Option<String>,

        /// 打印单个条目（模板内相对路径）到标准输出
        #[arg(long = "print")]
        print: Option<String>,

        /// 按 dataId 或 manifest.yaml 选择（可配合 --group）
        #[arg(long = "print-id")]
        print_id: Option<String>,
        /// 与 --print-id 搭配用于消歧的 group
        #[arg(long = "group")]
        group: Option<String>,

        /// 强制输出到标准输出（需配合 --print 或单文件选择）
        #[arg(long = "stdout", action = ArgAction::SetTrue)]
        stdout: bool,

        /// 允许缺失变量（若有默认值则使用默认）
        #[arg(long = "allow-missing", action = ArgAction::SetTrue)]
        allow_missing: bool,

        /// 仅包含匹配这些 glob 的文件（相对于模板目录）
        #[arg(long = "include", num_args = 1.., value_delimiter = ',')]
        include: Vec<String>,
        /// 排除匹配这些 glob 的文件
        #[arg(long = "exclude", num_args = 1.., value_delimiter = ',')]
        exclude: Vec<String>,

        /// 目录渲染后写出 manifest.yaml（path/dataId/group/type 映射）
        #[arg(long = "write-manifest", action = ArgAction::SetTrue)]
        write_manifest: bool,
    },

    /// 从导出包或目录初始化模板
    #[command(
        about = "从导出包或目录初始化模板",
        long_about = r#"初始化模板（目录或 zip 输入）🧭

示例：
- 从目录初始化：
  nacos-tpl init -i examples/export_dir -o template/ -r mapping.rules.yaml
- 从 zip 初始化：
  nacos-tpl init -i export.zip -o template/ -r mapping.rules.yaml
"#,
        after_help = r#"示例👇
  从目录初始化：
    nacos-tpl init -i examples/export_dir -o template/ -r mapping.rules.yaml"#
    )]
    Init {
        /// 输入：目录或 zip
        #[arg(short = 'i', long = "input")]
        input: String,
        /// 输出模板目录
        #[arg(short = 'o', long = "output")]
        output: String,
        /// 规则文件（YAML），指导替换
        #[arg(short = 'r', long = "rules")]
        rules: Option<String>,
    },

    /// 校验模板与规则
    #[command(
        about = "校验模板与规则",
        long_about = r#"校验模板（必填变量 + 残留占位符）🧪

示例：
- 基础校验：
  nacos-tpl validate -t ./templates -v ./vars/dev.yaml
- 严格模式（渲染后不允许残留占位符）：
  nacos-tpl validate -t ./templates -v ./vars/dev.yaml --strict
- 指定必填变量规则：
  nacos-tpl validate -t ./templates -v ./vars/dev.yaml -r ./rules.yaml
- 输出 JSON + 写报告：
  nacos-tpl validate -t ./templates -v ./vars/dev.yaml --json --report ./build/validate_report.json

规则文件（rules.yaml）示例：
required:\n  - DB_HOST\n  - DB_PORT\n  - REDIS_URL\n  - SPRING_PROFILES_ACTIVE
"#,
        after_help = r#"示例👇
  严格模式 + 规则：
    nacos-tpl validate -t ./templates -v ./vars/dev.yaml --strict -r ./rules.yaml
  JSON 报告：
    nacos-tpl validate -t ./templates -v ./vars/dev.yaml --json --report ./build/validate_report.json"#
    )]
    Validate {
        #[arg(short = 't', long = "template")]
        template: String,
        #[arg(short = 'v', long = "vars")]
        vars: String,
        /// 可选规则文件，提供必填变量列表
        #[arg(short = 'r', long = "rules")]
        rules: Option<String>,
        /// 严格模式：渲染后若仍存在占位符则失败
        #[arg(long = "strict", action = ArgAction::SetTrue)]
        strict: bool,
        /// 输出 JSON 报告
        #[arg(long = "json", action = ArgAction::SetTrue)]
        json: bool,
        /// 将 JSON 报告写入文件
        #[arg(long = "report")]
        report: Option<String>,
    },

    /// 通过 OpenAPI 发布配置
    #[command(
        about = "通过 OpenAPI 发布配置",
        long_about = r#"发布配置到 Nacos（谨慎操作生产）🚀

示例：
- 并发发布 + JSON 报告：
  nacos-tpl apply -s http://127.0.0.1:8848 -n public -d build/dev \
    --skip-unchanged --retries 3 --concurrency 5 --timeout-ms 10000 \
    --normalize-lf --max-bytes 1048576 \
    --json --report dist/apply-report.json
"#,
        after_help = r#"示例👇
  空跑（不发布，只计算变化）：
    nacos-tpl apply -s http://127.0.0.1:8848 -n public -d build/dev --dry-run"#
    )]
    Apply {
        /// 服务地址（支持逗号分隔多个）
        #[arg(short = 's', long = "server")]
        server: Option<String>,
        /// 命名空间（tenant）
        #[arg(short = 'n', long = "namespace")]
        namespace: Option<String>,
        /// 用户名（用于登录获取 token，默认 nacos）
        /// 密码（用于登录获取 token，默认 nacos）
        /// 用户名（用于登录获取 token，默认 nacos）
        #[arg(short = 'u', long = "username")]
        username: Option<String>,
        /// 密码（用于登录获取 token，默认 nacos）
        #[arg(short = 'p', long = "password")]
        password: Option<String>,
        /// 渲染目录
        #[arg(short = 'd', long = "dir")]
        dir: String,
        /// 用户名（用于登录获取 token，默认 nacos）
        /// 密码（用于登录获取 token，默认 nacos）
        /// 用户名（用于登录获取 token，默认 nacos）
        /// 密码（用于登录获取 token，默认 nacos）
        /// 用户名（用于登录获取 token，默认 nacos）
        /// 密码（用于登录获取 token，默认 nacos）
        /// 与远端内容一致则跳过
        #[arg(long = "skip-unchanged", action = ArgAction::SetTrue)]
        skip_unchanged: bool,
        /// 并发数（默认 5）
        #[arg(long = "concurrency", default_value_t = 5)]
        concurrency: usize,
        /// 失败重试次数（默认 3）
        #[arg(long = "retries", default_value_t = 3)]
        retries: usize,
        /// 请求超时毫秒（默认来自配置或 10000）
        #[arg(long = "timeout-ms")]
        timeout_ms: Option<u64>,
        /// 输出结构化 JSON 报告
        #[arg(long = "json", action = ArgAction::SetTrue)]
        json: bool,
        /// 将 JSON 报告写入文件
        #[arg(long = "report")]
        report: Option<String>,
        /// 报告写入模式：overwrite|append|timestamp
        #[arg(long = "report-mode")]
        report_mode: Option<String>,
        /// 存在失败项时返回非零退出码
        #[arg(long = "fail-on-error", action = ArgAction::SetTrue)]
        fail_on_error: bool,
        /// 空跑：不发布，仅计算变化
        #[arg(long = "dry-run", action = ArgAction::SetTrue)]
        dry_run: bool,
        /// 单项内容大小上限（字节），超出则失败并跳过
        #[arg(long = "max-bytes")]
        max_bytes: Option<usize>,
        /// 统一换行为 LF 再发布
        #[arg(long = "normalize-lf", action = ArgAction::SetTrue)]
        normalize_lf: bool,
        /// 报告中包含本地 MD5 与字节大小
        #[arg(long = "include-md5", action = ArgAction::SetTrue)]
        include_md5: bool,
        /// 允许覆盖远端已存在的配置（默认不覆盖）
        #[arg(long = "overwrite", action = ArgAction::SetTrue)]
        overwrite: bool,
        /// 当命名空间不存在时自动创建（默认不创建）
        #[arg(long = "create-namespace", action = ArgAction::SetTrue)]
        create_namespace: bool,
    },

    /// 对比远端与本地
    #[command(
        about = "对比远端与本地",
        long_about = r#"对比本地渲染结果与远端配置（文本级）🔍

示例：
- 统一 diff（默认差异展示）：
  nacos-tpl diff-remote -s http://127.0.0.1:8848 -n public -d build/dev --only-changed
- 左右对比：
  nacos-tpl diff-remote -s http://127.0.0.1:8848 -d build/dev --only-changed --diff-style side-by-side
- 只看新增：
  nacos-tpl diff-remote -s http://127.0.0.1:8848 -n public -d build/dev --only-added
- 过滤文件：
  nacos-tpl diff-remote -s http://127.0.0.1:8848 -n public -d build/dev --include "**/*.yml" --exclude "**/test/**"

说明：-s 支持不带协议地址（自动补全 http://）；会自动登录获取 accessToken 后再请求。
"#,
        after_help = r#"示例👇
  统一 diff（默认差异展示）：
    nacos-tpl diff-remote -s http://127.0.0.1:8848 -d build/dev --only-changed
  左右对比：
    nacos-tpl diff-remote -s http://127.0.0.1:8848 -d build/dev --only-changed --diff-style side-by-side"#
    )]
    DiffRemote {
        /// 服务地址（支持逗号分隔多个）
        #[arg(short = 's', long = "server")]
        server: String,
        /// 命名空间（tenant）
        #[arg(short = 'n', long = "namespace")]
        namespace: Option<String>,
        /// 本地目录
        #[arg(short = 'd', long = "dir")]
        dir: String,
        /// 用户名（用于登录获取 token，默认 nacos）
        #[arg(short = 'u', long = "username", default_value = "nacos")]
        username: String,
        /// 密码（用于登录获取 token，默认 nacos）
        #[arg(short = 'p', long = "password", default_value = "nacos")]
        password: String,
        /// 失败重试次数（默认 3）
        #[arg(long = "retries", default_value_t = 3)]
        retries: usize,
        /// 请求超时毫秒（默认 10000）
        #[arg(long = "timeout-ms", default_value_t = 10000)]
        timeout_ms: u64,
        /// 仅包含匹配这些 glob 的文件（相对于目录）
        #[arg(long = "include", num_args = 1.., value_delimiter = ',')]
        include: Vec<String>,
        /// 排除匹配这些 glob 的文件
        #[arg(long = "exclude", num_args = 1.., value_delimiter = ',')]
        exclude: Vec<String>,
        /// 输出 JSON 报告
        #[arg(long = "json", action = ArgAction::SetTrue)]
        json: bool,
        /// 将 JSON 报告写入文件
        #[arg(long = "report")]
        report: Option<String>,
        /// 对已变更项显示差异（默认启用）；可用 --no-show-changed 关闭
        #[arg(long = "show-changed", action = ArgAction::SetTrue, default_value_t = true)]
        show_changed: bool,
        /// 关闭变更项差异输出（覆盖 --show-changed）
        #[arg(long = "no-show-changed", action = ArgAction::SetTrue)]
        no_show_changed: bool,
        /// 统一 diff 的上下文行数
        #[arg(long = "context", default_value_t = 3)]
        context: usize,
        /// 变更内容展示风格：unified|side-by-side
        #[arg(long = "diff-style")]
        diff_style: Option<String>,
        /// 仅打印 changed/added（省略汇总）
        #[arg(long = "only-changed", action = ArgAction::SetTrue)]
        only_changed: bool,
        /// 仅打印新增项
        #[arg(long = "only-added", action = ArgAction::SetTrue)]
        only_added: bool,
        /// 按 group 分组列出变更/新增（非 JSON 输出）
        #[arg(long = "grouped", action = ArgAction::SetTrue)]
        grouped: bool,
    },

    /// 将渲染结果打包为可在控制台导入的 zip
    #[command(
        about = "将渲染结果打包为可在控制台导入的 zip",
        long_about = r#"打包渲染结果为 zip，便于 Nacos 控制台导入 📦

示例：
- 最小示例：
  nacos-tpl package -t template/ -v examples/vars.yaml -o dist/dev.zip
"#,
        after_help = r#"示例👇
  打包：
    nacos-tpl package -t template/ -v examples/vars.yaml -o dist/dev.zip"#
    )]
    Package {
        /// 模板目录
        #[arg(short = 't', long = "template")]
        template: String,
        /// 变量 YAML 文件
        #[arg(short = 'v', long = "vars")]
        vars: String,
        /// 输出 zip 路径
        #[arg(short = 'o', long = "output")]
        output: String,
        /// 允许缺失变量（若有默认值则使用默认）
        #[arg(long = "allow-missing", action = ArgAction::SetTrue)]
        allow_missing: bool,
    },
}

fn init_tracing() {
    // 兼容不同终端：优先读取 RUST_LOG，其次 NACOS_TPL_LOG；解析失败回落到 info
    let raw = std::env::var("RUST_LOG")
        .or_else(|_| std::env::var("NACOS_TPL_LOG"))
        .ok()
        .map(|v| v.trim().to_string());
    let filter = if let Some(s) = raw {
        // 若仅给出级别名，则限定到本 crate，避免依赖库英文日志干扰
        let lv = s.to_ascii_lowercase();
        let s2 = match lv.as_str() {
            "trace" | "debug" | "info" | "warn" | "error" => {
                format!("nacos_tpl={lv},reqwest=warn,hyper=warn")
            }
            _ => s,
        };
        EnvFilter::try_new(s2).unwrap_or_else(|_| EnvFilter::new("info"))
    } else {
        EnvFilter::new("info")
    };
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}

#[allow(clippy::collapsible_else_if)]
#[allow(unused_variables)]
fn run(cli: Cli) -> Result<()> {
    // 兜底默认值：用于非 DiffRemote 分支内的误用场景（不会影响实际逻辑）
    let (show_changed, no_show_changed) = (false, false);
    match cli.command {
        Commands::Render {
            template,
            vars,
            output,
            output_file,
            print,
            print_id,
            group,
            stdout,
            allow_missing,
            include,
            exclude,
            write_manifest,
        } => {
            let cfg = cfg::load_effective(cli.config.as_deref(), cli.profile.as_deref())
                .context("🐛 加载配置失败")?;
            let defaults = cfg.render_defaults();

            let vars_path = vars.or(defaults.variables_file).context(
                "⚠️ 需要提供变量文件：命令行 --vars 或配置 render.defaults.variables_file",
            )?;

            if output_file.is_some() && output.is_some() {
                bail!("⚠️ --output 与 --output-file 不能同时使用");
            }

            // 单文件渲染：输出到文件或标准输出
            if print.is_some() || print_id.is_some() {
                let sel = if let Some(id) = print_id.as_ref() {
                    render::resolve_by_id(&template, id, group.as_deref())?
                } else {
                    print.unwrap()
                };
                let rendered =
                    render::render_single(&template, &vars_path, &cfg, &sel, allow_missing)?;
                if let Some(of) = output_file {
                    std::fs::create_dir_all(
                        std::path::Path::new(&of)
                            .parent()
                            .unwrap_or_else(|| std::path::Path::new(".")),
                    )
                    .ok();
                    std::fs::write(&of, rendered)
                        .with_context(|| format!("📝 写入文件失败：{}", of))?;
                    return Ok(());
                }
                // 默认：标准输出
                let mut out = std::io::stdout();
                use std::io::Write as _;
                out.write_all(rendered.as_bytes())?;
                return Ok(());
            }

            // 未选择单个条目却要求 stdout -> 不允许
            if stdout {
                bail!("⚠️ --stdout 需配合 --print <相对路径> 或 --print-id <dataId> 使用");
            }

            // 目录渲染
            if let Some(out_dir) = output.or(defaults.output_dir) {
                let mode = render::Mode::Directory {
                    out_dir: out_dir.clone(),
                };
                render::render_all(
                    &template,
                    &vars_path,
                    &cfg,
                    mode,
                    allow_missing,
                    &include,
                    &exclude,
                )?;
                if write_manifest {
                    render::write_manifest_output(&out_dir, &cfg)?;
                }
                return Ok(());
            }

            bail!("⚠️ 请选择 --print（可配合 --output-file）或指定 --output 目录")
        }
        Commands::Init {
            input,
            output,
            rules,
        } => {
            let cfg = cfg::load_effective(cli.config.as_deref(), cli.profile.as_deref())
                .context("🐛 加载配置失败")?;
            init::run_init(&input, &output, rules.as_deref(), &cfg)
        }
        Commands::Validate {
            template,
            vars,
            rules,
            strict,
            json,
            report,
        } => {
            let rep = validate::validate_report(&template, &vars, rules.as_deref(), strict)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&rep)?);
            } else {
                if rep.ok {
                    println!("✅ 校验通过");
                } else {
                    println!("🐛 校验失败：存在未解析项（{}）", rep.missing.len());
                    for e in &rep.missing {
                        println!("  {}", e);
                    }
                }
            }
            if let Some(p) = report {
                let s = serde_json::to_string_pretty(&rep)?;
                std::fs::write(&p, s).with_context(|| format!("📝 写入报告失败：{}", p))?;
            }
            if !rep.ok {
                bail!("校验失败（{} 项）", rep.missing.len());
            }
            Ok(())
        }
        Commands::Apply {
            server,
            namespace,
            username,
            password,
            dir,
            skip_unchanged,
            concurrency,
            retries,
            timeout_ms,
            json,
            report,
            report_mode,
            fail_on_error,
            dry_run,
            max_bytes,
            normalize_lf,
            include_md5,
            overwrite,
            create_namespace,
        } => {
            let mut cfg = cfg::load_effective(cli.config.as_deref(), cli.profile.as_deref())
                .context("🐛 加载配置失败")?;
            if let Some(u) = username {
                cfg.username = Some(u);
            }
            if let Some(p) = password {
                cfg.password = Some(p);
            }
            let timeout = timeout_ms.or(cfg.timeout_ms).unwrap_or(10_000);
            let report_struct = apply::apply_dir_opts(
                &dir,
                &cfg,
                server.as_deref(),
                namespace.as_deref(),
                skip_unchanged,
                concurrency,
                retries,
                timeout,
                dry_run,
                max_bytes,
                normalize_lf,
                include_md5,
                overwrite,
                create_namespace,
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report_struct)?);
            } else {
                println!(
                    "📊 发布汇总：已更新={} 已跳过={} 失败={}",
                    report_struct.updated, report_struct.skipped, report_struct.failed
                );
                if !report_struct.groups.is_empty() {
                    println!("📦 按组统计：");
                    for g in &report_struct.groups {
                        println!(
                            "  {}：已更新={} 已跳过={} 失败={}",
                            g.group, g.updated, g.skipped, g.failed
                        );
                    }
                }
                if report_struct.failed > 0 {
                    println!("❌ 失败条目：");
                    for it in &report_struct.items {
                        if it.status == "failed" {
                            if let Some(r) = &it.reason {
                                println!("  {}/{} -> {}", it.group, it.dataId, r);
                            } else {
                                println!("  {}/{}", it.group, it.dataId);
                            }
                        }
                    }
                }
            }
            // 写入报告（按模式）
            if let Some(p) = report {
                let s = serde_json::to_string_pretty(&report_struct)? + "\n";
                match report_mode.as_deref() {
                    Some("append") => {
                        use std::io::Write;
                        let mut f = std::fs::OpenOptions::new()
                            .create(true)
                            .append(true)
                            .open(&p)
                            .with_context(|| format!("📄 打开报告失败用于追加：{}", p))?;
                        f.write_all(s.as_bytes())
                            .with_context(|| format!("📄 追加写入报告失败：{}", p))?;
                    }
                    Some("timestamp") => {
                        let ts = chrono::Local::now().format("%Y%m%d_%H%M%S");
                        let path = std::path::Path::new(&p);
                        let new_name = if let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                        {
                            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                                format!("{}_{}.{}", stem, ts, ext)
                            } else {
                                format!("{}_{}", stem, ts)
                            }
                        } else {
                            format!("{}_{}", p, ts)
                        };
                        let new_path = path.with_file_name(new_name);
                        std::fs::write(&new_path, s)
                            .with_context(|| format!("📝 写入报告失败：{}", new_path.display()))?;
                    }
                    Some("overwrite") | None => {
                        std::fs::write(&p, s).with_context(|| format!("📝 写入报告失败：{}", p))?;
                    }
                    Some(other) => {
                        bail!(
                            "⚠️ 无效的 --report-mode: {}（可选：overwrite|append|timestamp）",
                            other
                        );
                    }
                }
            }
            if fail_on_error && report_struct.failed > 0 {
                bail!("发布失败项：{} ❌", report_struct.failed);
            }
            Ok(())
        }
        Commands::DiffRemote {
            server,
            namespace,
            dir,
            username,
            password,
            retries,
            timeout_ms,
            include,
            exclude,
            json,
            report,
            show_changed,
            no_show_changed,
            context,
            diff_style,
            only_changed,
            only_added,
            grouped,
        } => {
            // 加载配置以支持鉴权（如用户名/密码 -> token）
            let cfg = cfg::load_effective(cli.config.as_deref(), cli.profile.as_deref())
                .context("🐛 加载配置失败")?;
            // 解析 servers 列表
            let servers: Vec<String> = server
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| {
                    if s.starts_with("http://") || s.starts_with("https://") {
                        s.to_string()
                    } else {
                        format!("http://{}", s)
                    }
                })
                .collect();
            let timeout = timeout_ms;
            let (insecure, ca_cert) = cfg
                .tls
                .as_ref()
                .map(|t| (t.insecure, t.ca_cert.clone()))
                .unwrap_or((None, None));
            let http = nacos::ReqwestNacosHttp::new(timeout, insecure, ca_cert.as_deref())
                .context("🐛 初始化 HTTP 客户端失败")?;
            let mut token_q: Option<String> =
                cfg.token.clone().map(|t| format!("accessToken={}", t));
            if token_q.is_none() {
                // 轮询登录获取 token（使用 CLI 用户名/密码，默认 nacos/nacos）
                for s in &servers {
                    if let Ok(tok) = http.login(s, &username, &password) {
                        token_q = Some(format!("accessToken={}", tok));
                        break;
                    }
                }
            }

            // 命名空间优先级：命令行 -n > -d/.metadata.yml
            let ns_eff = if let Some(ns) = namespace {
                ns
            } else {
                infer_namespace_from_metadata_dir(&dir).with_context(|| {
                    format!(
                        "⚠️ 未传入 -n，且 {} 中未找到 tenant；请使用 -n 或在 .metadata.yml 指定",
                        std::path::Path::new(&dir).join(".metadata.yml").display()
                    )
                })?
            };

            let rep = diff::diff_dir(
                &server,
                &ns_eff,
                &dir,
                retries,
                timeout_ms,
                &include,
                &exclude,
                token_q.as_deref(),
            )?;
            if json {
                println!("{}", serde_json::to_string_pretty(&rep)?);
            } else {
                if !only_changed {
                    println!(
                        "📊 对比汇总：相同={} 变更={} 新增={} 错误={}",
                        rep.same, rep.changed, rep.added, rep.errors
                    );
                    if !rep.groups.is_empty() {
                        println!("📦 按组统计：");
                        for g in &rep.groups {
                            println!(
                                "  {}：相同={} 变更={} 新增={}",
                                g.group, g.same, g.changed, g.added
                            );
                        }
                    }
                }
                if rep.errors > 0 {
                    println!("❌ 请求失败条目（HTTP/权限等）：");
                    for it in &rep.items {
                        if it.status == "error" {
                            if let Some(r) = &it.reason {
                                println!("  {}/{} -> {}", it.group, it.dataId, r);
                            } else {
                                println!("  {}/{}", it.group, it.dataId);
                            }
                        }
                    }
                }
                if !only_added && rep.changed > 0 {
                    if grouped {
                        println!("📝 变更条目（分组）：");
                        let mut current = String::new();
                        for it in &rep.items {
                            if it.status != "changed" {
                                continue;
                            }
                            if it.group != current {
                                current = it.group.clone();
                                println!("  {}", current);
                            }
                            println!("    {}", it.dataId);
                        }
                    } else {
                        println!("📝 变更条目：");
                        for it in &rep.items {
                            if it.status == "changed" {
                                println!("  {}/{}", it.group, it.dataId);
                            }
                        }
                    }
                    if show_changed {
                        for it in &rep.items {
                            if it.status == "changed" {
                                let local_path =
                                    std::path::Path::new(&dir).join(&it.group).join(&it.dataId);
                                let local =
                                    std::fs::read_to_string(&local_path).unwrap_or_default();
                                if let Ok(Some(remote)) = diff::fetch_remote_text(
                                    &server,
                                    &ns_eff,
                                    &it.group,
                                    &it.dataId,
                                    retries,
                                    timeout_ms,
                                    token_q.as_deref(),
                                ) {
                                    println!("--- 本地: {}/{}", it.group, it.dataId);
                                    println!("+++ 远端: {}/{}", it.group, it.dataId);
                                    match diff_style.as_deref() {
                                        Some("side-by-side") | Some("sxs") | Some("side") => {
                                            print_side_by_side_diff(&local, &remote, context);
                                        }
                                        _ => {
                                            print_unified_diff(&local, &remote, context);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if rep.added > 0 {
                    if grouped {
                        println!("📝 新增条目（分组）：");
                        let mut current = String::new();
                        for it in &rep.items {
                            if it.status != "added" {
                                continue;
                            }
                            if it.group != current {
                                current = it.group.clone();
                                println!("  {}", current);
                            }
                            println!("    {}", it.dataId);
                        }
                    } else {
                        println!("📝 新增条目：");
                        for it in &rep.items {
                            if it.status == "added" {
                                println!("  {}/{}", it.group, it.dataId);
                            }
                        }
                    }
                }
            }
            if let Some(p) = report {
                let s = serde_json::to_string_pretty(&rep)?;
                std::fs::write(&p, s).with_context(|| format!("📝 写入报告失败：{}", p))?;
            }
            Ok(())
        }
        Commands::Package {
            template,
            vars,
            output,
            allow_missing,
        } => {
            let cfg = cfg::load_effective(cli.config.as_deref(), cli.profile.as_deref())
                .context("🐛 加载配置失败")?;
            package::package_zip(&template, &vars, &cfg, &output, allow_missing)
        }
    }
}

#[allow(clippy::collapsible_match, clippy::needless_borrows_for_generic_args)]
fn infer_namespace_from_metadata_dir(dir: &str) -> Option<String> {
    let p = std::path::Path::new(dir).join(".metadata.yml");
    if !p.exists() {
        return None;
    }
    let txt = std::fs::read_to_string(&p).ok()?;
    let yaml: serde_yaml::Value = serde_yaml::from_str(&txt).ok()?;
    if let serde_yaml::Value::Mapping(map) = &yaml {
        if let Some(v) = map.get(&serde_yaml::Value::String("tenant".to_string())) {
            if let serde_yaml::Value::String(s) = v {
                if !s.is_empty() {
                    return Some(s.clone());
                }
            }
        }
    }
    if let Some(meta) = yaml.get("metadata") {
        if let serde_yaml::Value::Sequence(items) = meta {
            let mut set = std::collections::BTreeSet::new();
            for it in items {
                if let serde_yaml::Value::Mapping(m) = it {
                    if let Some(v) = m.get(&serde_yaml::Value::String("tenant".to_string())) {
                        if let serde_yaml::Value::String(s) = v {
                            if !s.is_empty() {
                                set.insert(s.clone());
                            }
                        }
                    }
                }
            }
            if set.len() == 1 {
                return set.into_iter().next();
            }
        }
    }
    None
}

fn print_unified_diff(local: &str, remote: &str, context: usize) {
    let diff = TextDiff::from_lines(remote, local); // remote vs local
    for group in diff.grouped_ops(context) {
        println!("@@");
        for op in group {
            for change in diff.iter_changes(&op) {
                let line = match change.tag() {
                    similar::ChangeTag::Delete => format!("-{}", change.value()).red().to_string(),
                    similar::ChangeTag::Insert => {
                        format!("+{}", change.value()).green().to_string()
                    }
                    similar::ChangeTag::Equal => {
                        format!(" {}", change.value()).dimmed().to_string()
                    }
                };
                print!("{}", line);
            }
        }
    }
}

#[allow(clippy::collapsible_str_replace)]
fn print_side_by_side_diff(local: &str, remote: &str, context: usize) {
    let diff = TextDiff::from_lines(remote, local); // 左列: 远端, 右列: 本地
    let left_w: usize = 60;
    let sep = " | ";

    let clean = |s: &str| -> String { s.replace(['\r', '\n'], "") };
    let fmt = |s: &str, w: usize| -> String {
        let mut t = clean(s);
        if t.chars().count() > w {
            t = t.chars().take(w.saturating_sub(1)).collect();
            t.push('…');
        }
        let pad = w.saturating_sub(t.chars().count());
        format!("{}{}", t, " ".repeat(pad))
    };

    for group in diff.grouped_ops(context) {
        println!("@@");
        let mut left_lines: Vec<String> = Vec::new();
        let mut right_lines: Vec<String> = Vec::new();
        let mut left_marks: Vec<char> = Vec::new();
        let mut right_marks: Vec<char> = Vec::new();
        for op in group {
            for ch in diff.iter_changes(&op) {
                match ch.tag() {
                    similar::ChangeTag::Equal => {
                        let v = ch.value();
                        left_lines.push(v.to_string());
                        right_lines.push(v.to_string());
                        left_marks.push(' ');
                        right_marks.push(' ');
                    }
                    similar::ChangeTag::Delete => {
                        left_lines.push(ch.value().to_string());
                        right_lines.push(String::new());
                        left_marks.push('-');
                        right_marks.push(' ');
                    }
                    similar::ChangeTag::Insert => {
                        left_lines.push(String::new());
                        right_lines.push(ch.value().to_string());
                        left_marks.push(' ');
                        right_marks.push('+');
                    }
                }
            }
        }
        let rows = left_lines.len().max(right_lines.len());
        for i in 0..rows {
            let l = left_lines.get(i).map(|s| s.as_str()).unwrap_or("");
            let r = right_lines.get(i).map(|s| s.as_str()).unwrap_or("");
            let lm = *left_marks.get(i).unwrap_or(&' ');
            let rm = *right_marks.get(i).unwrap_or(&' ');
            let (left_colored, lmark) = match lm {
                '-' => (fmt(l, left_w).red().to_string(), "-".red().to_string()),
                '+' => (fmt(l, left_w).green().to_string(), "+".green().to_string()),
                _ => (
                    fmt(l, left_w).dimmed().to_string(),
                    " ".dimmed().to_string(),
                ),
            };
            let (right_colored, rmark) = match rm {
                '-' => (clean(r).red().to_string(), "-".red().to_string()),
                '+' => (clean(r).green().to_string(), "+".green().to_string()),
                _ => (clean(r).dimmed().to_string(), " ".dimmed().to_string()),
            };
            println!("{}{}{}{}{}", lmark, left_colored, sep, rmark, right_colored);
        }
    }
}

fn classify_exit_code(err: &anyhow::Error) -> i32 {
    let s = err.to_string();
    // 4: 网络/认证/调用失败
    if s.contains("login failed")
        || s.contains("apply failed")
        || s.contains("remote get failed")
        || s.contains("all servers failed")
        || s.contains("登录失败")
        || s.contains("发布失败")
        || s.contains("获取失败")
        || s.contains("远端获取失败")
        || s.contains("所有服务器节点")
    {
        return 4;
    }
    if err.downcast_ref::<reqwest::Error>().is_some() {
        return 4;
    }

    // 3: IO/解压/解析失败
    if err.downcast_ref::<std::io::Error>().is_some()
        || err.downcast_ref::<serde_yaml::Error>().is_some()
        || err.downcast_ref::<serde_json::Error>().is_some()
        || err.downcast_ref::<zip::result::ZipError>().is_some()
    {
        return 3;
    }
    if s.contains("unzip")
        || s.contains("解压")
        || s.contains("读取")
        || s.contains("写入")
        || s.contains("打开")
    {
        return 3;
    }

    // 2: 渲染/校验失败
    if err.downcast_ref::<tera::Error>().is_some() {
        return 2;
    }
    if s.contains("missing variables")
        || s.contains("缺失变量")
        || s.contains("validation failed")
        || s.contains("校验失败")
        || s.contains("占位符")
    {
        return 2;
    }

    // 1: 参数校验等其他失败
    1
}

fn main() {
    init_tracing();
    // 使用 clap 的内建处理：--help/--version 自动打印并以 0 退出
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("❌ 错误: {}", e);
        let code = classify_exit_code(&e);
        std::process::exit(code);
    }
}
