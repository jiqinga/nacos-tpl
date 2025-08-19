use crate::cfg::EffectiveConfig;
use anyhow::{bail, Context, Result};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use regex::Regex;
use serde::Deserialize;
use serde_json::Value as JsonValue;
use serde_yaml::Value as YamlValue;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use walkdir::WalkDir;
use zip::ZipArchive;

#[derive(Debug, Deserialize)]
struct RulesFile {
    #[allow(dead_code)]
    version: Option<u32>,
    #[allow(dead_code)]
    defaults: Option<HashMap<String, String>>, // not used in init
    #[serde(default)]
    mask_keywords: Option<Vec<String>>, // configurable masking keywords
    #[serde(default)]
    matchers: Vec<Matcher>,
}

#[derive(Debug, Deserialize)]
struct Matcher {
    when: When,
    #[serde(default)]
    replace: HashMap<String, String>, // key/path -> placeholder like ${VAR}
    #[serde(default)]
    regex_replace: Vec<RegexRule>,
}

#[derive(Debug, Deserialize)]
struct When {
    ext: String, // "yaml" | "properties"
    #[serde(default)]
    path_glob: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct RegexRule {
    pattern: String,
    to: String,
}

pub fn run_init(
    input: &str,
    output: &str,
    rules_path: Option<&str>,
    cfg: &EffectiveConfig,
) -> Result<()> {
    let (_temp_guard, in_dir) = prepare_input(input)?;
    let out_dir = PathBuf::from(output);
    fs::create_dir_all(&out_dir)
        .with_context(|| format!("鍒涘缓杈撳嚭鐩綍: {}", out_dir.display()))?;

    let rules = if let Some(p) = rules_path {
        Some(load_rules(Path::new(p))?)
    } else {
        None
    };
    let yaml_rules = filter_rules(&rules, "yaml");
    let json_rules = filter_rules(&rules, "json");
    let prop_rules = filter_rules(&rules, "properties");
    let prop_regex = filter_regex_rules(&rules, "properties");
    let json_regex = filter_regex_rules(&rules, "json");
    let mask_kw: Vec<String> = rules
        .as_ref()
        .and_then(|r| r.mask_keywords.clone())
        .or_else(|| cfg.init_defaults.mask_keywords.clone())
        .unwrap_or_else(|| {
            vec![
                "PASS".into(),
                "PASSWORD".into(),
                "SECRET".into(),
                "TOKEN".into(),
                "AK".into(),
                "SK".into(),
                "KEY".into(),
            ]
        });

    let mut example: HashMap<String, String> = HashMap::new();

    for entry in WalkDir::new(&in_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            continue;
        }
        let rel = path.strip_prefix(&in_dir).unwrap();
        let rel_unix = rel.to_string_lossy().replace('\\', "/");
        let dest = out_dir.join(rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }

        match path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase()
            .as_str()
        {
            "yml" | "yaml" => {
                let content = fs::read_to_string(path)?;
                let mut yaml: YamlValue = serde_yaml::from_str(&content)
                    .with_context(|| format!("瑙ｆ瀽 YAML 澶辫触: {}", path.display()))?;
                let changed = apply_yaml_replacements(
                    &mut yaml,
                    &yaml_rules,
                    &rel_unix,
                    &mut example,
                    &mask_kw,
                )?;
                if changed {
                    fs::write(&dest, serde_yaml::to_string(&yaml)?)?;
                } else {
                    fs::write(&dest, content)?;
                }
            }
            "json" => {
                let content = fs::read_to_string(path)?;
                let mut json: JsonValue = serde_json::from_str(&content)
                    .with_context(|| format!("瑙ｆ瀽 JSON 澶辫触: {}", path.display()))?;
                let changed = apply_json_replacements(
                    &mut json,
                    &json_rules,
                    &rel_unix,
                    &mut example,
                    &mask_kw,
                )?;
                let mut out_s = if changed {
                    serde_json::to_string_pretty(&json)?
                } else {
                    content
                };
                // apply regex replacements for json
                for (set, rules) in &json_regex {
                    if let Some(gs) = set {
                        if !gs.is_match(&rel_unix) {
                            continue;
                        }
                    }
                    for rr in rules {
                        let re = Regex::new(&rr.pattern)
                            .with_context(|| format!("闈炴硶姝ｅ垯: {}", rr.pattern))?;
                        if let Some(var) = extract_var(&rr.to) {
                            if !example.contains_key(var) {
                                if let Some(mat) = re.captures(&out_s).and_then(|c| c.get(0)) {
                                    example.insert(
                                        var.to_string(),
                                        mask_example_with(var, mat.as_str(), mask_kw.as_slice()),
                                    );
                                }
                            }
                        }
                        out_s = re.replace_all(&out_s, rr.to.as_str()).to_string();
                    }
                }
                fs::write(&dest, out_s)?;
            }
            "properties" => {
                let content = fs::read_to_string(path)?;
                let mut new_content = apply_properties_replacements(
                    &content,
                    &prop_rules,
                    &rel_unix,
                    &mut example,
                    &mask_kw,
                )?;
                // apply regex replacements (properties only in this step)
                for (set, rules) in &prop_regex {
                    if let Some(gs) = set {
                        if !gs.is_match(&rel_unix) {
                            continue;
                        }
                    }
                    for rr in rules {
                        let re = Regex::new(&rr.pattern)
                            .with_context(|| format!("闈炴硶姝ｅ垯: {}", rr.pattern))?;
                        // capture example if replacement is exactly one ${VAR}
                        if let Some(var) = extract_var(&rr.to) {
                            if !example.contains_key(var) {
                                if let Some(mat) = re.captures(&new_content).and_then(|c| c.get(0))
                                {
                                    example.insert(
                                        var.to_string(),
                                        mask_example_with(var, mat.as_str(), mask_kw.as_slice()),
                                    );
                                }
                            }
                        }
                        new_content = re.replace_all(&new_content, rr.to.as_str()).to_string();
                    }
                }
                fs::write(&dest, new_content)?;
            }
            _ => {
                // copy as-is
                fs::copy(path, &dest)?;
            }
        }
    }

