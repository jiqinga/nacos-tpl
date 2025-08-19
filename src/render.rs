use crate::cfg::EffectiveConfig;
use anyhow::{bail, Context, Result};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use serde::Serialize;
use serde_yaml::Value as YamlValue;
use std::fs::File;
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use tera::{Context as TeraContext, Tera};
use walkdir::WalkDir;
use zip::ZipArchive;

#[derive(Debug)]
pub enum Mode {
    #[allow(dead_code)]
    Stdout {
        select: Option<String>,
    },
    Directory {
        out_dir: String,
    },
}

pub fn render_all(
    template_dir: &str,
    vars_path: &str,
    _cfg: &EffectiveConfig,
    mode: Mode,
    allow_missing: bool,
    includes: &[String],
    excludes: &[String],
) -> Result<()> {
    let template_path = PathBuf::from(template_dir);
    if !template_path.exists() {
        bail!("模板路径未找到: {}", template_path.display());
    }

    let vars = load_vars_merged(vars_path).context("加载变量失败")?;

    let entries = load_template_entries(&template_path, includes, excludes)?;

    // Prepare Tera and register templates by relative path
    let mut tera = Tera::default();
    for (rel, content) in &entries {
        tera.add_raw_template(rel, content)
            .with_context(|| format!("注册模板到引擎: {}", rel))?;
    }

    let tctx = to_tera_context(&vars);

    match mode {
        Mode::Stdout { select } => {
            let name = if let Some(sel) = select {
                sel
            } else {
                bail!("--stdout 需要配合 --print <相对路径> 选择单个文件");
            };
            let rendered = render_one(&tera, &name, &tctx, &vars, allow_missing)
                .with_context(|| format!("渲染 {}", name))?;
            let mut out = std::io::stdout();
            out.write_all(rendered.as_bytes())?;
            Ok(())
        }
        Mode::Directory { out_dir } => {
            let out_dir = PathBuf::from(out_dir);
            fs::create_dir_all(&out_dir)
                .with_context(|| format!("创建输出目录: {}", out_dir.display()))?;

            for (rel, _content) in &entries {
                let rendered = render_one(&tera, rel, &tctx, &vars, allow_missing)
                    .with_context(|| format!("渲染 {}", rel))?;
                let dest = out_dir.join(rel);
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&dest, rendered.as_bytes())
                    .with_context(|| format!("写入 {}", dest.display()))?;
            }
            Ok(())
        }
    }
}

fn to_tera_context(vars: &HashMap<String, String>) -> TeraContext {
    let mut ctx = TeraContext::new();
    for (k, v) in vars {
        ctx.insert(k, v);
    }
    ctx
}

fn render_one(
    tera: &Tera,
    name: &str,
    ctx: &TeraContext,
    vars: &HashMap<String, String>,
    allow_missing: bool,
) -> Result<String> {
    let mut s = tera
        .render(name, ctx)
        .with_context(|| format!("模板渲染失败 {}", name))?;
    s = replace_compat_vars(&s, vars, allow_missing)?;
    Ok(s)
}

// 支持 ${VAR}、${ VAR }、${VAR:-default}、${ VAR : - default } 等带可选空白的写法
static RE_VAR: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\$\{\s*([A-Z0-9_]+)\s*(?::-(.*?))?\s*\}").unwrap());

fn replace_compat_vars(
    input: &str,
    vars: &HashMap<String, String>,
    allow_missing: bool,
) -> Result<String> {
    // 安全处理转义:$${VAR} -> ${VAR},直接基于 UTF-8 字符串替换,避免逐字节破坏多字节中文
    let unescaped = input.replace("$$", "$");

    let mut err_missing: Vec<String> = Vec::new();
    let replaced = RE_VAR.replace_all(&unescaped, |caps: &regex::Captures| {
        let key = caps.get(1).unwrap().as_str();
        let default = caps.get(2).map(|m| m.as_str());
        if let Some(val) = vars.get(key) {
            val.to_string()
        } else if let Some(d) = default {
            d.to_string()
        } else {
            err_missing.push(key.to_string());
            format!("${{{}}}", key)
        }
    });

    if !err_missing.is_empty() && !allow_missing {
        bail!("缺失变量: {}", err_missing.join(", "));
    }
    Ok(replaced.into_owned())
}

