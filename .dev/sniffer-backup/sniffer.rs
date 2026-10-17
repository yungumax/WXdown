//! 嗅探命令层：本地 MITM 代理的启停、证书与系统代理引导、
//! 命中清单、公众号会话收割与文章枚举（重放 profile_ext）。

use crate::state::{AppState, Settings};
use crate::types::TaskUpdate;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::Emitter;
use tauri::State;
use tauri::Manager;
use wx_core::download as dl;
use wx_sniffer::SniffEvent;

pub const SNIFF_HIT_EVENT: &str = "sniffer://hit";
pub const SNIFF_PROFILE_EVENT: &str = "sniffer://profile";

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ───────────────────────── 状态与启停 ─────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SniffHit {
    pub kind: String,
    pub url: String,
    pub host: String,
    pub ts: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileCapture {
    pub biz: String,
    pub action: String,
    pub url: String,
    /// 抓到的文章数（枚举后更新）
    pub loaded: usize,
    pub ts: u64,
}

#[tauri::command]
pub fn sniffer_status(state: State<'_, AppState>) -> SnifferStatus {
    let s = &state.sniffer;
    SnifferStatus {
        running: s.running.load(Ordering::Relaxed),
        port: *s.port.lock().unwrap(),
        cert_installed: cert_installed(),
        sys_proxy_on: s.sys_proxy_applied.load(Ordering::Relaxed),
        hits: s.hits.lock().unwrap().len(),
        profiles: s.profiles.lock().unwrap().len(),
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnifferStatus {
    pub running: bool,
    pub port: u16,
    pub cert_installed: bool,
    pub sys_proxy_on: bool,
    pub hits: usize,
    pub profiles: usize,
}

#[tauri::command]
pub fn sniffer_start(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    port: Option<u16>,
) -> Result<SnifferStatus, String> {
    if state.sniffer.running.load(Ordering::Relaxed) {
        return Ok(sniffer_status(state));
    }
    let port = port.unwrap_or(8899);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let dir = state.settings_snapshot().data_root();
    let (stop, mut ready) = wx_sniffer::run_proxy(port, dir, tx).map_err(err)?;
    // 等监听确认（最多 5 秒）
    let listen_ok = tauri::async_runtime::block_on(async {
        match tokio::time::timeout(Duration::from_secs(5), &mut ready).await {
            Ok(Ok(Ok(()))) => true,
            _ => false,
        }
    });
    if !listen_ok {
        stop.stop();
        return Err(format!("代理没能监听 127.0.0.1:{port}（端口被占用？换个端口试试）"));
    }

    *state.sniffer.port.lock().unwrap() = port;
    *state.sniffer.stop.lock().unwrap() = Some(stop);
    state.sniffer.running.store(true, Ordering::Relaxed);

    // 事件泵：命中 → 状态 + 前端
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            let state = app2.state::<AppState>();
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            match event {
                SniffEvent::Media { kind, url, host } => {
                    let hit = SniffHit { kind, url, host, ts: now };
                    {
                        let mut hits = state.sniffer.hits.lock().unwrap();
                        hits.insert(0, hit.clone());
                        hits.truncate(500);
                    }
                    let _ = app2.emit(SNIFF_HIT_EVENT, &hit);
                }
                SniffEvent::Profile { biz, action, url, cookies, ua } => {
                    // 会话凭据只记内存（含 uin/key,不能落盘）
                    {
                        let mut creds = state.sniffer.profile_creds.lock().unwrap();
                        creds.retain(|(b, _, _)| *b != biz);
                        creds.push((biz.clone(), cookies, ua));
                    }
                    let cap = ProfileCapture {
                        biz: biz.clone(),
                        action,
                        url: url.clone(),
                        loaded: 0,
                        ts: now,
                    };
                    {
                        let mut profiles = state.sniffer.profiles.lock().unwrap();
                        profiles.retain(|p: &ProfileCapture| p.biz != biz);
                        profiles.insert(0, cap.clone());
                        profiles.truncate(20);
                    }
                    let _ = app2.emit(SNIFF_PROFILE_EVENT, &cap);
                }
            }
        }
    });
    Ok(sniffer_status(state))
}

#[tauri::command]
pub fn sniffer_stop(state: State<'_, AppState>) -> Result<SnifferStatus, String> {
    if let Some(stop) = state.sniffer.stop.lock().unwrap().take() {
        stop.stop();
    }
    state.sniffer.running.store(false, Ordering::Relaxed);
    // 系统代理是跟着代理开的,停代理必须还原,否则用户断网
    if state.sniffer.sys_proxy_applied.load(Ordering::Relaxed) {
        restore_system_proxy(&state.settings_snapshot());
        state.sniffer.sys_proxy_applied.store(false, Ordering::Relaxed);
    }
    Ok(sniffer_status(state))
}

