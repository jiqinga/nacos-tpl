use crate::common::selector::{collect_upload_candidates, derive_group_and_dataid};
use anyhow::{bail, Result};
use reqwest::blocking::Client;
use serde::Serialize;
use std::{fs, path::PathBuf};
// globset 在公共模块内使用
use tracing::debug;

#[inline]
fn ensure_scheme(s: &str) -> String {
    if s.starts_with("http://") || s.starts_with("https://") {
        s.to_string()
    } else {
        format!("http://{}", s)
    }
}

#[derive(Debug, Serialize)]
#[allow(non_snake_case)]
pub struct DiffItem {
    pub dataId: String,
    pub group: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GroupSummary {
    pub group: String,
    pub same: usize,
    pub changed: usize,
    pub added: usize,
}

#[derive(Debug, Serialize)]
pub struct GroupItems {
    pub group: String,
    pub changed: Vec<String>,
    pub added: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct DiffReport {
    pub same: usize,
    pub changed: usize,
    pub added: usize,
    pub errors: usize,
    pub items: Vec<DiffItem>,
    pub groups: Vec<GroupSummary>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub groups_detail: Vec<GroupItems>,
}

#[allow(clippy::too_many_arguments)]
pub fn diff_dir(
    server: &str,
    namespace: &str,
    dir: &str,
    retries: usize,
    timeout_ms: u64,
    includes: &[String],
    excludes: &[String],
    token_q: Option<&str>,
) -> Result<DiffReport> {
    let dir = PathBuf::from(dir);
    if !dir.is_dir() {
        bail!("对比目录未找到: {} 🚫", dir.display());
    }
    debug!(
        "开始对比:dir={} namespace={} servers={} ⏱️{}ms",
        dir.display(),
        namespace,
        server,
        timeout_ms
    );
    if !includes.is_empty() {
        debug!("包含规则(include):{:?} 🎯", includes);
    }
    if !excludes.is_empty() {
        debug!("排除规则(exclude):{:?} 🪄", excludes);
    }
    let client = Client::builder()
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .build()?;

    // 支持逗号分隔的多个服务器，并补全缺失的协议
    let servers: Vec<String> = server
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(ensure_scheme)
        .collect();
    let servers_ref: Vec<String> = if servers.is_empty() {
        vec![ensure_scheme(server)]
    } else {
        servers
    };

    let mut items: Vec<DiffItem> = Vec::new();
    let files = collect_upload_candidates(&dir, includes, excludes, true)?;
    for p in files {
        let rel = p
            .strip_prefix(&dir)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let (group, dataid) = derive_group_and_dataid(&dir, &p);
        debug!("扫描文件:{} -> group={} dataId={} 🔎", rel, group, dataid);
        let local = fs::read_to_string(&p).unwrap_or_default();
        debug!("本地长度:{} 字节 📄", local.len());
        match fetch_remote_optional_multi(
            &client,
            &servers_ref,
            namespace,
            &group,
            &dataid,
            retries,
            token_q,
        ) {
            Ok(Some(remote)) => {
                debug!("远端长度:{} 字节 🌐", remote.len());
                if local == remote {
                    debug!("结果:一致 same ✅");
                    items.push(DiffItem {
                        dataId: dataid,
                        group,
                        status: "same".to_string(),
                        reason: None,
                    });
                } else {
                    debug!("结果:已变更 changed 📝");
                    items.push(DiffItem {
                        dataId: dataid,
                        group,
                        status: "changed".to_string(),
                        reason: None,
                    });
                }
            }
            Ok(None) => {
                debug!("结果:远端不存在 added ➕");
                items.push(DiffItem {
                    dataId: dataid,
                    group,
                    status: "added".to_string(),
                    reason: None,
                });
            }
            Err(e) => {
                debug!("结果:请求失败 error 🐛 {}", e);
                items.push(DiffItem {
                    dataId: dataid,
                    group,
                    status: "error".to_string(),
                    reason: Some(e.to_string()),
                });
            }
        }
    }
    // sort
    items.sort_by(|a, b| a.group.cmp(&b.group).then(a.dataId.cmp(&b.dataId)));
    let mut rep = DiffReport {
        same: 0,
        changed: 0,
        added: 0,
        errors: 0,
        items,
        groups: Vec::new(),
        groups_detail: Vec::new(),
    };
    use std::collections::BTreeMap;
    let mut gmap: BTreeMap<String, GroupSummary> = BTreeMap::new();
    for it in &rep.items {
        match it.status.as_str() {
            "same" => rep.same += 1,
            "changed" => rep.changed += 1,
            "added" => rep.added += 1,
            "error" => rep.errors += 1,
            _ => {}
        }
        let entry = gmap.entry(it.group.clone()).or_insert(GroupSummary {
            group: it.group.clone(),
            same: 0,
            changed: 0,
            added: 0,
        });
        match it.status.as_str() {
            "same" => entry.same += 1,
            "changed" => entry.changed += 1,
            "added" => entry.added += 1,
            _ => {}
        }
    }
    let mut detail_map: BTreeMap<String, GroupItems> = BTreeMap::new();
    for it in &rep.items {
        let entry = detail_map.entry(it.group.clone()).or_insert(GroupItems {
            group: it.group.clone(),
            changed: Vec::new(),
            added: Vec::new(),
        });
        match it.status.as_str() {
            "changed" => entry.changed.push(it.dataId.clone()),
            "added" => entry.added.push(it.dataId.clone()),
            _ => {}
        }
    }
    rep.groups = gmap.into_values().collect();
    rep.groups_detail = detail_map.into_values().collect();
    debug!(
        "对比完成:same={} changed={} added={} 📊",
        rep.same, rep.changed, rep.added
    );
    Ok(rep)
}

// glob 构建已在公共模块中处理

pub fn fetch_remote_text(
    server: &str,
    tenant: &str,
    group: &str,
    dataid: &str,
    retries: usize,
    timeout_ms: u64,
    token_q: Option<&str>,
) -> Result<Option<String>> {
    let client = Client::builder()
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .build()?;
    let servers: Vec<String> = server
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(ensure_scheme)
        .collect();
    if servers.is_empty() {
        fetch_remote_optional(
            &client,
            &ensure_scheme(server),
            tenant,
            group,
            dataid,
            retries,
            token_q,
        )
    } else {
        fetch_remote_optional_multi(&client, &servers, tenant, group, dataid, retries, token_q)
    }
}

fn fetch_remote_optional(
    client: &Client,
    server: &str,
    tenant: &str,
    group: &str,
    dataid: &str,
    retries: usize,
    token_q: Option<&str>,
) -> Result<Option<String>> {
    retry(retries, || {
        let base = ensure_scheme(server);
        let mut url = format!(
            "{}/nacos/v1/cs/configs?dataId={}&group={}&tenant={}",
            base.trim_end_matches('/'),
            dataid,
            group,
            tenant
        );
        if let Some(t) = token_q {
            url.push('&');
            url.push_str(t);
        }
        debug!("请求远端:GET {} 🌐", url);
        let resp = client.get(&url).send()?;
        if resp.status().is_success() {
            let text = resp.text().unwrap_or_default();
            debug!("远端返回成功({} 字节)✅", text.len());
            Ok(Some(text))
        } else if resp.status().as_u16() == 404 {
            debug!("远端不存在(404)➕");
            Ok(None)
        } else {
            bail!("远端获取失败: {} ❌", resp.status())
        }
    })
}

fn fetch_remote_optional_multi(
    client: &Client,
    servers: &[String],
    tenant: &str,
    group: &str,
    dataid: &str,
    retries: usize,
    token_q: Option<&str>,
) -> Result<Option<String>> {
    let mut last_err: Option<anyhow::Error> = None;
    for s in servers {
        debug!("尝试服务器节点:{} 🔁", s);
        match fetch_remote_optional(client, s, tenant, group, dataid, retries, token_q) {
            Ok(v) => return Ok(v),
            Err(e) => {
                debug!("节点失败:{} 🐛", e);
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("所有服务器节点均请求失败(GET)")))
}

fn retry<T, F>(retries: usize, mut f: F) -> Result<T>
where
    F: FnMut() -> Result<T>,
{
    let mut attempt = 0usize;
    let max = if retries == 0 { 1 } else { retries };
    loop {
        match f() {
            Ok(v) => return Ok(v),
            Err(e) => {
                attempt += 1;
                if attempt >= max {
                    return Err(e);
                }
                let sleep_ms = 200u64.saturating_mul(2u64.pow((attempt - 1) as u32));
                std::thread::sleep(std::time::Duration::from_millis(sleep_ms));
            }
        }
    }
}
