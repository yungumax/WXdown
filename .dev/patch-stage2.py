# -*- coding: utf-8 -*-
"""阶段二补丁:公众平台登录 + 公众号全量枚举"""
import io
import sys

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")

# ══════════ state.rs: BatchCache.total ══════════
p = "src-tauri/src/state.rs"
s = open(p, encoding="utf-8").read()
s = s.replace(
    """    /// 来源类型：album（合集/文章展开）/ account（公众号全量，album_id 为空按偏移翻页）
    pub kind: String,""",
    """    /// 来源类型：album（合集/文章展开）/ account（公众号全量，album_id 为空按偏移翻页）
    pub kind: String,
    /// 来源总数（合集 article_count / 公众号 app_msg_cnt；用于判断是否翻完）
    pub total: usize,""",
)
open(p, "w", encoding="utf-8").write(s)

# ══════════ commands.rs ══════════
p = "src-tauri/src/commands.rs"
s = open(p, encoding="utf-8").read()

# 1) 三处 BatchCache 构造补 kind/total
s = s.replace(
    """        BatchCache {
            uid: biz.to_string(),
            album_id: album_id.to_string(),
            kind: "album".into(),
            items: items.clone(),""",
    """        BatchCache {
            uid: biz.to_string(),
            album_id: album_id.to_string(),
            kind: "album".into(),
            total,
            items: items.clone(),""",
)
s = s.replace(
    """                            BatchCache {
                                uid: probe.uid.clone(),
                                album_id: probe.bid.clone(),
                                kind: "album".into(),
                                items: probe.items.clone(),""",
    """                            BatchCache {
                                uid: probe.uid.clone(),
                                album_id: probe.bid.clone(),
                                kind: "album".into(),
                                total: probe.total,
                                items: probe.items.clone(),""",
)

# 2) probe_source: AccountQuery 分支 + 平台辅助函数
s = s.replace(
    """        SourceTarget::Album { biz, album_id, url } => {
            album_probe(&state, &client, &biz, &album_id, &url).await
        }
    }
}""",
    """        SourceTarget::Album { biz, album_id, url } => {
            album_probe(&state, &client, &biz, &album_id, &url).await
        }
        SourceTarget::AccountQuery { name } => account_probe(&state, &name).await,
    }
}

// ───────────────────────── 公众平台(阶段二) ─────────────────────────

/// 平台 appmsg 条目 → 表格条目。
fn appmsg_item_to_entry(item: &wx_core::platform::AppmsgItem, account: &str) -> PostEntry {
    PostEntry {
        bid: item.aid.clone(),
        mid: String::new(),
        title: naming::title_of(&item.title),
        author: account.to_string(),
        created_at: item.create_time,
        pics: 0,
        has_video: false,
        is_audio: false,
        media_url: String::new(),
        page_url: item.link.clone(),
        duration: 0,
        is_retweet: false,
        media_unknown: true,
        digest: item.digest.clone(),
    }
}

/// 取平台会话（client + token）；未登录给人话错误。
fn mp_session(state: &State<'_, AppState>) -> Result<(std::sync::Arc<WxClient>, String), String> {
    let token = state.mp_token.lock().unwrap().clone();
    let logged_in = state
        .login
        .read()
        .unwrap()
        .as_ref()
        .map(|l| l.logged_in)
        .unwrap_or(false);
    if token.is_empty() || !logged_in {
        return Err("这个功能需要先登录微信公众平台：点右上角「登录」，用你自己的公众号扫码".into());
    }
    Ok((state.client(), token))
}

/// 频控退避重试（平台接口限频 ret 200013 时等一会再试）。
async fn with_freq_retry<T, F, Fut>(mut f: F, tries: usize) -> Result<T, String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = std::result::Result<T, wx_core::platform::PlatformError>>,
{
    let mut delay = Duration::from_millis(1500);
    let mut last = String::from("接口限频");
    for attempt in 0..tries {
        match f().await {
            Ok(v) => return Ok(v),
            Err(wx_core::platform::PlatformError::FreqControl(m)) => {
                last = format!("接口限频（{m}）");
                wx_core::login::debug_log(&format!(
                    "平台接口限频，第 {} 次退避 {:.1}s 后重试",
                    attempt + 1,
                    delay.as_secs_f64()
                ));
                tokio::time::sleep(delay).await;
                delay *= 2;
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    Err(format!("{last}，重试次数用完；请稍等一分钟再点解析"))
}

/// 按名称查公众号 → 枚举第一页文章 → kind=account 的 Probe。
async fn account_probe(state: &State<'_, AppState>, name: &str) -> Result<Probe, String> {
    let (client, token) = mp_session(state)?;
    let hits = with_freq_retry(|| client.search_biz(&token, name), 2).await?;
    let Some(hit) = hits.first() else {
        return Err(format!("没有找到公众号「{name}」：换个更准确的名字试试"));
    };
    if hits.len() > 1 {
        wx_core::login::debug_log(&format!(
            "搜索「{name}」命中 {} 个号，取第一个：{}（fakeid={}）",
            hits.len(),
            hit.nickname,
            hit.fakeid
        ));
    }
    let fakeid = hit.fakeid.clone();
    let page = with_freq_retry(|| client.appmsg_page(&token, &fakeid, 0, 5), 3).await?;
    let account = hit.nickname.clone();
    let items: Vec<PostEntry> = page
        .items
        .iter()
        .map(|item| appmsg_item_to_entry(item, &account))
        .collect();
    let total = if page.total > 0 {
        page.total as usize
    } else {
        items.len()
    };
    // 同一份缓存记两个键:名称键(继续解析/重命名按输入找) + fakeid 键
    for key in [format!("account-query:{name}"), format!("account:{fakeid}")] {
        state.cache_batch(
            key,
            BatchCache {
                uid: fakeid.clone(),
                album_id: String::new(),
                kind: "account".into(),
                total,
                items: items.clone(),
                exhausted: items.len() >= total,
                url: String::new(),
                stale: 0,
            },
        );
    }
    Ok(Probe {
        kind: "account".into(),
        bid: fakeid.clone(),
        mid: fakeid.clone(),
        title: account.clone(),
        author: account.clone(),
        uid: fakeid.clone(),
        cover: page.items.first().map(|i| i.cover.clone()).unwrap_or_default(),
        note: format!(
            "公众号全部文章：共 {} 篇；点「解析全部」逐页加载（平台限频，几百篇要几分钟）",
            total
        ),
        total,
        loaded: items.len(),
        exhausted: items.len() >= total,
        qualities: Vec::new(),
        items,
    })
}""",
)

