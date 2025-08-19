use crate::cfg::EffectiveConfig;
use crate::common::selector::{collect_upload_candidates, derive_group_and_dataid};
use crate::nacos::{NacosHttp, ReqwestNacosHttp};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[inline]
fn ensure_scheme(s: &str) -> String {
    if s.starts_with("http://") || s.starts_with("https://") {
        s.to_string()
    } else {
        format!("http://{}", s)
    }
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
        .to_ascii_lowercase()
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

#[derive(Debug, Serialize)]
#[allow(non_snake_case)]
pub struct ItemReport {
    pub dataId: String,
    pub group: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub localMd5: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sizeBytes: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct GroupSummary {
    pub group: String,
    pub updated: usize,
    pub skipped: usize,
    pub failed: usize,
}

#[derive(Debug, Serialize)]
pub struct ApplyReport {
    pub updated: usize,
    pub skipped: usize,
    pub failed: usize,
    pub items: Vec<ItemReport>,
    pub groups: Vec<GroupSummary>,
}

fn build_servers(cfg: &EffectiveConfig, override_list: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if let Some(s) = override_list {
        for v in s.split(',').map(|v| v.trim()).filter(|v| !v.is_empty()) {
            if !out.contains(&v.to_string()) {
                out.push(ensure_scheme(v));
            }
        }
    }
    if let Some(list) = &cfg.servers {
        for s in list {
            if !out.contains(s) {
                out.push(ensure_scheme(s));
            }
        }
    }
    if let Some(s) = &cfg.server {
        if !out.contains(s) {
            out.push(ensure_scheme(s));
        }
    }
    out
}

fn try_login_token(
    http: &dyn NacosHttp,
    servers: &[String],
    user: &str,
    pass: &str,
) -> Option<String> {
    for b in servers {
        tracing::debug!("尝试登录节点: {} 🔑", b);
        match http.login(b, user, pass) {
            Ok(tok) => return Some(tok),
            Err(e) => {
                tracing::warn!("登录失败: {} ❌", e);
            }
        }
    }
    None
}

fn ensure_namespace_exists(
    http: &dyn NacosHttp,
    servers: &[String],
    token_q: Option<&str>,
    namespace: &str,
    create_if_missing: bool,
) -> Result<()> {
    // 兼容 public 与空串
    let mut candidates = std::collections::HashSet::new();
    candidates.insert(namespace.to_string());
    if namespace.eq_ignore_ascii_case("public") {
        candidates.insert(String::new());
    }
    if namespace.is_empty() {
        candidates.insert("public".to_string());
    }

    // 查询命名空间列表
    let mut last_err: Option<anyhow::Error> = None;
    for s in servers {
        match http.list_namespaces(s, token_q) {
            Ok(list) => {
                for ns in list {
                    let id = ns.namespace;
                    let show = ns.namespaceShowName.unwrap_or_default();
                    if candidates.contains(&id)
                        || id.eq_ignore_ascii_case(namespace)
                        || show.eq_ignore_ascii_case(namespace)
                    {
                        tracing::debug!(
                            "命名空间已存在: {} (使用ID={}, 显示名={}) 🎯",
                            namespace,
                            id,
                            show
                        );
                        return Ok(());
                    }
                }
            }
            Err(e) => {
                last_err = Some(e);
                continue;
            }
        }
    }
    if let Some(e) = last_err {
        tracing::warn!("命名空间列表查询失败，最后错误: {} ⚠️", e);
    }

    if !create_if_missing {
        bail!(
            "命名空间不存在或不可用: {} 🚫，可使用 --create-namespace 自动创建",
            namespace
        );
    }
    // 不允许创建内置 public
    if namespace.eq_ignore_ascii_case("public") || namespace.is_empty() {
        bail!("内置命名空间 public 不需要创建，但未在列表中找到，请检查服务状态 ⚠️");
    }

    // 尝试创建
    let mut last_ce: Option<anyhow::Error> = None;
    for s in servers {
        match http.create_namespace(s, token_q, namespace, namespace, None) {
            Ok(_) => {
                tracing::debug!("已自动创建命名空间并使用 ID={} ✅", namespace);
                return Ok(());
            }
            Err(e) => last_ce = Some(e),
        }
    }
    Err(last_ce.unwrap_or_else(|| anyhow::anyhow!("创建命名空间失败(未知原因) ⚠️")))
}

fn remote_get_optional(
    http: &dyn NacosHttp,
    servers: &[String],
    tenant: &str,
    group: &str,
    dataid: &str,
) -> Result<Option<String>> {
    let mut last_err: Option<anyhow::Error> = None;
    for s in servers {
        match http.get_config_optional(s, tenant, group, dataid) {
            Ok(opt) => return Ok(opt),
            Err(e) => last_err = Some(e),
        }
    }
    if let Some(e) = last_err {
        tracing::debug!("远端获取失败(最终错误): {}/{} -> {} 🐛", group, dataid, e);
    }
    Ok(None)
}

#[allow(clippy::too_many_arguments)]
fn post_config_multi(
    http: &dyn NacosHttp,
    servers: &[String],
    token_q: Option<&str>,
    group: &str,
    dataid: &str,
    tenant: &str,
    tp: &str,
    content: &str,
    retries: usize,
) -> Result<()> {
    let attempts = if retries == 0 { 1 } else { retries };
    let mut last_err: Option<anyhow::Error> = None;
    for s in servers {
        for i in 0..attempts {
            match http.post_config(s, token_q, group, dataid, tenant, tp, content, None) {
                Ok(_) => return Ok(()),
                Err(e) => {
                    last_err = Some(e);
                    if i + 1 < attempts {
                        let sleep_ms = 200u64.saturating_mul(2u64.pow(i as u32));
                        std::thread::sleep(std::time::Duration::from_millis(sleep_ms));
                    }
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("所有服务器节点均请求失败(POST) ❌")))
}

#[allow(clippy::too_many_arguments)]
#[allow(unreachable_code)]
pub fn apply_dir_opts(
    dir: &str,
    cfg: &EffectiveConfig,
    server_override: Option<&str>,
    ns_override: Option<&str>,
    skip_unchanged: bool,
    concurrency: usize,
    retries: usize,
    timeout_ms: u64,
    dry_run: bool,
    max_bytes: Option<usize>,
    normalize_lf: bool,
    include_md5: bool,
    overwrite: bool,
    create_namespace: bool,
) -> Result<ApplyReport> {
    let dir = PathBuf::from(dir);
    if !dir.is_dir() {
        bail!("发布目录未找到: {}", dir.display());
    }

    let servers = build_servers(cfg, server_override);
    if servers.is_empty() {
        bail!("未指定服务地址(请使用 --server 或配置文件) ⚠️");
    }

    let namespace = ns_override
        .map(|s| s.to_string())
        .or_else(|| cfg.namespace.clone())
        .unwrap_or_else(|| "public".to_string());

    let (insecure, ca_cert) = cfg
        .tls
        .as_ref()
        .map(|t| (t.insecure, t.ca_cert.clone()))
        .unwrap_or((None, None));
    let http = ReqwestNacosHttp::new(timeout_ms, insecure, ca_cert.as_deref())
        .context("初始化 HTTP 客户端失败 🐛")?;

    // token 优先使用 cfg.token；否则尝试登录
    let token = cfg
        .token
        .clone()
        .or_else(|| match (&cfg.username, &cfg.password) {
            (Some(u), Some(p)) => try_login_token(&http, &servers, u, p),
            _ => None,
        });
    let token_q = token.as_ref().map(|t| format!("accessToken={}", t));

    // 发布前确保命名空间存在(可选择创建)
    ensure_namespace_exists(
        &http,
        &servers,
        token_q.as_deref(),
        &namespace,
        create_namespace,
    )?;

    // 收集需要发布的文件
    let files = collect_upload_candidates(&dir, &[], &[], true)?;
    tracing::debug!("待处理文件数: {} 📄", files.len());

    // 使用并发发布，尊重重试/限流等参数
    let items = apply_items_parallel(
        &dir,
        &files,
        &http,
        &servers,
        token_q.as_deref(),
        &namespace,
        skip_unchanged,
        retries,
        dry_run,
        max_bytes,
        normalize_lf,
        include_md5,
        overwrite,
        concurrency,
    )?;

    // 汇总
    let mut updated = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    let mut gmap: std::collections::BTreeMap<String, GroupSummary> =
        std::collections::BTreeMap::new();
    for it in &items {
        match it.status.as_str() {
            "updated" => {
                updated += 1;
                let e = gmap.entry(it.group.clone()).or_insert(GroupSummary {
                    group: it.group.clone(),
                    updated: 0,
                    skipped: 0,
                    failed: 0,
                });
                e.updated += 1;
            }
            "skipped" => {
                skipped += 1;
                let e = gmap.entry(it.group.clone()).or_insert(GroupSummary {
                    group: it.group.clone(),
                    updated: 0,
                    skipped: 0,
                    failed: 0,
                });
                e.skipped += 1;
            }
            "failed" => {
                failed += 1;
                let e = gmap.entry(it.group.clone()).or_insert(GroupSummary {
                    group: it.group.clone(),
                    updated: 0,
                    skipped: 0,
                    failed: 0,
                });
                e.failed += 1;
            }
            _ => {}
        }
    }
    let groups = gmap.into_values().collect();
    return Ok(ApplyReport {
        updated,
        skipped,
        failed,
        items,
        groups,
    });

    let mut items: Vec<ItemReport> = Vec::new();

    for path in files {
        let (group, dataid) = derive_group_and_dataid(&dir, &path);
        let tp = infer_type(&path);

        // 读取本地内容
        let mut content =
            fs::read_to_string(&path).with_context(|| format!("读取失败: {}", path.display()))?;
        if let Some(limit) = max_bytes {
            if content.len() > limit {
                items.push(ItemReport {
                    dataId: dataid.clone(),
                    group: group.clone(),
                    status: "failed".to_string(),
                    reason: Some(format!("内容超过大小上限: {} 字节 🚫", content.len())),
                    localMd5: None,
                    sizeBytes: Some(content.len()),
                });
                continue;
            }
        }
        if normalize_lf {
            content = content.replace("\r\n", "\n").replace('\r', "\n");
        }

        let local_md5 = if include_md5 {
            Some(format!("{:x}", md5::compute(content.as_bytes())))
        } else {
            None
        };

        // 远端存在性与是否跳过
        let mut existed = false;
        let mut same = false;
        if skip_unchanged || !overwrite {
            match remote_get_optional(&http, &servers, &namespace, &group, &dataid) {
                Ok(opt) => {
                    if let Some(remote) = opt {
                        existed = true;
                        if skip_unchanged && remote == content {
                            same = true;
                        }
                    }
                }
                Err(e) => {
                    tracing::debug!(
                        "远端获取失败，跳过存在性判断: {}/{} -> {} 🐛",
                        group,
                        dataid,
                        e
                    );
                }
            }
        }

        if same {
            items.push(ItemReport {
                dataId: dataid.clone(),
                group: group.clone(),
                status: "skipped".to_string(),
                reason: None,
                localMd5: local_md5.clone(),
                sizeBytes: Some(content.len()),
            });
            continue;
        }

        if existed && !overwrite {
            items.push(ItemReport {
                dataId: dataid.clone(),
                group: group.clone(),
                status: "failed".to_string(),
                reason: Some("远端已存在，请使用 --overwrite 明确覆盖 ⚠️".to_string()),
                localMd5: local_md5.clone(),
                sizeBytes: Some(content.len()),
            });
            continue;
        }

        if dry_run {
            items.push(ItemReport {
                dataId: dataid.clone(),
                group: group.clone(),
                status: "updated".to_string(),
                reason: Some("空跑".to_string()),
                localMd5: local_md5.clone(),
                sizeBytes: Some(content.len()),
            });
            continue;
        }

        // 发布
        let res = post_config_multi(
            &http,
            &servers,
            token_q.as_deref(),
            &group,
            &dataid,
            &namespace,
            type_str(tp),
            &content,
            retries,
        );
        match res {
            Ok(_) => items.push(ItemReport {
                dataId: dataid.clone(),
                group: group.clone(),
                status: "updated".to_string(),
                reason: None,
                localMd5: local_md5.clone(),
                sizeBytes: Some(content.len()),
            }),
            Err(e) => items.push(ItemReport {
                dataId: dataid.clone(),
                group: group.clone(),
                status: "failed".to_string(),
                reason: Some(e.to_string()),
                localMd5: local_md5.clone(),
                sizeBytes: Some(content.len()),
            }),
        }
    }

    // 汇总
    let mut updated = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    let mut gmap: std::collections::BTreeMap<String, GroupSummary> =
        std::collections::BTreeMap::new();
    for it in &items {
        match it.status.as_str() {
            "updated" => {
                updated += 1;
                let e = gmap.entry(it.group.clone()).or_insert(GroupSummary {
                    group: it.group.clone(),
                    updated: 0,
                    skipped: 0,
                    failed: 0,
                });
                e.updated += 1;
            }
            "skipped" => {
                skipped += 1;
                let e = gmap.entry(it.group.clone()).or_insert(GroupSummary {
                    group: it.group.clone(),
                    updated: 0,
                    skipped: 0,
                    failed: 0,
                });
                e.skipped += 1;
            }
            "failed" => {
                failed += 1;
                let e = gmap.entry(it.group.clone()).or_insert(GroupSummary {
                    group: it.group.clone(),
                    updated: 0,
                    skipped: 0,
                    failed: 0,
                });
                e.failed += 1;
            }
            _ => {}
        }
    }

    let groups = gmap.into_values().collect();
    Ok(ApplyReport {
        updated,
        skipped,
        failed,
        items,
        groups,
    })
}

// 并发处理文件集合以生成发布条目
#[allow(clippy::too_many_arguments)]
fn apply_items_parallel(
    dir: &Path,
    files: &[PathBuf],
    http: &dyn NacosHttp,
    servers: &[String],
    token_q: Option<&str>,
    namespace: &str,
    skip_unchanged: bool,
    retries: usize,
    dry_run: bool,
    max_bytes: Option<usize>,
    normalize_lf: bool,
    include_md5: bool,
    overwrite: bool,
    concurrency: usize,
) -> Result<Vec<ItemReport>> {
    use rayon::prelude::*;
    let namespace = namespace.to_string();

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(concurrency.max(1))
        .build()
        .context("并发线程池创建失败 🐛")?;

    let items = pool.install(|| {
        files
            .par_iter()
            .map(|path| {
                let (group, dataid) = derive_group_and_dataid(dir, path);
                let tp = infer_type(path);

                // 读取本地内容
                let mut content = match fs::read_to_string(path) {
                    Ok(s) => s,
                    Err(e) => {
                        return ItemReport {
                            dataId: dataid,
                            group,
                            status: "failed".to_string(),
                            reason: Some(format!("读取失败：{} 🐛", e)),
                            localMd5: None,
                            sizeBytes: None,
                        };
                    }
                };
                if let Some(limit) = max_bytes {
                    if content.len() > limit {
                        return ItemReport {
                            dataId: dataid,
                            group,
                            status: "failed".to_string(),
                            reason: Some(format!("内容超过大小上限：{} 字节 🚫", content.len())),
                            localMd5: None,
                            sizeBytes: Some(content.len()),
                        };
                    }
                }
                if normalize_lf {
                    content = content.replace("\r\n", "\n").replace('\r', "\n");
                }

                let local_md5 = if include_md5 {
                    Some(format!("{:x}", md5::compute(content.as_bytes())))
                } else {
                    None
                };

                // 远端存在性与是否跳过
                let mut existed = false;
                let mut same = false;
                if skip_unchanged || !overwrite {
                    match remote_get_optional(http, servers, &namespace, &group, &dataid) {
                        Ok(opt) => {
                            if let Some(remote) = opt {
                                existed = true;
                                if skip_unchanged && remote == content {
                                    same = true;
                                }
                            }
                        }
                        Err(e) => {
                            tracing::debug!(
                                "远端获取失败，跳过存在性判断：{}/{} -> {} 🐛",
                                group,
                                dataid,
                                e
                            );
                        }
                    }
                }

                if same {
                    return ItemReport {
                        dataId: dataid,
                        group,
                        status: "skipped".to_string(),
                        reason: None,
                        localMd5: local_md5,
                        sizeBytes: Some(content.len()),
                    };
                }

                if existed && !overwrite {
                    return ItemReport {
                        dataId: dataid,
                        group,
                        status: "failed".to_string(),
                        reason: Some("远端已存在，请使用 --overwrite 明确覆盖 ⚠️".to_string()),
                        localMd5: local_md5,
                        sizeBytes: Some(content.len()),
                    };
                }

                if dry_run {
                    return ItemReport {
                        dataId: dataid,
                        group,
                        status: "updated".to_string(),
                        reason: Some("空跑".to_string()),
                        localMd5: local_md5,
                        sizeBytes: Some(content.len()),
                    };
                }

                // 发布
                let res = post_config_multi(
                    http,
                    servers,
                    token_q,
                    &group,
                    &dataid,
                    &namespace,
                    type_str(tp),
                    &content,
                    retries,
                );
                match res {
                    Ok(_) => ItemReport {
                        dataId: dataid,
                        group,
                        status: "updated".to_string(),
                        reason: None,
                        localMd5: local_md5,
                        sizeBytes: Some(content.len()),
                    },
                    Err(e) => ItemReport {
                        dataId: dataid,
                        group,
                        status: "failed".to_string(),
                        reason: Some(e.to_string()),
                        localMd5: local_md5,
                        sizeBytes: Some(content.len()),
                    },
                }
            })
            .collect()
    });

    Ok(items)
}

#[cfg(test)]
#[allow(clippy::type_complexity)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct FakeHttp {
        posts: Arc<Mutex<Vec<(String, String, String, String, String)>>>,
        return_existing: bool,
    }

    impl FakeHttp {
        fn new(return_existing: bool) -> Self {
            Self {
                posts: Arc::new(Mutex::new(Vec::new())),
                return_existing,
            }
        }
    }

    impl NacosHttp for FakeHttp {
        fn login(&self, _base: &str, _user: &str, _pass: &str) -> Result<String> {
            Ok("tok".into())
        }
        fn get_config(
            &self,
            _base: &str,
            _tenant: &str,
            _group: &str,
            _dataid: &str,
            _token_q: Option<&str>,
        ) -> Result<String> {
            Ok(String::new())
        }
        fn get_config_optional(
            &self,
            _base: &str,
            _tenant: &str,
            _group: &str,
            _dataid: &str,
        ) -> Result<Option<String>> {
            if self.return_existing {
                Ok(Some("same".into()))
            } else {
                Ok(None)
            }
        }
        fn post_config(
            &self,
            base: &str,
            _token_q: Option<&str>,
            group: &str,
            dataid: &str,
            tenant: &str,
            tp: &str,
            content: &str,
            _desc: Option<&str>,
        ) -> Result<()> {
            self.posts.lock().unwrap().push((
                base.to_string(),
                group.to_string(),
                dataid.to_string(),
                tenant.to_string(),
                tp.to_string(),
            ));
            // 为了测试 skip_unchanged，我们用 "same" 作为远端内容
            let _ = content;
            Ok(())
        }
        fn list_namespaces(
            &self,
            _base: &str,
            _token_q: Option<&str>,
        ) -> Result<Vec<crate::nacos::NamespaceInfo>> {
            Ok(vec![])
        }
        fn create_namespace(
            &self,
            _base: &str,
            _token_q: Option<&str>,
            _id: &str,
            _name: &str,
            _desc: Option<&str>,
        ) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn parallel_updates_and_skips() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join("G1")).unwrap();
        std::fs::create_dir_all(root.join("G2")).unwrap();
        std::fs::write(root.join("G1/a.yaml"), "same").unwrap();
        std::fs::write(root.join("G2/b.properties"), "v=1").unwrap();

        let files = vec![root.join("G1/a.yaml"), root.join("G2/b.properties")];
        let fake = FakeHttp::new(true); // 使第一个文件命中 same -> 跳过
        let servers = vec!["http://s1".to_string()];
        let out = apply_items_parallel(
            root, &files, &fake, &servers, None, "public", true,  // skip_unchanged
            1,     // retries
            false, // dry_run
            None,  // max_bytes
            false, // normalize_lf
            false, // include_md5
            true,  // overwrite
            2,     // concurrency
        )
        .unwrap();

        assert_eq!(out.len(), 2);
        let skipped = out.iter().filter(|i| i.status == "skipped").count();
        let updated = out.iter().filter(|i| i.status == "updated").count();
        assert_eq!(skipped, 1);
        assert_eq!(updated, 1);
    }
}