#[tauri::command]
pub fn sniffer_hits(state: State<'_, AppState>) -> Vec<SniffHit> {
    state.sniffer.hits.lock().unwrap().clone()
}

#[tauri::command]
pub fn sniffer_clear(state: State<'_, AppState>) {
    state.sniffer.hits.lock().unwrap().clear();
}

#[tauri::command]
pub fn sniffer_profiles(state: State<'_, AppState>) -> Vec<ProfileCapture> {
    state.sniffer.profiles.lock().unwrap().clone()
}

// ───────────────────────── 证书 ─────────────────────────

/// 系统根证书库里有没有我们的 CA（certutil 列表文本匹配,读库不需要管理员）。
fn cert_installed() -> bool {
    let out = std::process::Command::new("certutil")
        .args(["-store", "ROOT"])
        .output();
    match out {
        Ok(out) => String::from_utf8_lossy(&out.stdout).contains(wx_sniffer::ca::CA_NAME),
        Err(_) => false,
    }
}

/// 安装根证书：UAC 提权跑 certutil -addstore ROOT（会弹系统确认框）。
#[tauri::command]
pub fn sniffer_install_cert(state: State<'_, AppState>) -> Result<SnifferStatus, String> {
    let dir = state.settings_snapshot().data_root();
    // 确保 CA 已生成并导出
    let ca = wx_sniffer::ca::ensure_ca(&dir).map_err(err)?;
    let crt_path = dir.join("proxy-ca.crt");
    std::fs::write(&crt_path, &ca.cert_pem).map_err(err)?;

    let script = format!(
        "Start-Process certutil -ArgumentList '-addstore','-f','ROOT','{}' -Verb RunAs -Wait",
        crt_path.display()
    );
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .creation_flags(0x0800_0000)
        .output()
        .map_err(err)?;
    if !out.status.success() {
        return Err(format!(
            "证书安装没有完成（用户取消或失败）：{}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    if !cert_installed() {
        return Err("安装命令已执行，但证书库里没找到 WXdown CA（可能被取消）".into());
    }
    Ok(sniffer_status(state))
}

/// 卸载根证书（同样 UAC）。
#[tauri::command]
pub fn sniffer_uninstall_cert(state: State<'_, AppState>) -> Result<SnifferStatus, String> {
    let script = format!(
        "Start-Process certutil -ArgumentList '-delstore','ROOT','{}' -Verb RunAs -Wait",
        wx_sniffer::ca::CA_NAME
    );
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .creation_flags(0x0800_0000)
        .output()
        .map_err(err)?;
    if !out.status.success() {
        return Err("证书卸载没有完成（用户取消或失败）".into());
    }
    Ok(sniffer_status(state))
}

// ───────────────────────── 系统代理（当前用户,免管理员） ─────────────────────────

const INET_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

const INTERNET_OPTION_SETTINGS_CHANGED: u32 = 39;
const INTERNET_OPTION_REFRESH: u32 = 37;

#[link(name = "wininet")]
extern "system" {
    fn InternetSetOptionW(
        hInternet: *mut core::ffi::c_void,
        dwOption: u32,
        lpBuffer: *mut core::ffi::c_void,
        dwBufferLength: u32,
    ) -> i32;
}

fn refresh_proxy_settings() {
    unsafe {
        InternetSetOptionW(std::ptr::null_mut(), INTERNET_OPTION_SETTINGS_CHANGED, std::ptr::null_mut(), 0);
        InternetSetOptionW(std::ptr::null_mut(), INTERNET_OPTION_REFRESH, std::ptr::null_mut(), 0);
    }
}

/// 保存的旧代理设置（退出/停止时还原）。
#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
struct SavedProxy {
    enabled: u32,
    server: String,
    override_: String,
    applied: bool,
}

fn saved_proxy_path(settings: &Settings) -> PathBuf {
    settings.data_root().join("proxy-restore.json")
}

fn read_saved(settings: &Settings) -> SavedProxy {
    std::fs::read_to_string(saved_proxy_path(settings))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

#[cfg(windows)]
fn set_system_proxy(state: &State<'_, AppState>, enable: bool) -> Result<(), String> {
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
    use winreg::RegKey;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu
        .open_subkey_with_flags(INET_KEY, KEY_READ | KEY_SET_VALUE)
        .map_err(|e| format!("打开注册表失败：{e}"))?;
    let settings = state.settings_snapshot();

    if enable {
        // 先记旧的（只记一次；重复开关以第一次为准）
        let mut saved = read_saved(&settings);
        if !saved.applied {
            saved = SavedProxy {
                enabled: key.get_value("ProxyEnable").unwrap_or(0),
                server: key.get_value("ProxyServer").unwrap_or_default(),
                override_: key.get_value("ProxyOverride").unwrap_or_default(),
                applied: false,
            };
            std::fs::create_dir_all(settings.data_root()).ok();
            let mut first = saved.clone();
            first.applied = false;
            let _ = std::fs::write(
                saved_proxy_path(&settings),
                serde_json::to_string(&first).unwrap_or_default(),
            );
        }
        let port = *state.sniffer.port.lock().unwrap();
        key.set_value("ProxyEnable", &1u32).map_err(err)?;
        key.set_value("ProxyServer", &format!("127.0.0.1:{port}")).map_err(err)?;
        key.set_value(
            "ProxyOverride",
            &"localhost;127.*;192.168.*;10.*;172.16.*;172.17.*;172.18.*;172.19.*;172.20.*;172.21.*;172.22.*;172.23.*;172.24.*;172.25.*;172.26.*;172.27.*;172.28.*;172.29.*;172.30.*;172.31.*;<local>",
        )
        .map_err(err)?;
        state.sniffer.sys_proxy_applied.store(true, Ordering::Relaxed);
    } else {
        restore_system_proxy(&settings);
        state.sniffer.sys_proxy_applied.store(false, Ordering::Relaxed);
    }
    refresh_proxy_settings();
    Ok(())
}

/// 还原系统代理：有备份就写回,没备份就关掉 ProxyEnable。
pub fn restore_system_proxy(settings: &Settings) {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
        use winreg::RegKey;
        let saved = read_saved(settings);
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(key) = hkcu.open_subkey_with_flags(INET_KEY, KEY_READ | KEY_SET_VALUE) {
            if saved.applied {
                // applied=true 表示是崩溃恢复场景,记录里存的是"我们开代理前的状态"
            }
            let _ = key.set_value("ProxyEnable", &saved.enabled);
            if !saved.server.is_empty() {
                let _ = key.set_value("ProxyServer", &saved.server);
            }
            if !saved.override_.is_empty() {
                let _ = key.set_value("ProxyOverride", &saved.override_);
            }
        }
        let _ = std::fs::remove_file(saved_proxy_path(settings));
        refresh_proxy_settings();
    }
    #[cfg(not(windows))]
    {
        let _ = settings;
    }
}

#[cfg(not(windows))]
fn set_system_proxy(_state: &State<'_, AppState>, _enable: bool) -> Result<(), String> {
    Err("仅支持 Windows".into())
}

#[tauri::command]
pub fn sniffer_set_system_proxy(
    state: State<'_, AppState>,
    enable: bool,
) -> Result<SnifferStatus, String> {
    set_system_proxy(&state, enable).map_err(|e| e)?;
    Ok(sniffer_status(state))
}

/// 应用退出时调用：如果系统代理还开着,还原。
pub fn shutdown_hook(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.settings_snapshot();
    if state.sniffer.sys_proxy_applied.load(Ordering::Relaxed) {
        restore_system_proxy(&settings);
        state.sniffer.sys_proxy_applied.store(false, Ordering::Relaxed);
    }
    let stop = state.sniffer.stop.lock().unwrap().take();
    if let Some(stop) = stop {
        stop.stop();
    }
}

// ───────────────────────── 公众号文章枚举（重放 profile_ext） ─────────────────────────

/// 用收割到的凭据重放 profile_ext getmsg,枚举文章。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnumArticle {
    pub title: String,
    pub url: String,
    pub digest: String,
    pub create_time: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnumResult {
    pub items: Vec<EnumArticle>,
    pub loaded: usize,
    /// 还有更多（可再点继续）
    pub more: bool,
    pub note: String,
}

fn replay_client(ua: &str) -> Result<reqwest::Client, String> {
    let ua = if ua.is_empty() {
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36 MicroMessenger".to_string()
    } else {
        ua.to_string()
    };
    reqwest::Client::builder()
        .user_agent(ua)
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(err)
}

/// 从捕获 URL 造出指定 offset 的 getmsg URL（客户端把 uin/key 等全放查询串里）。
fn getmsg_url(captured: &str, offset: u32) -> String {
    let base = captured.split(['?', '#']).next().unwrap_or(captured).to_string();
    let query = captured.split_once('?').map(|(_, q)| q).unwrap_or("");
    // 保留 __biz / uin / key / pass_ticket / scene 等会话参数,重写翻页参数
    let mut keep: Vec<(String, String)> = Vec::new();
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if matches!(
                k,
                "__biz" | "uin" | "key" | "pass_ticket" | "scene" | "is_ok" | "f" | "lang" | "enter_id" | "x5"
            ) && !v.is_empty()
            {
                keep.push((k.to_string(), v.to_string()));
            }
        }
    }
    let mut url = format!("{base}?action=getmsg&f=json&offset={offset}&count=10&is_ok=1");
    for (k, v) in keep {
        url.push_str(&format!("&{k}={v}"));
    }
    url
}