# 3) input_key 补 AccountQuery
s = s.replace(
    """        SourceTarget::Article { url } => format!("article:{}", url.trim()),
    }
}""",
    """        SourceTarget::Article { url } => format!("article:{}", url.trim()),
        SourceTarget::AccountQuery { name } => format!("account-query:{name}"),
    }
}""",
)

# 4) probe_more: account 偏移翻页分支(插在 album_id 判空之前)
s = s.replace(
    """    if cache.album_id.is_empty() {
        return Ok(ProbeMore {
            items: Vec::new(),
            loaded: cache.items.len(),
            total: cache.items.len(),
            exhausted: true,
            note: "这个来源没有更多内容".into(),
        });
    }
    if cache.exhausted {""",
    """    if cache.kind == "account" {
        // 公众号全量：appmsg 按 begin 偏移翻页(每页 5 条,页间退避限频)
        let token = state.mp_token.lock().unwrap().clone();
        if token.is_empty() {
            return Err("登录已失效，请重新登录后再试".into());
        }
        let fakeid = cache.uid.clone();
        let account = cache
            .items
            .last()
            .map(|i| i.author.clone())
            .unwrap_or_default();
        let want = want.max(5) as usize;
        let mut new_items: Vec<PostEntry> = Vec::new();
        let mut note = String::new();
        let mut total = cache.total;
        while new_items.len() < want {
            let begin = (cache.items.len() + new_items.len()) as u32;
            let page = match with_freq_retry(|| client.appmsg_page(&token, &fakeid, begin, 5), 3)
                .await
            {
                Ok(page) => page,
                Err(e) => {
                    note = format!("{e}；已加载的可先下载，稍后再点「继续解析」补齐");
                    break;
                }
            };
            if page.total > 0 {
                total = page.total as usize;
            }
            if page.items.is_empty() {
                break;
            }
            for item in &page.items {
                if cache.items.iter().any(|e| e.bid == item.aid)
                    || new_items.iter().any(|e| e.bid == item.aid)
                {
                    continue;
                }
                new_items.push(appmsg_item_to_entry(item, &account));
            }
            if (cache.items.len() + new_items.len()) >= total {
                break;
            }
        }
        let loaded = cache.items.len() + new_items.len();
        let exhausted = note.is_empty() && (new_items.is_empty() || loaded >= total);
        cache.total = total;
        cache.items.extend(new_items.clone());
        cache.exhausted = exhausted || loaded >= total;
        state.cache_batch(key, cache);
        return Ok(ProbeMore {
            items: new_items,
            loaded,
            total,
            exhausted: exhausted || loaded >= total,
            note: if !note.is_empty() {
                note
            } else if exhausted {
                "已到最后一篇".into()
            } else {
                String::new()
            },
        });
    }
    if cache.album_id.is_empty() {
        return Ok(ProbeMore {
            items: Vec::new(),
            loaded: cache.items.len(),
            total: cache.items.len(),
            exhausted: true,
            note: "这个来源没有更多内容".into(),
        });
    }
    if cache.exhausted {""",
)