    // 鎸夐渶鍙湪鏈潵澧炲姞寮€鍏崇敓鎴愮ず渚嬪彉閲忔枃浠讹紱褰撳墠鎸夐渶姹備笉鍦ㄨ緭鍑虹洰褰曠敓鎴?variables.example.yaml銆?
    // 生成示例变量文件（默认 variables.example.yaml，可由配置/环境覆盖）
    if !example.is_empty() {
        let file_name = cfg
            .init_defaults
            .example_file
            .clone()
            .unwrap_or_else(|| "variables.example.yaml".to_string());
        let path = out_dir.join(file_name);
        let yaml = serde_yaml::to_string(&example).context("序列化示例变量失败 🐛")?;
        fs::write(&path, yaml)
            .with_context(|| format!("写入示例变量失败：{} 📝", path.display()))?;
        tracing::info!("已生成示例变量文件：{} ✅", path.display());
    }
    Ok(())
}

// 鐢熸垚绀轰緥鍙橀噺鏂囦欢锛堥粯璁?variables.example.yaml锛屽彲鐢遍厤缃?鐜瑕嗙洊锛?// if !example.is_empty() {
//     let file_name = cfg
//         .init_defaults
//         .example_file
//         .clone()
//         .unwrap_or_else(|| "variables.example.yaml".to_string());
//     let path = out_dir.join(file_name);
//     let yaml = serde_yaml::to_string(&example).context("搴忓垪鍖栫ず渚嬪彉閲忓け璐?馃悰")?;
//     fs::write(&path, yaml).with_context(|| format!("鍐欏叆绀轰緥鍙橀噺澶辫触锛歿} 馃摑", path.display()))?;
//     tracing::info!("宸茬敓鎴愮ず渚嬪彉閲忔枃浠讹細{} 鉁?, path.display());
// }

