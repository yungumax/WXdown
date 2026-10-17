//! HTTP 会话层：桌面 UA、Referer、Cookie 罐。
//!
//! 公众号文章页与合集接口（getalbum）对普通浏览器免登录开放，
//! 本层只做最基础的会话伪装；登录态（公众平台，阶段二）由
//! src-tauri 的 web_login_* 命令收割后写入。

use crate::error::{Result, WxError};
use reqwest::cookie::{CookieStore, Jar};
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT_LANGUAGE, REFERER, USER_AGENT};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;

/// 持久化的会话 Cookie（阶段一通常为空：文章与合集免登录）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DomainCookies {
    pub web: Vec<(String, String)>,
}

impl DomainCookies {
    pub fn is_empty(&self) -> bool {
        self.web.is_empty()
    }
}

/// 桌面 Chrome UA：mp.weixin.qq.com 按桌面端渲染，页面数据最全。
pub const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";
pub const SITE_REFERER: &str = "https://mp.weixin.qq.com/";

/// Cookie 要装到哪些主机。
pub const COOKIE_ORIGINS: &[&str] = &["https://mp.weixin.qq.com/"];

pub struct WxClient {
    pub http: Client,
    jar: Arc<Jar>,
}

impl WxClient {
    pub fn new() -> Result<Self> {
        Self::build(None)
    }

    /// 走指定代理（形如 `http://127.0.0.1:7890`）；传空则直连。
    pub fn with_proxy(proxy: Option<&str>) -> Result<Self> {
        Self::build(proxy.filter(|value| !value.trim().is_empty()))
    }

    fn build(proxy: Option<&str>) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(USER_AGENT, HeaderValue::from_static(UA));
        headers.insert(REFERER, HeaderValue::from_static(SITE_REFERER));
        headers.insert(
            reqwest::header::ACCEPT,
            HeaderValue::from_static(
                "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,image/apng,*/*;q=0.8",
            ),
        );
        headers.insert(
            ACCEPT_LANGUAGE,
            HeaderValue::from_static("zh-CN,zh;q=0.9,en;q=0.8"),
        );

        let jar = Arc::new(Jar::default());
        let builder = Client::builder()
            .default_headers(headers)
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(180));

        let mut builder = builder;
        if let Some(addr) = proxy {
            let proxy = reqwest::Proxy::all(addr.trim())
                .map_err(|e| WxError::InvalidInput(format!("代理地址无效: {e}")))?;
            builder = builder.proxy(proxy);
        }

        Ok(Self {
            http: builder.build()?,
            jar,
        })
    }

    /// 向会话中写入一条 Cookie，后续请求自动携带。
    pub fn add_cookie(&self, cookie: &str, url: &Url) {
        self.jar.add_cookie_str(cookie, url);
    }

    /// 导出某个域的 Cookie（k=v 形式）。
    pub fn export_cookies_for(&self, origin: &str) -> Vec<(String, String)> {
        let Ok(url) = Url::parse(origin) else { return Vec::new() };
        let Some(header) = self.jar.cookies(&url) else { return Vec::new() };
        let Ok(text) = header.to_str() else { return Vec::new() };
        let mut out = Vec::new();
        for pair in text.split(';') {
            if let Some((name, value)) = pair.trim().split_once('=') {
                let name = name.trim().to_string();
                if !name.is_empty() {
                    out.push((name, value.trim().to_string()));
                }
            }
        }
        out
    }

    /// 导出全部 Cookie（单桶：web）。
    pub fn export_cookies(&self) -> Vec<(String, String)> {
        self.export_cookies_for("https://mp.weixin.qq.com/")
    }

    /// 按域恢复 Cookie。
    pub fn install_cookies(&self, cookies: &[(String, String)]) {
        for origin in COOKIE_ORIGINS {
            self.install_cookies_for(origin, cookies);
        }
    }

    pub fn install_cookies_for(&self, origin: &str, cookies: &[(String, String)]) {
        let Ok(url) = Url::parse(origin) else { return };
        for (name, value) in cookies {
            self.jar
                .add_cookie_str(&format!("{name}={value}"), &url);
        }
    }

    /// 按请求 URL 的域取 Cookie 头。
    fn cookie_header_for(&self, url: &Url) -> Option<String> {
        let header = self.jar.cookies(url)?;
        let text = header.to_str().ok()?;
        (!text.is_empty()).then(|| text.to_string())
    }

    /// 跟随重定向拿到最终地址（用于短链解析）。
    pub async fn resolve_redirect(&self, url: &str) -> Result<String> {
        let resp = self.http.get(url).send().await?;
        Ok(resp.url().to_string())
    }

    /// 取一页文本（HTML 解析主通道）。
    pub async fn fetch_text(&self, url: &str, referer: &str) -> Result<String> {
        let mut req = self.http.get(url).header(REFERER, referer);
        if let Ok(parsed) = Url::parse(url) {
            if let Some(cookies) = self.cookie_header_for(&parsed) {
                req = req.header(reqwest::header::COOKIE, cookies);
            }
        }
        let resp = req.send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(WxError::Unavailable(format!(
                "HTTP {status}: {}",
                crate::error::truncate(&text, 200)
            )));
        }
        Ok(text)
    }

    /// 取一页 JSON（合集接口主通道）。
    pub async fn fetch_json(&self, url: &str, referer: &str) -> Result<serde_json::Value> {
        let text = self.fetch_text(url, referer).await?;
        // 有个别接口用 jsonp 包裹（callback({...})），剥掉再解析
        let trimmed = text.trim();
        let body = if let Some(start) = trimmed.find('(') {
            if trimmed.ends_with(')') && (trimmed.starts_with("callback")
                || trimmed.starts_with("jsonCallback")
                || trimmed.starts_with("wx")
                || trimmed.split('(').next().map(|h| h.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !h.is_empty() && !h.starts_with('{')).unwrap_or(false))
            {
                &trimmed[start + 1..trimmed.len() - 1]
            } else {
                trimmed
            }
        } else {
            trimmed
        };
        serde_json::from_str(body)
            .map_err(|e| WxError::Decode(format!("JSON 解析失败：{e}；正文开头：{}", crate::error::truncate(body, 160))))
    }

    /// 是否已具备登录凭据（阶段二：公众平台 Cookie 出现 slave_sid 即算）。
    pub fn has_session(&self) -> bool {
        self.export_cookies()
            .iter()
            .any(|(name, value)| name == "slave_sid" && !value.is_empty())
    }
}