# 5) 登录窗设施:替换四个占位命令
s = s.replace(
    """// ───────────────────────── 登录（阶段二占位） ─────────────────────────

#[tauri::command]
pub async fn web_login_open() -> Result<(), String> {
    Err("公众平台登录（UP主全量下载）将在下一个版本提供".into())
}

#[tauri::command]
pub async fn web_login_cookies() -> Result<LoginInfo, String> {
    Err("尚未登录".into())
}

#[tauri::command]
pub async fn web_login_watch() -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub async fn web_login_close() -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub async fn logout(state: State<'_, AppState>) -> Result<LoginInfo, String> {
    *state.login.write().unwrap() = None;
    let settings = state.settings_snapshot();
    let _ = std::fs::remove_file(settings.cookies_path());
    Ok(LoginInfo::default())
}""",
    """// ───────────────────────── 登录（内嵌网页 · 公众平台） ─────────────────────────

/// 唯一的浏览器窗口：登录用（WebView2 数据目录跨窗口共享不可靠，XHSdown 实测踩过）。
async fn ensure_work_window(
    app: &tauri::AppHandle,
    settings: &Settings,
) -> Result<tauri::WebviewWindow, String> {
    if let Some(window) = app.get_webview_window("wx-work") {
        return Ok(window);
    }
    let dir = crate::state::webview_dir(settings);
    std::fs::create_dir_all(&dir).map_err(err)?;
    let app2 = app.clone();
    let window = tauri::WebviewWindowBuilder::new(
        app,
        "wx-work",
        tauri::WebviewUrl::External("about:blank".parse().unwrap()),
    )
    .title("WXdown · 登录微信公众平台")
    .inner_size(560.0, 760.0)
    .visible(false)
    .data_directory(dir)
    // 平台后台的链接常以新窗口打开：统一改成就地跳转，避免"点了没反应"
    .on_new_window(move |url, _features| {
        let text = url.to_string();
        if text.starts_with("http://") || text.starts_with("https://") {
            if let Some(work) = app2.get_webview_window("wx-work") {
                let _ = work.navigate(url.clone());
            }
        }
        tauri::webview::NewWindowResponse::Deny
    })
    .build()
    .map_err(err)?;
    Ok(window)
}

/// 从后台首页 URL 里抠 token（登录成功的落点是 /cgi-bin/home?…&token=XXXX）。
fn token_from_url(url: &str) -> Option<String> {
    let query = url.split_once('?').map(|(_, q)| q).unwrap_or("");
    wx_core::parser::query_value(query, "token").filter(|t| !t.is_empty())
}

/// 收割平台 Cookie 装进 reqwest 会话（平台接口没有 JS 签名，cookie+token 直调即可，
/// 与小红书不同——实测路线，wechat-article-exporter 同款）。
fn harvest_mp_cookies(window: &tauri::WebviewWindow) -> Vec<(String, String)> {
    let mut cookies = Vec::new();
    let Ok(url) = tauri::Url::parse("https://mp.weixin.qq.com/") else {
        return cookies;
    };
    if let Ok(list) = window.cookies_for_url(url) {
        for c in list {
            cookies.push((c.name().to_string(), c.value().to_string()));
        }
    }
    cookies
}

/// 登录成功收尾：token/Cookie/昵称落账，推 login://confirmed。
async fn finish_mp_login(
    app: &tauri::AppHandle,
    token: String,
    cookies: Vec<(String, String)>,
    hide_window: bool,
) -> Result<LoginInfo, String> {
    let state = app.state::<AppState>();
    state.client().install_cookies(&cookies);
    state.save_cookies();
    *state.mp_token.lock().unwrap() = token.clone();

    // 昵称从后台首页抠（拿不到就给通用名，不影响功能）
    let uname = {
        let client = state.client();
        let home = client
            .fetch_text(
                &format!("https://mp.weixin.qq.com/cgi-bin/home?t=home/index&lang=zh_CN&token={token}"),
                "https://mp.weixin.qq.com/",
            )
            .await
            .unwrap_or_default();
        extract_platform_nickname(&home)
    };

    let login = LoginInfo {
        logged_in: true,
        uname: uname.clone(),
        face: String::new(),
        mid: token.clone(),
        ..Default::default()
    };
    *state.login.write().unwrap() = Some(login.clone());
    wx_core::login::debug_log(&format!("公众平台登录完成：昵称={uname:?}"));
    if hide_window {
        if let Some(window) = app.get_webview_window("wx-work") {
            let _ = window.hide();
        }
    }
    state.login_window_open.store(false, Ordering::Relaxed);
    let _ = app.emit("login://confirmed", login.clone());
    Ok(login)
}

/// 后台首页 HTML → 公众号昵称（多级兜底）。
fn extract_platform_nickname(home_html: &str) -> String {
    // 常见形态:var nickname = "xxx" / "nick_name":"xxx" / <title>xxx</title>
    for pat in [
        r#"var\s+nickname\s*=\s*"([^"]{1,40})""#,
        r#""nick_name"\s*:\s*"([^"]{1,40})""#,
        r#"<title>([^<>]{1,40})</title>"#,
    ] {
        if let Ok(re) = regex::Regex::new(pat) {
            if let Some(c) = re.captures(home_html) {
                let v = c.get(1).map(|m| m.as_str()).unwrap_or_default().trim();
                if !v.is_empty() && v != "首页" && v != "登录" {
                    return v.to_string();
                }
            }
        }
    }
    "公众平台账号".into()
}

/// 后端登录轮询（WebView2 遮挡降频的坑由后端定时器规避——XHSdown 实测结论）。
async fn login_watch(app: tauri::AppHandle) {
    let state = app.state::<AppState>();
    let started = std::time::Instant::now();
    loop {
        if started.elapsed() > Duration::from_secs(600) {
            wx_core::login::debug_log("登录轮询：超 10 分钟，停止");
            break;
        }
        tokio::time::sleep(Duration::from_millis(2500)).await;
        if !state.login_window_open.load(Ordering::Relaxed) {
            wx_core::login::debug_log("登录轮询：窗口已关，停止");
            break;
        }
        let Some(window) = app.get_webview_window("wx-work") else {
            break;
        };
        let url = window.url().map(|u| u.to_string()).unwrap_or_default();
        // 登录成功的落点一定带 token=
        if url.contains("mp.weixin.qq.com") {
            if let Some(token) = token_from_url(&url) {
                let cookies = harvest_mp_cookies(&window);
                wx_core::login::debug_log(&format!(
                    "检测到平台登录(token={}…),Cookie {} 个",
                    &token[..token.len().min(4)],
                    cookies.len()
                ));
                let app2 = app.clone();
                let _ = tauri::async_runtime::spawn(async move {
                    if let Err(e) = finish_mp_login(&app2, token, cookies, true).await {
                        wx_core::login::debug_log(&format!("登录收尾失败：{e}"));
                    }
                })
                .await;
                break;
            }
        }
    }
    state.login_watch_running.store(false, Ordering::Relaxed);
}

#[tauri::command]
pub async fn web_login_open(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let settings = state.settings_snapshot();
    let window = ensure_work_window(&app, &settings).await?;
    let url: tauri::Url = "https://mp.weixin.qq.com/".parse().unwrap();
    window.navigate(url).map_err(err)?;
    let _ = window.set_title("WXdown · 登录微信公众平台");
    let _ = window.show();
    let _ = window.set_focus();
    state.login_window_open.store(true, Ordering::Relaxed);
    Ok(())
}

#[tauri::command]
pub async fn web_login_cookies(state: State<'_, AppState>) -> Result<LoginInfo, String> {
    let login = state.login.read().unwrap().clone().unwrap_or_default();
    if login.logged_in {
        Ok(login)
    } else {
        Err("尚未登录".into())
    }
}

#[tauri::command]
pub async fn web_login_watch(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if state.login_watch_running.swap(true, Ordering::Relaxed) {
        return Ok(());
    }
    tauri::async_runtime::spawn(login_watch(app));
    Ok(())
}

#[tauri::command]
pub async fn web_login_close(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    state.login_window_open.store(false, Ordering::Relaxed);
    if let Some(window) = app.get_webview_window("wx-work") {
        let _ = window.hide();
    }
    Ok(())
}

#[tauri::command]
pub async fn logout(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<LoginInfo, String> {
    *state.login.write().unwrap() = None;
    *state.mp_token.lock().unwrap() = String::new();
    let settings = state.settings_snapshot();
    let _ = std::fs::remove_file(settings.cookies_path());
    // 浏览器目录删不掉(WebView2 占用)就留标记,下次启动清——避免"登出又自动登录"
    let _ = std::fs::remove_dir_all(crate::state::webview_dir(&settings));
    if crate::state::webview_dir(&settings).exists() {
        let _ = std::fs::write(crate::state::webview_logout_marker(&settings), b"");
    }
    if let Some(window) = app.get_webview_window("wx-work") {
        let _ = window.close();
    }
    Ok(LoginInfo::default())
}""",
)

open(p, "w", encoding="utf-8").write(s)
print("stage2 commands 写入")