pub fn render_single(
    template_dir: &str,
    vars_path: &str,
    _cfg: &EffectiveConfig,
    select: &str,
    allow_missing: bool,
) -> Result<String> {
    let template_path = PathBuf::from(template_dir);
    if !template_path.exists() {
        bail!("模板路径未找到: {}", template_path.display());
    }
    let vars = load_vars_merged(vars_path).context("加载变量失败")?;
    let mut tera = Tera::default();
    let entries = load_template_entries(&template_path, &[], &[])?;
    for (rel, content) in &entries {
        tera.add_raw_template(rel, content)
            .with_context(|| format!("注册模板到引擎: {}", rel))?;
    }
    let tctx = to_tera_context(&vars);
    let name = select.replace('\\', "/");
    let rendered = render_one(&tera, &name, &tctx, &vars, allow_missing)
        .with_context(|| format!("渲染 {}", name))?;
    Ok(rendered)
}

fn load_template_entries(
    template_path: &Path,
    includes: &[String],
    excludes: &[String],
) -> Result<Vec<(String, String)>> {
    let inc = build_globset(includes).transpose()?;
    let exc = build_globset(excludes).transpose()?;
    let mut out: Vec<(String, String)> = Vec::new();
    if template_path.is_dir() {
        for entry in WalkDir::new(template_path)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_file() {
                let rel = path
                    .strip_prefix(template_path)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                if let Some(gs) = &inc {
                    if !gs.is_match(&rel) {
                        continue;
                    }
                }
                if let Some(gs) = &exc {
                    if gs.is_match(&rel) {
                        continue;
                    }
                }
                let content = fs::read_to_string(path)
                    .with_context(|| format!("读取模板: {}", path.display()))?;
                out.push((rel, content));
            }
        }
        return Ok(out);
    }
    // zip 包支持:遍历压缩条目
    if template_path.is_file()
        && template_path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.eq_ignore_ascii_case("zip"))
            .unwrap_or(false)
    {
        let f = File::open(template_path)
            .with_context(|| format!("打开 zip 失败: {}", template_path.display()))?;
        let mut zip = ZipArchive::new(f)
            .with_context(|| format!("读取 zip 失败: {}", template_path.display()))?;
        for i in 0..zip.len() {
            let mut file = zip
                .by_index(i)
                .with_context(|| format!("读取 zip 条目失败 index={}", i))?;
            if file.is_dir() {
                continue;
            }
            let name = file.name().replace('\\', "/");
            if let Some(gs) = &inc {
                if !gs.is_match(&name) {
                    continue;
                }
            }
            if let Some(gs) = &exc {
                if gs.is_match(&name) {
                    continue;
                }
            }
            let mut buf = String::new();
            use std::io::Read;
            file.read_to_string(&mut buf)
                .with_context(|| format!("读取 zip 内文件失败: {}", name))?;
            out.push((name, buf));
        }
        return Ok(out);
    }
    bail!("模板路径既不是目录也不是 zip: {}", template_path.display())
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
        // 环境变量覆盖同名键(仅覆盖已有键,避免污染)
        overlay_env(&mut out);
        return Ok(out);
    }
    // 先加载单文件(低优先级)
    merge_yaml_file(&p, &mut out)?;
    // 再加载同名 .d 目录(高优先级,按文件名顺序覆盖)
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
    let txt = std::fs::read_to_string(path)
        .with_context(|| format!("读取变量文件失败: {}", path.display()))?;
    let yaml: YamlValue = serde_yaml::from_str(&txt).context("解析变量 YAML 失败 🐛")?;
    flatten_yaml2(None, &yaml, out);
    Ok(())
}