#[tauri::command]
pub async fn sniffer_profile_enum(
    state: State<'_, AppState>,
    biz: String,
    pages: Option<u32>,
) -> Result<EnumResult, String> {
    let (cookies, ua) = {
        let creds = state.sniffer.profile_creds.lock().unwrap();
        creds
            .iter()
            .find(|(b, _, _)| *b == biz)
            .map(|(_, c, u)| (c.clone(), u.clone()))
            .ok_or_else(|| "这个公众号的会话已过期,请在微信里重新打开它的历史文章页".to_string())?
    };
    let captured = {
        let profiles = state.sniffer.profiles.lock().unwrap();
        profiles
            .iter()
            .find(|p| p.biz == biz)
            .map(|p| p.url.clone())
            .ok_or_else(|| "会话已过期".to_string())?
    };
    let client = replay_client(&ua)?;
    let cookie_header = cookies
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("; ");
    let pages = pages.unwrap_or(3).clamp(1, 10);

    let mut items: Vec<EnumArticle> = Vec::new();
    let mut more = false;
    let mut note = String::new();
    let mut offset: u32 = 0;
    for page in 0..pages {
        if page > 0 {
            tokio::time::sleep(Duration::from_millis(1500)).await;
        }
        let url = getmsg_url(&captured, offset);
        let resp = client
            .get(&url)
            .header("Cookie", &cookie_header)
            .header("Referer", "https://mp.weixin.qq.com/")
            .send()
            .await
            .map_err(err)?;
        if !resp.status().is_success() {
            note = format!("HTTP {}", resp.status());
            break;
        }
        let value: serde_json::Value = resp.json().await.map_err(err)?;
        let ret = value.pointer("/base_resp/ret").and_then(|v| v.as_i64()).unwrap_or(0);
        if ret != 0 {
            let msg = value.pointer("/base_resp/errmsg").and_then(|v| v.as_str()).unwrap_or("");
            note = format!("会话失效（ret={ret} {msg}）,请在微信里重新打开该公众号的历史文章页");
            break;
        }
        let can_continue = value.get("can_msg_continue").and_then(|v| v.as_i64()).unwrap_or(0);
        let msg_count = value.get("msg_count").and_then(|v| v.as_i64()).unwrap_or(0);
        let list_raw = value.get("general_msg_list").and_then(|v| v.as_str()).unwrap_or("[]");
        let list: serde_json::Value = serde_json::from_str(list_raw).unwrap_or_default();
        if let Some(arr) = list.as_array() {
            for entry in arr {
                // 一次发布:主文章 + 多图文(multi_app_msg_item_list)
                let mut queue: Vec<&serde_json::Value> = Vec::new();
                if let Some(main) = entry.get("app_msg_ext_info") {
                    queue.push(main);
                }
                if let Some(multi) = entry.get("multi_app_msg_item_list").and_then(|v| v.as_array()) {
                    for m in multi {
                        queue.push(m);
                    }
                }
                for info in queue {
                    let title = str_of(info, "title");
                    if title.is_empty() {
                        continue;
                    }
                    let url = str_of(info, "content_url")
                        .replace("\\/", "/")
                        .replace("&amp;", "&");
                    if !url.starts_with("http") {
                        continue;
                    }
                    if items.iter().any(|i| i.url == url) {
                        continue;
                    }
                    items.push(EnumArticle {
                        title,
                        url,
                        digest: str_of(info, "digest"),
                        create_time: info.get("datetime").and_then(|v| v.as_i64()).unwrap_or(0),
                    });
                }
            }
        }
        if msg_count <= 0 {
            more = false;
            break;
        }
        offset += msg_count as u32;
        more = can_continue == 1;
        if !more {
            break;
        }
    }
    let loaded = items.len();
    // 更新捕获记录里的条数
    {
        let mut profiles = state.sniffer.profiles.lock().unwrap();
        if let Some(p) = profiles.iter_mut().find(|p| p.biz == biz) {
            p.loaded = p.loaded.max(loaded);
        }
    }
    Ok(EnumResult { items, loaded, more, note })
}

