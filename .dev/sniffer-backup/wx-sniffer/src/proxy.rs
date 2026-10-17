//! MITM 代理本体：hudsucker 内核 + 请求侧嗅探。
//!
//! 嗅探全部在 handle_request 做（拿得到完整 URL、Cookie、UA），不做
//! 请求/响应配对——文章列表的响应体由上层用收割到的凭据重放获取。

use crate::ca::build_authority;
use crate::rules::{classify_media, is_profile_ext};
use hudsucker::{
    certificate_authority::RcgenAuthority,
    hyper::header::{COOKIE, HOST, USER_AGENT},
    Body, HttpContext, HttpHandler, Proxy, RequestOrResponse,
};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// 嗅探到的事件（推给上层）。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SniffEvent {
    /// 媒体直链命中
    Media {
        kind: String,
        url: String,
        host: String,
    },
    /// 公众号历史文章会话（凭据已收割,可枚举全部文章）
    Profile {
        biz: String,
        action: String,
        url: String,
        cookies: Vec<(String, String)>,
        ua: String,
    },
}

/// 停止句柄：abort 代理任务。
#[derive(Clone)]
pub struct StopHandle {
    task: tokio::task::AbortHandle,
}

impl StopHandle {
    pub fn stop(&self) {
        self.task.abort();
    }
}

/// 请求侧嗅探 handler。
#[derive(Clone)]
pub struct SniffHandler {
    tx: mpsc::UnboundedSender<SniffEvent>,
    seen: Arc<Mutex<HashSet<String>>>,
}

impl SniffHandler {
    pub fn new(tx: mpsc::UnboundedSender<SniffEvent>) -> Self {
        Self {
            tx,
            seen: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    fn mark_seen(&self, key: &str) -> bool {
        let mut seen = self.seen.lock().unwrap();
        // 上限防膨胀
        if seen.len() > 8000 {
            seen.clear();
        }
        seen.insert(key.to_string())
    }
}

impl HttpHandler for SniffHandler {
    async fn handle_request(&mut self, _ctx: &HttpContext, mut req: hudsucker::hyper::Request<Body>) -> RequestOrResponse {
        // 完整 URL：host 头 + path（MITM 隧道里 uri 是 origin-form）
        let host = req
            .headers()
            .get(HOST)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .or_else(|| req.uri().host().map(|h| h.to_string()))
            .unwrap_or_default();
        let path_q = req
            .uri()
            .path_and_query()
            .map(|pq| pq.as_str().to_string())
            .unwrap_or_else(|| "/".into());
        let url = format!("https://{host}{path_q}");

        // 媒体直链
        if let Some(kind) = classify_media(&url) {
            // 同一地址只报一次（转码档、重复播放都会再来）
            if self.mark_seen(&url) {
                let _ = self.tx.send(SniffEvent::Media {
                    kind: kind.label().to_string(),
                    url: url.clone(),
                    host: host.clone(),
                });
            }
        }

        // 公众号历史文章会话
        if is_profile_ext(&url) {
            let cookies = req
                .headers()
                .get(COOKIE)
                .and_then(|v| v.to_str().ok())
                .map(parse_cookies)
                .unwrap_or_default();
            let ua = req
                .headers()
                .get(USER_AGENT)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_string();
            let biz = query_value(&url, "__biz").unwrap_or_default();
            let action = query_value(&url, "action").unwrap_or_default();
            let _ = self.tx.send(SniffEvent::Profile {
                biz,
                action,
                url,
                cookies,
                ua,
            });
        }

        req.into()
    }
}

fn parse_cookies(header: &str) -> Vec<(String, String)> {
    header
        .split(';')
        .filter_map(|pair| pair.trim().split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .filter(|(k, _)| !k.is_empty())
        .collect()
}

fn query_value(url: &str, key: &str) -> Option<String> {
    let q = url.split_once('?').map(|(_, rest)| rest).unwrap_or("");
    for pair in q.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k == key {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// 启动代理。返回 (停止句柄, 就绪信号)——就绪信号在端口监听成功后发 Ok(())，
/// 构建失败发 Err。**必须等它**再回 UI 成功，否则端口被占时会误报成功。
pub fn run_proxy(
    port: u16,
    data_dir: PathBuf,
    tx: mpsc::UnboundedSender<SniffEvent>,
) -> Result<(StopHandle, tokio::sync::oneshot::Receiver<std::io::Result<()>>), String> {
    let ca = crate::ca::ensure_ca(&data_dir).map_err(|e| e.to_string())?;
    let authority: RcgenAuthority = build_authority(&ca).map_err(|e| e.to_string())?;
    let handler = SniffHandler::new(tx);
    let addr: std::net::SocketAddr = ([127, 0, 0, 1], port).into();

    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let proxy = match Proxy::builder()
            .with_addr(addr)
            .with_ca(authority)
            .with_rustls_connector(hudsucker::rustls::crypto::aws_lc_rs::default_provider())
            .with_http_handler(handler)
            .build()
        {
            Ok(p) => p,
            Err(e) => {
                let _ = ready_tx.send(Err(std::io::Error::other(e.to_string())));
                return;
            }
        };
        // builder::build() 里 bind 已完成：这里就是"就绪"
        let _ = ready_tx.send(Ok(()));
        // 阻塞服务;退出与否由 StopHandle 的 abort 决定
        let _ = proxy.start().await;
    });
    Ok((StopHandle { task: task.abort_handle() }, ready_rx))
}