fn flatten_yaml2(prefix: Option<String>, v: &YamlValue, out: &mut HashMap<String, String>) {
    match v {
        YamlValue::Mapping(map) => {
            for (k, vv) in map.iter() {
                if let YamlValue::String(s) = k {
                    let key = match &prefix {
                        Some(p) => format!("{}_{}", p, s).to_uppercase(),
                        None => s.to_uppercase(),
                    };
                    flatten_yaml2(Some(key), vv, out);
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

fn overlay_env(out: &mut HashMap<String, String>) {
    for (k, v) in std::env::vars() {
        let key = k.to_uppercase();
        if out.contains_key(&key) {
            out.insert(key, v);
        }
    }
}

fn build_globset(patterns: &[String]) -> Option<Result<GlobSet>> {
    if patterns.is_empty() {
        return None;
    }
    let mut b = GlobSetBuilder::new();
    for pat in patterns {
        match GlobBuilder::new(pat).literal_separator(true).build() {
            Ok(g) => {
                b.add(g);
            }
            Err(e) => {
                return Some(Err(anyhow::anyhow!("无效的 glob 模式 {}: {}", pat, e)));
            }
        }
    }
    Some(
        b.build()
            .map_err(|e| anyhow::anyhow!("glob 构建失败: {}", e)),
    )
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
struct ManifestItem {
    path: String,
    dataId: String,
    group: Option<String>,
}
#[derive(Deserialize)]
struct ManifestRoot {
    items: Vec<ManifestItem>,
}

pub fn resolve_by_id(template_dir: &str, data_id: &str, group: Option<&str>) -> Result<String> {
    let path = PathBuf::from(template_dir).join("manifest.yaml");
    if path.exists() {
        let txt = std::fs::read_to_string(&path)
            .with_context(|| format!("读取 manifest: {}", path.display()))?;
        let root: ManifestRoot = serde_yaml::from_str(&txt).context("解析 manifest.yaml 失败")?;
        let mut matches: Vec<&ManifestItem> = root
            .items
            .iter()
            .filter(|it| it.dataId == data_id)
            .collect();
        if let Some(g) = group {
            matches.retain(|it| it.group.as_deref() == Some(g));
        }
        if matches.is_empty() {
            bail!(
                "manifest 中未找到 dataId: {}{}",
                data_id,
                group.map(|g| format!(" (group={})", g)).unwrap_or_default()
            );
        }
        if matches.len() > 1 {
            bail!("存在多个匹配 dataId {}；请指定 --group", data_id);
        }
        let rel = matches[0].path.replace('\\', "/");
        return Ok(rel);
    }
    // 兜底:从模板文件 front-matter 扫描解析 dataId/group
    resolve_by_id_from_front_matter(template_dir, data_id, group)
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
struct FrontMatterMeta {
    #[serde(default)]
    dataId: Option<String>,
    #[serde(default)]
    group: Option<String>,
}

fn resolve_by_id_from_front_matter(
    template_dir: &str,
    data_id: &str,
    group: Option<&str>,
) -> Result<String> {
    let root = PathBuf::from(template_dir);
    if !root.is_dir() {
        bail!("模板目录未找到: {}", root.display());
    }
    let mut candidates: Vec<String> = Vec::new();
    for entry in WalkDir::new(&root).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if !p.is_file() {
            continue;
        }
        let content = match std::fs::read_to_string(p) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if let Some((meta, _body_start)) = parse_front_matter(&content) {
            if let Some(id) = meta.dataId.as_deref() {
                if id == data_id {
                    if let Some(g) = group {
                        if meta.group.as_deref() == Some(g) {
                            let rel = p
                                .strip_prefix(&root)
                                .unwrap()
                                .to_string_lossy()
                                .replace('\\', "/");
                            return Ok(rel);
                        }
                    } else {
                        candidates.push(
                            p.strip_prefix(&root)
                                .unwrap()
                                .to_string_lossy()
                                .replace('\\', "/"),
                        );
                    }
                }
            }
        }
    }
    if candidates.len() == 1 {
        return Ok(candidates.remove(0));
    }
    if candidates.len() > 1 {
        bail!("存在多个匹配 dataId {}；请指定 --group", data_id);
    }
    bail!(
        "front-matter 未找到 dataId: {}{}",
        data_id,
        group.map(|g| format!(" (group={})", g)).unwrap_or_default()
    )
}

fn parse_front_matter(s: &str) -> Option<(FrontMatterMeta, usize)> {
    let mut lines = s.lines();
    if lines.next()? != "---" {
        return None;
    }
    let mut yaml = String::new();
    let mut _consumed = 4; // length of first '---' + newline approx; we'll recompute
    yaml.push_str("");
    let mut idx = 3; // simplistic tracking
    for line in s.splitn(2, "\n").last().unwrap_or("").lines() {
        if line.trim() == "---" {
            break;
        }
        yaml.push_str(line);
        yaml.push('\n');
        idx += line.len() + 1;
    }
    let meta: FrontMatterMeta = match serde_yaml::from_str(&yaml) {
        Ok(m) => m,
        Err(_) => return None,
    };
    Some((meta, idx))
}

#[derive(Debug, Clone, Copy)]
enum CfgType {
    Yaml,
    Json,
    Properties,
    Text,
}

fn infer_type(p: &Path) -> CfgType {
    match p
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "yml" | "yaml" => CfgType::Yaml,
        "json" => CfgType::Json,
        "properties" => CfgType::Properties,
        _ => CfgType::Text,
    }
}

fn type_str(t: CfgType) -> &'static str {
    match t {
        CfgType::Yaml => "yaml",
        CfgType::Json => "json",
        CfgType::Properties => "properties",
        CfgType::Text => "text",
    }
}

fn derive_group_and_dataid(root: &Path, file: &Path) -> (String, String) {
    let rel = file.strip_prefix(root).unwrap();
    let mut comps = rel
        .iter()
        .map(|s| s.to_string_lossy().to_string())
        .collect::<Vec<_>>();
    if comps.len() >= 2 {
        let dataid = comps.pop().unwrap();
        let group = comps.pop().unwrap();
        (group, dataid)
    } else {
        (
            "DEFAULT_GROUP".to_string(),
            rel.file_name().unwrap().to_string_lossy().to_string(),
        )
    }
}

#[derive(Serialize)]
#[allow(non_snake_case)]
struct ManifestItemOut {
    path: String,
    dataId: String,
    group: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    r#type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tenant: Option<String>,
}

#[derive(Serialize)]
struct ManifestRootOut {
    items: Vec<ManifestItemOut>,
}

pub fn write_manifest_output(out_dir: &str, cfg: &EffectiveConfig) -> Result<()> {
    let out_dir = PathBuf::from(out_dir);
    if !out_dir.is_dir() {
        bail!("输出目录不存在：{} 🚫", out_dir.display());
    }
    let mut items: Vec<ManifestItemOut> = Vec::new();
    for entry in WalkDir::new(&out_dir).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if p.is_file() {
            let rel = p
                .strip_prefix(&out_dir)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let (group, dataid) = derive_group_and_dataid(&out_dir, p);
            let tp = infer_type(p);
            items.push(ManifestItemOut {
                path: rel,
                dataId: dataid,
                group,
                r#type: Some(type_str(tp).to_string()),
                tenant: cfg.namespace.clone(),
            });
        }
    }
    items.sort_by(|a, b| a.group.cmp(&b.group).then(a.dataId.cmp(&b.dataId)));
    let root = ManifestRootOut { items };
    let y = serde_yaml::to_string(&root)?;
    std::fs::write(out_dir.join("manifest.yaml"), y)?;
    Ok(())
}

#[allow(dead_code)]
fn load_vars(path: &str) -> Result<HashMap<String, String>> {
    let txt = fs::read_to_string(path).with_context(|| format!("读取变量失败：{} 🐛", path))?;
    let yaml: YamlValue = serde_yaml::from_str(&txt).context("解析变量 YAML 失败 🐛")?;
    let mut out = HashMap::new();
    flatten_yaml(None, &yaml, &mut out);
    Ok(out)
}

#[allow(dead_code)]
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
            // store as JSON array string
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replace_vars_with_default_and_missing() {
        let mut vars = std::collections::HashMap::new();
        vars.insert("A".to_string(), "1".to_string());
        let s = replace_compat_vars("x ${A} y ${B:-d}", &vars, false).unwrap();
        assert_eq!(s, "x 1 y d");
        let err = replace_compat_vars("${C}", &vars, false)
            .unwrap_err()
            .to_string();
        assert!(err.contains("缺失变量"));
        let ok = replace_compat_vars("${C}", &vars, true).unwrap();
        assert_eq!(ok, "${C}");
    }
}