fn prepare_input(input: &str) -> Result<(Option<TempDir>, PathBuf)> {
    let p = PathBuf::from(input);
    if p.is_dir() {
        return Ok((None, p));
    }
    if p.is_file()
        && p.extension()
            .and_then(|s| s.to_str())
            .map(|s| s.eq_ignore_ascii_case("zip"))
            .unwrap_or(false)
    {
        let tmp = TempDir::new().context("涓?zip 鍒涘缓涓存椂鐩綍澶辫触")?;
        let target = tmp.path().join("unzipped");
        fs::create_dir_all(&target)?;
        unzip_to(&p, &target).with_context(|| format!("瑙ｅ帇澶辫触 {}", p.display()))?;
        return Ok((Some(tmp), target));
    }
    bail!("杈撳叆蹇呴』鏄洰褰曟垨 .zip 鏂囦欢: {}", p.display())
}

fn unzip_to(zip_path: &Path, dest_dir: &Path) -> Result<()> {
    let file = fs::File::open(zip_path)
        .with_context(|| format!("鎵撳紑 zip 澶辫触: {}", zip_path.display()))?;
    let mut zip = ZipArchive::new(file).context("璇诲彇 zip 褰掓。澶辫触")?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).context("zip entry")?;
        let out_path = dest_dir.join(f.mangled_name());
        if f.name().ends_with('/') {
            fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut outfile = fs::File::create(&out_path)?;
            std::io::copy(&mut f, &mut outfile)?;
        }
    }
    Ok(())
}

fn load_rules(path: &Path) -> Result<RulesFile> {
    let txt = fs::read_to_string(path)
        .with_context(|| format!("璇诲彇瑙勫垯澶辫触: {}", path.display()))?;
    let r: RulesFile = serde_yaml::from_str(&txt).context("瑙ｆ瀽瑙勫垯 YAML 澶辫触")?;
    Ok(r)
}

