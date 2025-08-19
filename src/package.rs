use crate::{cfg::EffectiveConfig, render};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use walkdir::WalkDir;
use zip::write::FileOptions;

pub fn package_zip(
    template_dir: &str,
    vars_path: &str,
    cfg: &EffectiveConfig,
    out_zip: &str,
    allow_missing: bool,
) -> Result<()> {
    let (_guard, tpl_dir) = prepare_template_input(template_dir)?;
    let tmp = TempDir::new().context("创建临时目录失败")?;
    let out_dir = tmp.path().join("render");
    fs::create_dir_all(&out_dir)?;

    render::render_all(
        &tpl_dir,
        vars_path,
        cfg,
        render::Mode::Directory {
            out_dir: out_dir.to_string_lossy().to_string(),
        },
        allow_missing,
        &[],
        &[],
    )?;

    write_manifest(&out_dir, cfg).context("写入 manifest 失败")?;
    zip_dir(&out_dir, out_zip).context("压缩渲染目录失败")
}

#[derive(Serialize)]
#[allow(non_snake_case)]
struct ManifestItem {
    path: String,
    dataId: String,
    group: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    r#type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tenant: Option<String>,
}

#[derive(Serialize)]
struct ManifestRoot {
    items: Vec<ManifestItem>,
}

fn write_manifest(out_dir: &Path, cfg: &EffectiveConfig) -> Result<()> {
    let mut items: Vec<ManifestItem> = Vec::new();
    for entry in WalkDir::new(out_dir).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if p.is_file() {
            let rel = p
                .strip_prefix(out_dir)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let (group, dataid) = derive_group_and_dataid(out_dir, p);
            let tp = infer_type(p);
            items.push(ManifestItem {
                path: rel,
                dataId: dataid,
                group,
                r#type: Some(type_str(tp).to_string()),
                tenant: cfg.namespace.clone(),
            });
        }
    }
    // Stable sort by group, dataId
    items.sort_by(|a, b| a.group.cmp(&b.group).then(a.dataId.cmp(&b.dataId)));

    let root = ManifestRoot { items };
    let y = serde_yaml::to_string(&root)?;
    fs::write(out_dir.join("manifest.yaml"), y)?;
    Ok(())
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

fn zip_dir(dir: &Path, out_zip: &str) -> Result<()> {
    // 若输出父目录不存在则自动创建
    let out_path = Path::new(out_zip);
    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("创建输出目录失败: {}", parent.display()))?;
        }
    }
    let file = fs::File::create(out_path)
        .with_context(|| format!("创建 zip 失败: {}", out_path.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let base = dir;
    for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if p.is_file() {
            let name = p
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            // 避免将 manifest.yaml 作为配置项打入 zip(Nacos 控制台会提示未识别的条目)
            if name == "manifest.yaml" {
                continue;
            }
            zip.start_file(name, opts)?;
            let data = fs::read(p)?;
            zip.write_all(&data)?;
        }
    }
    zip.finish()?;
    Ok(())
}

// 允许 -t 传目录或 Nacos 导出的 zip:若为 zip 则解压到临时目录后使用
fn prepare_template_input(input: &str) -> Result<(Option<TempDir>, String)> {
    let p = PathBuf::from(input);
    if p.is_dir() {
        return Ok((None, p.to_string_lossy().to_string()));
    }
    if p.is_file()
        && p.extension()
            .and_then(|s| s.to_str())
            .map(|s| s.eq_ignore_ascii_case("zip"))
            .unwrap_or(false)
    {
        let tmp = TempDir::new().context("为 zip 创建临时目录失败")?;
        let target = tmp.path().join("unzipped");
        fs::create_dir_all(&target)?;
        unzip_to(&p, &target).with_context(|| format!("解压失败 {}", p.display()))?;
        return Ok((Some(tmp), target.to_string_lossy().to_string()));
    }
    bail!("模板必须是目录或 .zip 文件: {}", p.display())
}

fn unzip_to(zip_path: &Path, dest_dir: &Path) -> Result<()> {
    let file = fs::File::open(zip_path)
        .with_context(|| format!("打开 zip 失败: {}", zip_path.display()))?;
    let mut zip = zip::ZipArchive::new(file).context("读取 zip 归档失败")?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).context("zip 条目读取失败")?;
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
