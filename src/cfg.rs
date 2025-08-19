use anyhow::{Context, Result};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, path::PathBuf};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    pub enabled: Option<bool>,
    pub insecure: Option<bool>,
    pub ca_cert: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RenderDefaults {
    pub variables_file: Option<String>,
    pub stdout: Option<bool>,
    // not in docs but handy
    pub output_dir: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct InitDefaults {
    pub example_file: Option<String>,
    pub mask_keywords: Option<Vec<String>>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SubCommandDefaults {
    pub defaults: Option<RenderDefaults>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Config {
    pub version: Option<u32>,
    pub active_profile: Option<String>,
    pub server: Option<String>,
    pub servers: Option<Vec<String>>,
    pub namespace: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
    pub token: Option<String>,
    pub timeout_ms: Option<u64>,
    pub tls: Option<TlsConfig>,

    pub render: Option<SubCommandDefaults>,
    pub init: Option<InitSection>,

    pub profiles: Option<HashMap<String, ConfigProfile>>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct InitSection {
    pub defaults: Option<InitDefaults>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ConfigProfile {
    pub server: Option<String>,
    pub servers: Option<Vec<String>>,
    pub namespace: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
    pub token: Option<String>,
    pub timeout_ms: Option<u64>,
    pub tls: Option<TlsConfig>,
    pub render: Option<SubCommandDefaults>,
    pub init: Option<InitSection>,
}

#[derive(Debug, Default, Clone)]
pub struct EffectiveConfig {
    pub server: Option<String>,
    pub servers: Option<Vec<String>>,
    pub namespace: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
    pub token: Option<String>,
    pub timeout_ms: Option<u64>,
    pub tls: Option<TlsConfig>,

    pub render_defaults: RenderDefaults,
    pub init_defaults: InitDefaults,
}

impl EffectiveConfig {
    pub fn render_defaults(&self) -> RenderDefaults {
        self.render_defaults.clone()
    }
}

pub fn load_effective(explicit: Option<&str>, profile: Option<&str>) -> Result<EffectiveConfig> {
    let user = find_user_config();
    let project = find_project_config();
    let selected = explicit.map(PathBuf::from).or(project).or(user);

    let root: Config = if let Some(path) = selected {
        let txt = fs::read_to_string(&path)
            .with_context(|| format!("读取配置失败: {}", path.display()))?;
        serde_yaml::from_str(&txt).with_context(|| format!("解析 YAML 失败: {}", path.display()))?
    } else {
        Config::default()
    };

    // Determine profile to apply
    let prof_name = profile
        .map(|s| s.to_string())
        .or_else(|| std::env::var("NACOS_TPL_PROFILE").ok())
        .or_else(|| root.active_profile.clone());

    let mut eff = EffectiveConfig::default();
    apply_root(&mut eff, &root);

    if let Some(name) = prof_name {
        if let Some(profs) = &root.profiles {
            if let Some(p) = profs.get(&name) {
                apply_profile(&mut eff, p);
            }
        }
    }

    // Env overrides
    if let Ok(v) = std::env::var("NACOS_TPL_SERVER") {
        eff.server = Some(v);
    }
    if let Ok(v) = std::env::var("NACOS_TPL_SERVERS") {
        let list = v
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        if !list.is_empty() {
            eff.servers = Some(list);
        }
    }
    if let Ok(v) = std::env::var("NACOS_TPL_NAMESPACE") {
        eff.namespace = Some(v);
    }
    if let Ok(v) = std::env::var("NACOS_TPL_USERNAME") {
        eff.username = Some(v);
    }
    if let Ok(v) = std::env::var("NACOS_TPL_PASSWORD") {
        eff.password = Some(v);
    }
    if let Ok(v) = std::env::var("NACOS_TPL_ACCESS_KEY") {
        eff.access_key = Some(v);
    }
    if let Ok(v) = std::env::var("NACOS_TPL_SECRET_KEY") {
        eff.secret_key = Some(v);
    }
    if let Ok(v) = std::env::var("NACOS_TPL_TOKEN") {
        eff.token = Some(v);
    }
    if let Ok(v) = std::env::var("NACOS_TPL_TIMEOUT_MS") {
        if let Ok(n) = v.parse::<u64>() {
            eff.timeout_ms = Some(n);
        }
    }
    // TLS envs
    if let Ok(v) = std::env::var("NACOS_TPL_TLS_INSECURE") {
        let on = matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "on");
        let mut tls = eff.tls.clone().unwrap_or_default();
        tls.insecure = Some(on);
        eff.tls = Some(tls);
    }
    if let Ok(v) = std::env::var("NACOS_TPL_TLS_CA_CERT") {
        let mut tls = eff.tls.clone().unwrap_or_default();
        tls.ca_cert = Some(v);
        eff.tls = Some(tls);
    }

    // Env overrides (render/init defaults)
    let env_vars_file = std::env::var("NACOS_TPL_RENDER_VARIABLES_FILE").ok();
    if let Some(v) = env_vars_file {
        eff.render_defaults.variables_file = Some(v);
    }
    if let Ok(v) = std::env::var("NACOS_TPL_RENDER_STDOUT") {
        let on = matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "on");
        eff.render_defaults.stdout = Some(on);
    }
    if let Ok(v) = std::env::var("NACOS_TPL_INIT_EXAMPLE_FILE") {
        eff.init_defaults.example_file = Some(v);
    }

    Ok(eff)
}

fn find_user_config() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("NACOS_TPL_CONFIG") {
        return Some(PathBuf::from(path));
    }
    if let Some(base) = BaseDirs::new() {
        let mut p = base.home_dir().to_path_buf();
        p.push(".nacos-tpl");
        p.push("config.yaml");
        if p.exists() {
            return Some(p);
        }
    }
    // XDG optional
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        let p = PathBuf::from(xdg).join("nacos-tpl").join("config.yaml");
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn find_project_config() -> Option<PathBuf> {
    let p1 = PathBuf::from(".nacos-tpl").join("config.yaml");
    if p1.exists() {
        return Some(p1);
    }
    let p2 = PathBuf::from("nacos-tpl.yaml");
    if p2.exists() {
        return Some(p2);
    }
    None
}

fn apply_root(e: &mut EffectiveConfig, c: &Config) {
    // simple option overwrite semantics
    e.server = c.server.clone().or(e.server.take());
    e.servers = c.servers.clone().or(e.servers.take());
    e.namespace = c.namespace.clone().or(e.namespace.take());
    e.username = c.username.clone().or(e.username.take());
    e.password = c.password.clone().or(e.password.take());
    e.access_key = c.access_key.clone().or(e.access_key.take());
    e.secret_key = c.secret_key.clone().or(e.secret_key.take());
    e.token = c.token.clone().or(e.token.take());
    e.timeout_ms = c.timeout_ms.or(e.timeout_ms.take());
    e.tls = c.tls.clone().or(e.tls.take());

    if let Some(r) = &c.render {
        if let Some(d) = &r.defaults {
            merge_render_defaults(&mut e.render_defaults, d);
        }
    }
    if let Some(i) = &c.init {
        if let Some(d) = &i.defaults {
            merge_init_defaults(&mut e.init_defaults, d);
        }
    }
}

fn apply_profile(e: &mut EffectiveConfig, p: &ConfigProfile) {
    e.server = p.server.clone().or(e.server.take());
    e.servers = p.servers.clone().or(e.servers.take());
    e.namespace = p.namespace.clone().or(e.namespace.take());
    e.username = p.username.clone().or(e.username.take());
    e.password = p.password.clone().or(e.password.take());
    e.access_key = p.access_key.clone().or(e.access_key.take());
    e.secret_key = p.secret_key.clone().or(e.secret_key.take());
    e.token = p.token.clone().or(e.token.take());
    e.timeout_ms = p.timeout_ms.or(e.timeout_ms.take());
    e.tls = p.tls.clone().or(e.tls.take());

    if let Some(r) = &p.render {
        if let Some(d) = &r.defaults {
            merge_render_defaults(&mut e.render_defaults, d);
        }
    }
    if let Some(i) = &p.init {
        if let Some(d) = &i.defaults {
            merge_init_defaults(&mut e.init_defaults, d);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_override_example_file() {
        std::env::set_var("NACOS_TPL_INIT_EXAMPLE_FILE", "myvars.yaml");
        let eff = load_effective(None, None).expect("加载配置应成功");
        assert_eq!(
            eff.init_defaults.example_file.as_deref(),
            Some("myvars.yaml")
        );
        std::env::remove_var("NACOS_TPL_INIT_EXAMPLE_FILE");
    }

    #[test]
    fn env_override_server() {
        std::env::set_var("NACOS_TPL_SERVER", "http://127.0.0.1:8848");
        let eff = load_effective(None, None).expect("加载配置应成功");
        assert_eq!(eff.server.as_deref(), Some("http://127.0.0.1:8848"));
        std::env::remove_var("NACOS_TPL_SERVER");
    }
}

fn merge_render_defaults(dst: &mut RenderDefaults, src: &RenderDefaults) {
    if src.variables_file.is_some() {
        dst.variables_file = src.variables_file.clone();
    }
    if src.stdout.is_some() {
        dst.stdout = src.stdout;
    }
    if src.output_dir.is_some() {
        dst.output_dir = src.output_dir.clone();
    }
}

fn merge_init_defaults(dst: &mut InitDefaults, src: &InitDefaults) {
    if src.example_file.is_some() {
        dst.example_file = src.example_file.clone();
    }
    if src.mask_keywords.is_some() {
        dst.mask_keywords = src.mask_keywords.clone();
    }
}