fn filter_rules(
    rules: &Option<RulesFile>,
    ext: &str,
) -> Vec<(Option<GlobSet>, HashMap<String, String>)> {
    let mut out = Vec::new();
    if let Some(r) = rules {
        for m in &r.matchers {
            if m.when.ext.to_lowercase() == ext {
                let set = m
                    .when
                    .path_glob
                    .as_ref()
                    .and_then(|g| build_globset(g).ok());
                out.push((set, m.replace.clone()));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_example_file_from_rules() {
        let tmp = tempfile::TempDir::new().unwrap();
        let in_dir = tmp.path().join("in");
        let out_dir = tmp.path().join("out");
        std::fs::create_dir_all(&in_dir).unwrap();
        std::fs::create_dir_all(&out_dir).unwrap();

        // 输入 YAML（将被替换为占位符，但我们主要关心示例变量文件）
        let yaml_in = r#"
db:
  user: root
  pass: secret
"#;
        std::fs::write(in_dir.join("conf.yaml"), yaml_in).unwrap();

        // 规则：将 db.user/db.pass 抽取到示例变量
        let rules = r#"
version: 1
matchers:
  - when: { ext: "yaml" }
    replace:
      "db.user": "${DB_USER}"
      "db.pass": "${DB_PASS}"
"#;
        let rules_path = tmp.path().join("rules.yaml");
        std::fs::write(&rules_path, rules).unwrap();

        let cfg = EffectiveConfig::default();
        run_init(
            in_dir.to_str().unwrap(),
            out_dir.to_str().unwrap(),
            Some(rules_path.to_str().unwrap()),
            &cfg,
        )
        .unwrap();

        // 验证示例变量文件
        let sample_file = out_dir.join("variables.example.yaml");
        assert!(sample_file.exists(), "应生成示例变量文件");
        let txt = std::fs::read_to_string(sample_file).unwrap();
        let map: std::collections::HashMap<String, String> = serde_yaml::from_str(&txt).unwrap();
        assert_eq!(map.get("DB_USER").map(|s| s.as_str()), Some("root"));
        // 包含 PASS 关键字 -> 应被掩码
        assert_eq!(map.get("DB_PASS").map(|s| s.as_str()), Some("******"));
    }
}

fn filter_regex_rules(
    rules: &Option<RulesFile>,
    ext: &str,
) -> Vec<(Option<GlobSet>, Vec<RegexRule>)> {
    let mut out = Vec::new();
    if let Some(r) = rules {
        for m in &r.matchers {
            if m.when.ext.to_lowercase() == ext {
                let set = m
                    .when
                    .path_glob
                    .as_ref()
                    .and_then(|g| build_globset(g).ok());
                if !m.regex_replace.is_empty() {
                    out.push((set, m.regex_replace.clone()));
                }
            }
        }
    }
    out
}

fn build_globset(glob: &str) -> Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    b.add(GlobBuilder::new(glob).literal_separator(true).build()?);
    Ok(b.build()?)
}

fn apply_yaml_replacements(
    doc: &mut YamlValue,
    rules: &[(Option<GlobSet>, HashMap<String, String>)],
    rel_path: &str,
    example: &mut HashMap<String, String>,
    mask_kw: &[String],
) -> Result<bool> {
    let mut changed = false;
    for (set, repl) in rules.iter() {
        if let Some(gs) = set {
            if !gs.is_match(rel_path) {
                continue;
            }
        }
        for (path, placeholder) in repl.iter() {
            if let Some((old, was)) = yaml_replace_path(doc, path, placeholder) {
                changed = changed || was;
                if let Some(var) = extract_var(placeholder) {
                    example
                        .entry(var.to_string())
                        .or_insert(mask_example_with(var, &old, mask_kw));
                }
            }
        }
    }
    Ok(changed)
}

fn yaml_replace_path(
    doc: &mut YamlValue,
    dotted: &str,
    placeholder: &str,
) -> Option<(String, bool)> {
    let parts: Vec<&str> = dotted.split('.').collect();
    yaml_replace_path_rec(doc, &parts, 0, placeholder)
}

fn yaml_replace_path_rec(
    cur: &mut YamlValue,
    parts: &[&str],
    idx: usize,
    placeholder: &str,
) -> Option<(String, bool)> {
    if idx >= parts.len() {
        return None;
    }
    match cur {
        YamlValue::Mapping(map) => {
            let key = YamlValue::String(parts[idx].to_string());
            if idx == parts.len() - 1 {
                if let Some(v) = map.get(&key) {
                    if let Some(old) = scalar_to_string(v) {
                        // set new value at key
                        let mut new_map = map.clone();
                        new_map.insert(key, YamlValue::String(placeholder.to_string()));
                        *cur = YamlValue::Mapping(new_map);
                        return Some((old, true));
                    }
                }
                None
            } else {
                // descend
                if let Some(mut child) = map.get(&key).cloned() {
                    let res = yaml_replace_path_rec(&mut child, parts, idx + 1, placeholder);
                    if let Some((old, changed)) = res {
                        // write back child
                        let mut new_map = map.clone();
                        new_map.insert(key, child);
                        *cur = YamlValue::Mapping(new_map);
                        return Some((old, changed));
                    }
                }
                None
            }
        }
        _ => None,
    }
}

fn scalar_to_string(v: &YamlValue) -> Option<String> {
    match v {
        YamlValue::String(s) => Some(s.clone()),
        YamlValue::Number(n) => Some(n.to_string()),
        YamlValue::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn apply_properties_replacements(
    input: &str,
    rules: &[(Option<GlobSet>, HashMap<String, String>)],
    rel_path: &str,
    example: &mut HashMap<String, String>,
    mask_kw: &[String],
) -> Result<String> {
    let mut map: HashMap<String, String> = HashMap::new();
    let mut order: Vec<(String, String)> = Vec::new();
    for line in input.lines() {
        let (k, v, sep) = parse_prop_line(line);
        if let Some(k) = k {
            map.insert(k.clone(), v.unwrap_or_default());
            order.push((k, sep.unwrap_or("=").to_string()));
        } else {
            order.push((line.to_string(), String::new()));
        }
    }

    // Flatten replacements for properties (respect path_glob if present)
    let mut repl_all: HashMap<String, String> = HashMap::new();
    for (set, repl) in rules {
        if let Some(gs) = set {
            if !gs.is_match(rel_path) {
                continue;
            }
        }
        for (k, v) in repl {
            repl_all.insert(k.clone(), v.clone());
        }
    }

    for (key, ph) in repl_all.iter() {
        if let Some(old) = map.get(key) {
            if let Some(var) = extract_var(ph) {
                example
                    .entry(var.to_string())
                    .or_insert(mask_example_with(var, old, mask_kw));
            }
            map.insert(key.clone(), ph.clone());
        }
    }

    // Rebuild content
    let mut out = String::new();
    for (k, sep) in order {
        if sep.is_empty() {
            out.push_str(&k);
            out.push('\n');
        } else {
            let v = map.get(&k).cloned().unwrap_or_default();
            out.push_str(&k);
            out.push_str(&sep);
            out.push_str(&v);
            out.push('\n');
        }
    }
    Ok(out)
}

fn apply_json_replacements(
    doc: &mut JsonValue,
    rules: &[(Option<GlobSet>, HashMap<String, String>)],
    rel_path: &str,
    example: &mut HashMap<String, String>,
    mask_kw: &[String],
) -> Result<bool> {
    let mut changed = false;
    for (set, repl) in rules.iter() {
        if let Some(gs) = set {
            if !gs.is_match(rel_path) {
                continue;
            }
        }
        for (path, placeholder) in repl.iter() {
            if let Some((old, was)) = json_replace_path(doc, path, placeholder) {
                changed = changed || was;
                if let Some(var) = extract_var(placeholder) {
                    example
                        .entry(var.to_string())
                        .or_insert(mask_example_with(var, &old, mask_kw));
                }
            }
        }
    }
    Ok(changed)
}

fn json_replace_path(
    doc: &mut JsonValue,
    dotted: &str,
    placeholder: &str,
) -> Option<(String, bool)> {
    let parts: Vec<&str> = dotted.split('.').collect();
    json_replace_path_rec(doc, &parts, 0, placeholder)
}

fn json_replace_path_rec(
    cur: &mut JsonValue,
    parts: &[&str],
    idx: usize,
    placeholder: &str,
) -> Option<(String, bool)> {
    if idx >= parts.len() {
        return None;
    }
    match cur {
        JsonValue::Object(map) => {
            let key = parts[idx];
            if idx == parts.len() - 1 {
                if let Some(v) = map.get(key) {
                    if let Some(old) = scalar_to_string_json(v) {
                        map.insert(key.to_string(), JsonValue::String(placeholder.to_string()));
                        return Some((old, true));
                    }
                }
                None
            } else {
                if let Some(child) = map.get_mut(key) {
                    let res = json_replace_path_rec(child, parts, idx + 1, placeholder);
                    if res.is_some() {
                        return res;
                    }
                }
                None
            }
        }
        _ => None,
    }
}

fn scalar_to_string_json(v: &JsonValue) -> Option<String> {
    match v {
        JsonValue::String(s) => Some(s.clone()),
        JsonValue::Number(n) => Some(n.to_string()),
        JsonValue::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn mask_example_with(var: &str, value: &str, keywords: &[String]) -> String {
    let up = var.to_uppercase();
    if keywords.iter().any(|k| up.contains(k)) {
        "******".to_string()
    } else {
        value.to_string()
    }
}

fn parse_prop_line(line: &str) -> (Option<String>, Option<String>, Option<&str>) {
    // very basic parser: key[=|:]value, ignoring comments and spaces
    let s = line.trim();
    if s.is_empty() || s.starts_with('#') {
        return (None, None, None);
    }
    if let Some(pos) = s.find('=') {
        let (k, v) = s.split_at(pos);
        return (
            Some(k.trim().to_string()),
            Some(v[1..].to_string()),
            Some("="),
        );
    }
    if let Some(pos) = s.find(':') {
        let (k, v) = s.split_at(pos);
        return (
            Some(k.trim().to_string()),
            Some(v[1..].to_string()),
            Some(":"),
        );
    }
    (Some(s.to_string()), Some(String::new()), Some("="))
}

fn extract_var(placeholder: &str) -> Option<&str> {
    static RE: once_cell::sync::Lazy<Regex> =
        once_cell::sync::Lazy::new(|| Regex::new(r"^\$\{([A-Z0-9_]+)\}").unwrap());
    RE.captures(placeholder)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
}

#[allow(dead_code)]
fn yaml_quote(s: &str) -> String {
    if s.chars().any(|c| c.is_whitespace() || c == ':' || c == '#') {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        s.to_string()
    }
}
