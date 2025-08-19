use anyhow::{bail, Result};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde::Deserialize;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

#[inline]
pub fn derive_group_and_dataid(root: &Path, file: &Path) -> (String, String) {
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

#[derive(Deserialize)]
#[allow(non_snake_case)]
struct MetaItem {
    dataId: String,
    #[serde(default)]
    group: Option<String>,
}

#[derive(Deserialize)]
struct MetaRoot {
    metadata: Vec<MetaItem>,
}

/// 尝试从渲染目录根的 .metadata.yml 读取允许的 (group, dataId) 清单
pub fn load_metadata_allow_set(dir: &Path) -> Option<HashSet<(String, String)>> {
    let p = dir.join(".metadata.yml");
    if !p.exists() {
        return None;
    }
    let txt = std::fs::read_to_string(&p).ok()?;
    let root: MetaRoot = match serde_yaml::from_str(&txt) {
        Ok(v) => v,
        Err(_) => return None,
    };
    let mut set = HashSet::new();
    for it in root.metadata {
        let g = it.group.unwrap_or_else(|| "DEFAULT_GROUP".to_string());
        set.insert((g, it.dataId));
    }
    Some(set)
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

/// 收集需要参与上传/对比的本地文件
/// - 忽略控制文件:.metadata.yml/.yaml、manifest.yml/.yaml
/// - 若存在 .metadata.yml 的 metadata 清单,则只保留其中列出的 (group,dataId)
/// - 支持额外 include/exclude glob 过滤(相对 dir)
pub fn collect_upload_candidates(
    dir: &Path,
    includes: &[String],
    excludes: &[String],
    use_allow_set: bool,
) -> Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        bail!("目录不存在:{} 🚫", dir.display());
    }
    let inc = build_globset(includes).transpose()?;
    let exc = build_globset(excludes).transpose()?;
    let allow = if use_allow_set {
        load_metadata_allow_set(dir)
    } else {
        None
    };

    let mut out = Vec::new();
    for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        let p = entry.path();
        if entry.file_type().is_file() {
            if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
                let name_lc = name.to_ascii_lowercase();
                if name_lc == ".metadata.yml"
                    || name_lc == ".metadata.yaml"
                    || name_lc == "manifest.yml"
                    || name_lc == "manifest.yaml"
                {
                    tracing::debug!("跳过控制文件:{} 🧩", p.display());
                    continue;
                }
            }
            // include/exclude(相对路径,使用 / 分隔)
            let rel = p
                .strip_prefix(dir)
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
            // allow-set(若存在)
            if let Some(ref set) = allow {
                let (g, d) = derive_group_and_dataid(dir, p);
                if !set.contains(&(g.clone(), d.clone())) {
                    tracing::debug!("不在 .metadata.yml 清单,已跳过:{}/{} 🚫", g, d);
                    continue;
                }
            }
            out.push(p.to_path_buf());
        }
    }
    Ok(out)
}
