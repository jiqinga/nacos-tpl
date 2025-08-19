use anyhow::{bail, Context, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use serde::Serialize;
use serde_yaml::Value as YamlValue;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};
use tera::Tera;
use walkdir::WalkDir;

#[derive(Debug, Deserialize)]
struct RulesReq {
    #[serde(default)]
    required: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ValidateReport {
    pub ok: bool,
    pub missing: Vec<String>,
}

pub fn validate_report(
    template_dir: &str,
    vars_path: &str,
    rules_path: Option<&str>,
    strict: bool,
) -> Result<ValidateReport> {
    let template_dir = PathBuf::from(template_dir);
    if !template_dir.is_dir() {
        bail!("模板目录未找到: {}", template_dir.display());
    }

    let vars = load_vars_merged(vars_path).context("加载变量失败")?;
    let required = if let Some(rp) = rules_path {
        load_required(Path::new(rp))?
    } else {
        Vec::new()
    };

    let mut tera = Tera::default();
    let mut names: Vec<String> = Vec::new();
    for entry in WalkDir::new(&template_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() {
            let rel = path
                .strip_prefix(&template_dir)
                .unwrap()
                .to_string_lossy()
                .to_string();
            let content = fs::read_to_string(path)
                .with_context(|| format!("读取模板: {}", path.display()))?;
            tera.add_raw_template(&rel, &content)
                .with_context(|| format!("注册模板: {}", rel))?;
            names.push(rel);
        }
    }

    let tctx = to_tera_context(&vars);
    let mut missing: Vec<String> = Vec::new();
    // Check required keys
    for k in required {
        if !vars.contains_key(&k) {
            missing.push(format!("必填缺失:{}", k));
        }
    }
    for n in names {
        match tera.render(&n, &tctx) {
            Ok(s) => {
                if strict {
                    // Strict: no ${...} remains at all
                    if let Some(ph) = find_any_placeholder(&s) {
                        missing.push(format!("占位符:{} 于 {}", ph, n));
                    }
                } else {
                    // After Tera, check for ${VAR} without default; allow defaulted ones
                    for var in find_compat_vars_without_default(&s) {
                        if !vars.contains_key(&var) {
                            missing.push(format!("缺失变量:{} 于 {}", var, n));
                        }
                    }
                }
            }
            Err(e) => {
                // Tera error often mentions missing variable names
                missing.push(format!("模板渲染失败 {}: {}", n, e));
            }
        }
    }

    missing.sort();
    missing.dedup();
    Ok(ValidateReport {
        ok: missing.is_empty(),
        missing,
    })
}

fn to_tera_context(vars: &HashMap<String, String>) -> tera::Context {
    let mut ctx = tera::Context::new();
    for (k, v) in vars {
        ctx.insert(k, v);
    }
    ctx
}

// removed unused RE_COMPAT_VARS
static RE_COMPAT_VARS_NODEFAULT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\$\{\s*([A-Z0-9_]+)\s*\}").unwrap());
static RE_ANY_PLACEHOLDER: Lazy<Regex> = Lazy::new(|| Regex::new(r"\$\{[^}]+\}").unwrap());

fn find_compat_vars_without_default(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    for caps in RE_COMPAT_VARS_NODEFAULT.captures_iter(s) {
        let key = caps.get(1).unwrap().as_str().to_string();
        out.push(key);
    }
    out
}

fn find_any_placeholder(s: &str) -> Option<String> {
    RE_ANY_PLACEHOLDER
        .captures(s)
        .map(|c| c.get(0).unwrap().as_str().to_string())
}

fn load_vars_merged(path: &str) -> Result<HashMap<String, String>> {
    use std::ffi::OsStr;
    let mut out: HashMap<String, String> = HashMap::new();
    let p = PathBuf::from(path);
    if p.is_dir() {
        let mut files: Vec<_> = std::fs::read_dir(&p)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|pp| matches!(pp.extension().and_then(OsStr::to_str), Some("yml" | "yaml")))
            .collect();
        files.sort();
        for f in files {
            merge_yaml_file(&f, &mut out)?;
        }
        // 环境变量覆盖同名键
        overlay_env(&mut out);
        return Ok(out);
    }
    // file first (lower precedence)
    merge_yaml_file(&p, &mut out)?;
    // overlay with same-name .d directory
    if let (Some(stem), Some(dir)) = (p.file_stem().and_then(OsStr::to_str), p.parent()) {
        let d = dir.join(format!("{}.d", stem));
        if d.is_dir() {
            let mut files: Vec<_> = std::fs::read_dir(&d)?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|pp| matches!(pp.extension().and_then(OsStr::to_str), Some("yml" | "yaml")))
                .collect();
            files.sort();
            for f in files {
                merge_yaml_file(&f, &mut out)?;
            }
        }
    }
    // 环境变量覆盖同名键
    overlay_env(&mut out);
    Ok(out)
}

fn merge_yaml_file(path: &Path, out: &mut HashMap<String, String>) -> Result<()> {
    if !path.is_file() {
        return Ok(());
    }
    let txt = fs::read_to_string(path)
        .with_context(|| format!("读取变量文件失败: {}", path.display()))?;
    let yaml: YamlValue = serde_yaml::from_str(&txt).context("解析变量 YAML 失败 🐛")?;
    flatten_yaml(None, &yaml, out);
    Ok(())
}

fn overlay_env(out: &mut HashMap<String, String>) {
    for (k, v) in std::env::vars() {
        let key = k.to_uppercase();
        if out.contains_key(&key) {
            out.insert(key, v);
        }
    }
}

fn load_required(path: &Path) -> Result<Vec<String>> {
    let txt =
        fs::read_to_string(path).with_context(|| format!("读取规则失败: {}", path.display()))?;
    let r: RulesReq = serde_yaml::from_str(&txt).context("解析规则 YAML 失败(required)")?;
    Ok(r.required)
}

// Backward-compatible runner used by earlier code paths (if any)
#[allow(dead_code)]
pub fn run_validate(
    template_dir: &str,
    vars_path: &str,
    rules_path: Option<&str>,
    strict: bool,
) -> Result<()> {
    let rep = validate_report(template_dir, vars_path, rules_path, strict)?;
    if rep.ok {
        Ok(())
    } else {
        bail!("校验失败({} 项)", rep.missing.len())
    }
}

fn flatten_yaml(prefix: Option<String>, v: &YamlValue, out: &mut HashMap<String, String>) {
    match v {
        YamlValue::Mapping(map) => {
            for (k, vv) in map.iter() {
                if let YamlValue::String(s) = k {
                    let key = match &prefix {
                        Some(p) => format!("{}_{}", p, s).to_uppercase(),
                        None => s.to_uppercase(),
                    };
                    flatten_yaml(Some(key), vv, out);
                }
            }
        }
        YamlValue::Sequence(seq) => {
            let s = serde_json::to_string(seq).unwrap_or_default();
            if let Some(k) = prefix {
                out.insert(k, s);
            }
        }
        other => {
            let s = match other {
                YamlValue::Null => String::new(),
                YamlValue::Bool(b) => b.to_string(),
                YamlValue::Number(n) => n.to_string(),
                YamlValue::String(s) => s.clone(),
                _ => serde_json::to_string(other).unwrap_or_default(),
            };
            if let Some(k) = prefix {
                out.insert(k, s);
            }
        }
    }
}