fn str_of(v: &serde_json::Value, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or_default().to_string()
}

// ───────────────────────── 媒体直下 ─────────────────────────

/// 嗅探命中直链的轻量下载任务（不走文章管线）。
#[tauri::command]
pub async fn sniffer_download_media(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    hits: Vec<SniffHit>,
) -> Result<usize, String> {
    let client = state.client();
    let settings = state.settings_snapshot();
    let base = settings.output_dir.join("嗅探下载");
    let mut started = 0;
    for hit in hits {
        let id = state.next_task_id();
        let ext = url_ext_of(&hit.url)
            .unwrap_or_else(|| default_ext_of(&hit.kind).to_string());
        let stem = stem_of(&hit.url);
        let name = format!("{}.{ext}", stem);
        let dest = base.join(&hit.host).join(&name);
        let task = TaskUpdate {
            id: id.clone(),
            title: name.clone(),
            quality_label: hit.kind.clone(),
            status: "queued".into(),
            message: "排队中".into(),
            ..Default::default()
        };
        let _ = app.emit("task://update", &task);
        let client = client.clone();
        let app2 = app.clone();
        let retries = settings.retry_count as usize;
        tauri::async_runtime::spawn(async move {
            let snapshot = Arc::new(Mutex::new(task));
            let progress: dl::ProgressFn = {
                let snapshot = snapshot.clone();
                let app2 = app2.clone();
                Arc::new(move |p| {
                    let mut t = snapshot.lock().unwrap();
                    if matches!(t.status.as_str(), "done" | "failed" | "canceled" | "paused") {
                        return;
                    }
                    t.status = "downloading".into();
                    t.message = "下载中".into();
                    t.downloaded = p.downloaded;
                    t.total = p.total;
                    t.speed_bps = p.speed_bps;
                    t.video_pct = if p.total > 0 { p.downloaded as f64 / p.total as f64 * 100.0 } else { 0.0 };
                    let _ = app2.emit("task://update", &t.clone());
                })
            };
            let opts = dl::DownloadOptions { retries };
            let done = |status: &str, message: &str, output: &str| {
                let mut t = snapshot.lock().unwrap();
                t.status = status.into();
                t.message = message.into();
                t.speed_bps = 0.0;
                if !output.is_empty() {
                    t.output_path = output.into();
                }
                let _ = app2.emit("task://update", &t.clone());
            };
            match dl::download_resumable(&client.http, &hit.url, &dest, &opts, progress, None).await {
                Ok(dl::DownloadEnd::Completed(size)) => {
                    let mut t = snapshot.lock().unwrap();
                    t.video_pct = 100.0;
                    t.downloaded = size;
                    t.total = size;
                    let _ = app2.emit("task://update", &t.clone());
                    done("done", "已完成", &dest.to_string_lossy());
                }
                Ok(dl::DownloadEnd::Paused(_)) => done("paused", "已暂停", ""),
                Err(e) => done("failed", &format!("下载失败：{e}"), ""),
            }
        });
        started += 1;
    }
    Ok(started)
}

fn url_ext_of(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let lower = path.to_ascii_lowercase();
    for ext in ["mp4", "m4v", "mov", "flv", "m3u8", "ts", "mp3", "m4a", "aac", "amr", "wav", "jpg", "jpeg", "png", "webp", "gif", "bmp"] {
        if lower.ends_with(&format!(".{ext}")) {
            return Some(ext.to_string());
        }
    }
    None
}

fn default_ext_of(kind: &str) -> &'static str {
    match kind {
        "视频" => "mp4",
        "直播流" => "flv",
        "音频" => "mp3",
        _ => "jpg",
    }
}

/// 文件名主干：路径最后一段；没有就取 URL 哈希。
fn stem_of(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    if let Some(last) = path.rsplit('/').next() {
        let clean = last.trim_matches('.');
        if clean.len() >= 4 {
            return sanitize(clean);
        }
    }
    // URL 哈希兜底
    let mut hash: u64 = 1469598103934665603;
    for b in url.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    format!("wx{}", hash)
}

fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            other => other,
        })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches('.').to_string();
    if trimmed.is_empty() {
        "media".into()
    } else {
        trimmed.chars().take(80).collect()
    }
}
