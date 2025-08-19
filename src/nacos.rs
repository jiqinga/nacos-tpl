use anyhow::{bail, Result};

// 统一的 Nacos HTTP 客户端抽象(便于测试替身注入)
pub trait NacosHttp: Send + Sync {
    fn login(&self, base: &str, user: &str, pass: &str) -> Result<String>;
    #[allow(dead_code)]
    fn get_config(
        &self,
        base: &str,
        tenant: &str,
        group: &str,
        dataid: &str,
        token_q: Option<&str>,
    ) -> Result<String>;
    fn get_config_optional(
        &self,
        base: &str,
        tenant: &str,
        group: &str,
        dataid: &str,
    ) -> Result<Option<String>>;
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    fn post_config(
        &self,
        base: &str,
        token_q: Option<&str>,
        group: &str,
        dataid: &str,
        tenant: &str,
        tp: &str,
        content: &str,
        desc: Option<&str>,
    ) -> Result<()>;
    fn list_namespaces(&self, base: &str, token_q: Option<&str>) -> Result<Vec<NamespaceInfo>>;
    fn create_namespace(
        &self,
        base: &str,
        token_q: Option<&str>,
        id: &str,
        name: &str,
        desc: Option<&str>,
    ) -> Result<()>;
}

pub struct ReqwestNacosHttp {
    client: reqwest::blocking::Client,
}

impl ReqwestNacosHttp {
    pub fn new(
        timeout_ms: u64,
        insecure: Option<bool>,
        ca_cert_path: Option<&str>,
    ) -> Result<Self> {
        let mut builder = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_millis(timeout_ms));
        if let Some(on) = insecure {
            builder = builder.danger_accept_invalid_certs(on);
        }
        if let Some(p) = ca_cert_path {
            let bytes = std::fs::read(p)?;
            // 尝试按 PEM 加载,失败则按 DER
            let cert = reqwest::Certificate::from_pem(&bytes)
                .or_else(|_| reqwest::Certificate::from_der(&bytes))?;
            builder = builder.add_root_certificate(cert);
        }
        let client = builder.build()?;
        Ok(Self { client })
    }
}

#[derive(Debug, serde::Deserialize)]
#[allow(non_snake_case)]
#[allow(dead_code)]
pub struct NamespaceInfo {
    pub namespace: String,
    pub namespaceShowName: Option<String>,
    pub namespaceDesc: Option<String>,
    pub quota: Option<i64>,
    pub configCount: Option<i64>,
    pub r#type: Option<i32>,
}

#[derive(serde::Deserialize)]
struct NamespaceListResp<T> {
    code: i32,
    message: String,
    data: T,
}

impl NacosHttp for ReqwestNacosHttp {
    fn login(&self, base: &str, user: &str, pass: &str) -> Result<String> {
        let url = format!("{}/nacos/v1/auth/login", base.trim_end_matches('/'));
        let resp = self
            .client
            .post(&url)
            .form(&[("username", user), ("password", pass)])
            .send()?;
        if !resp.status().is_success() {
            bail!("登录失败: {}", resp.status());
        }
        #[derive(serde::Deserialize)]
        #[allow(non_snake_case)]
        struct LoginResp {
            #[serde(rename = "accessToken")]
            accessToken: String,
        }
        let body: LoginResp = resp.json()?;
        tracing::debug!("登录成功:{} 用户={} ✅", base, user);
        Ok(body.accessToken)
    }

    fn get_config(
        &self,
        base: &str,
        tenant: &str,
        group: &str,
        dataid: &str,
        token_q: Option<&str>,
    ) -> Result<String> {
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
        let resp = self.client.get(&url).send()?;
        if !resp.status().is_success() {
            bail!("获取失败: {} ❌", resp.status());
        }
        tracing::debug!("GET 成功:{}/{}(tenant={}) ✅", group, dataid, tenant);
        Ok(resp.text().unwrap_or_default())
    }

    fn get_config_optional(
        &self,
        base: &str,
        tenant: &str,
        group: &str,
        dataid: &str,
    ) -> Result<Option<String>> {
        let url = format!(
            "{}/nacos/v1/cs/configs?dataId={}&group={}&tenant={}",
            base.trim_end_matches('/'),
            dataid,
            group,
            tenant
        );
        let resp = self.client.get(&url).send()?;
        if resp.status().is_success() {
            Ok(Some(resp.text().unwrap_or_default()))
        } else if resp.status().as_u16() == 404 {
            Ok(None)
        } else {
            bail!("远端获取失败: {}", resp.status())
        }
    }

    fn post_config(
        &self,
        base: &str,
        token_q: Option<&str>,
        group: &str,
        dataid: &str,
        tenant: &str,
        tp: &str,
        content: &str,
        desc: Option<&str>,
    ) -> Result<()> {
        // 使用 v2 发布接口,并与示例一致通过查询参数传递 accessToken
        let url = format!(
            "{}/nacos/v2/cs/config{}{}",
            base.trim_end_matches('/'),
            if token_q.is_some() { "?" } else { "" },
            token_q.unwrap_or("")
        );
        let mut params: Vec<(&str, &str)> = vec![
            ("dataId", dataid),
            ("group", group),
            ("namespaceId", tenant),
            ("type", tp),
            ("content", content),
        ];
        if let Some(d) = desc {
            params.push(("desc", d));
        }
        let resp = self.client.post(&url).form(&params).send()?;
        if !resp.status().is_success() {
            bail!("发布失败 {}/{}: {} ❌", group, dataid, resp.status());
        }
        tracing::debug!(
            "POST 成功:{}/{}(tenant={},type={}) ✅",
            group,
            dataid,
            tenant,
            tp
        );
        Ok(())
    }

    fn list_namespaces(&self, base: &str, token_q: Option<&str>) -> Result<Vec<NamespaceInfo>> {
        let mut url = format!(
            "{}/nacos/v2/console/namespace/list",
            base.trim_end_matches('/')
        );
        if let Some(t) = token_q {
            url.push('?');
            url.push_str(t);
        }
        let resp = self.client.get(&url).send()?;
        if !resp.status().is_success() {
            bail!("命名空间列表获取失败: {} ❌", resp.status());
        }
        let body: NamespaceListResp<Vec<NamespaceInfo>> = resp.json()?;
        if body.code != 0 {
            bail!(
                "命名空间列表返回异常:code={} message={} ❌",
                body.code,
                body.message
            );
        }
        Ok(body.data)
    }

    fn create_namespace(
        &self,
        base: &str,
        token_q: Option<&str>,
        id: &str,
        name: &str,
        desc: Option<&str>,
    ) -> Result<()> {
        let mut url = format!("{}/nacos/v2/console/namespace", base.trim_end_matches('/'));
        if let Some(t) = token_q {
            url.push('?');
            url.push_str(t);
        }
        #[derive(serde::Deserialize)]
        struct Resp {
            code: i32,
            message: String,
            data: Option<bool>,
        }
        let mut params: Vec<(&str, &str)> = vec![("namespaceId", id), ("namespaceName", name)];
        if let Some(d) = desc {
            params.push(("namespaceDesc", d));
        }
        let resp = self.client.post(&url).form(&params).send()?;
        if !resp.status().is_success() {
            bail!("创建命名空间失败: {} ❌", resp.status());
        }
        let body: Resp = resp.json()?;
        if body.code != 0 || body.data != Some(true) {
            bail!(
                "创建命名空间返回异常:code={} message={} ❌",
                body.code,
                body.message
            );
        }
        Ok(())
    }
}
