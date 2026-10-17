//! 命令层：把 wx-core 的能力暴露给界面。所有命令返回 Result<T, String>，
//! 错误一律转成人话（界面 toast 直接显示）。
//!
//! 阶段一范围：公众号文章（单篇）与合集（getalbum 免登录枚举）。
//! 公众平台登录（UP主全量枚举）与小程序嗅探在后续阶段加。

use crate::naming;
use crate::state::{AppState, BatchCache, Settings};
use crate::types::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tauri::Manager;
use std::time::Duration;
use tauri::Emitter;
use tauri::State;
use tauri_plugin_dialog::DialogExt;
use wx_core::album::AlbumItem;
use wx_core::article::ArticleData;
use wx_core::download as dl;
use wx_core::parser::{parse_input, SourceTarget};
use wx_core::WxClient;

const TASK_EVENT: &str = "task://update";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

type PauseFlag = Arc<std::sync::atomic::AtomicBool>;

fn flag_set(flag: &PauseFlag) -> bool {
    flag.load(std::sync::atomic::Ordering::Relaxed)
}

fn emit_task(app: &tauri::AppHandle, task: &TaskUpdate) {
    let _ = app.emit(TASK_EVENT, task);
}

// ───────────────────────── 应用状态 ─────────────────────────

#[tauri::command]
pub async fn app_status(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<AppStatus, String> {
    // 有 Cookie 备份 → 后台开隐藏窗口恢复平台登录态(拿新 token)
    if state.cookies_saved() {
        let app2 = app.clone();
        tauri::async_runtime::spawn(async move { restore_mp_login(app2).await });
    }
    let settings = state.settings_snapshot();
    let login = state.login.read().unwrap().clone().unwrap_or_default();
    Ok(AppStatus {
        version: APP_VERSION.to_string(),
        login,
        output_dir: settings.output_dir.to_string_lossy().to_string(),
        cookies_path: settings.cookies_path().to_string_lossy().to_string(),
    })
}

#[tauri::command]
pub fn app_settings(state: State<'_, AppState>) -> Result<SettingsEnv, String> {
    let settings = state.settings_snapshot();
    Ok(SettingsEnv {
        cookies_path: settings.cookies_path().to_string_lossy().to_string(),
        cookies_saved: state.cookies_saved(),
        version: APP_VERSION.to_string(),
        settings,
    })
}

#[tauri::command]
pub fn update_settings(
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<SettingsEnv, String> {
    let mut next = settings;
    next.clamp();

    let old = state.settings_snapshot();
    let proxy_changed = old.proxy != next.proxy;
    let slots_changed = old.max_concurrent_tasks != next.max_concurrent_tasks;

    if proxy_changed {
        let client = WxClient::with_proxy(Some(&next.proxy)).map_err(err)?;
        // 换会话时搬全部 Cookie（公众号会话没有"访客/登录"分桶的讲究）
        client.install_cookies(&state.client().export_cookies());
        *state.client.write().unwrap() = Arc::new(client);
    }
    if slots_changed {
        state.rebuild_slots();
    }

    next.save().map_err(err)?;
    *state.settings.lock().unwrap() = next.clone();
    state.save_cookies();

    Ok(SettingsEnv {
        cookies_path: next.cookies_path().to_string_lossy().to_string(),
        cookies_saved: true,
        version: APP_VERSION.to_string(),
        settings: next,
    })
}

// ───────────────────────── 来源解析 ─────────────────────────

/// 合集条目 → 表格条目。
fn album_item_to_entry(item: &AlbumItem, account: &str) -> PostEntry {
    PostEntry {
        bid: item.msgid.clone(),
        mid: String::new(),
        title: naming::title_of(&item.title),
        author: account.to_string(),
        created_at: item.create_time,
        pics: 0,
        has_video: false,
        is_audio: false,
        media_url: String::new(),
        page_url: item.url.clone(),
        duration: 0,
        is_retweet: false,
        media_unknown: true,
        digest: item.content.clone(),
    }
}

/// 文章 → 表格条目。
fn article_to_entry(data: &ArticleData) -> PostEntry {
    PostEntry {
        bid: if !data.sn.is_empty() {
            data.sn.clone()
        } else {
            data.mid.clone()
        },
        mid: data.biz.clone(),
        title: naming::title_of(&data.title),
        author: data.account.clone(),
        created_at: data.publish_ts,
        pics: data.images.len() as u32,
        has_video: !data.videos.is_empty(),
        is_audio: false,
        media_url: String::new(),
        page_url: data.url.clone(),
        duration: 0,
        is_retweet: false,
        media_unknown: false,
        digest: String::new(),
    }
}

fn quality_options(has_video: bool) -> Vec<QualityOption> {
    if !has_video {
        return Vec::new();
    }
    vec![QualityOption {
        value: "auto".into(),
        label: "最佳可用".into(),
        available: true,
        hint: "文章内视频按能解析到的最高画质保存".into(),
    }]
}

/// 来源的唯一键（批量缓存与重命名按它取回）。
fn input_key(target: &SourceTarget) -> String {
    match target {
        SourceTarget::Album { biz, album_id, .. } => format!("album:{biz}:{album_id}"),
        SourceTarget::Article { url } => format!("article:{}", url.trim()),
        SourceTarget::AccountQuery { name } => format!("account-query:{name}"),
    }
}

#[tauri::command]
pub async fn probe_source(
    state: State<'_, AppState>,
    input: String,
) -> Result<Probe, String> {
    let client = state.client();
    let target = parse_probe_target(&client, &input).await?;

    match target {
        SourceTarget::Article { url } => {
            let (_, data) = client.fetch_article(&url).await.map_err(err)?;
            // 文章属于合集 → 自动展开整个合集（BILIdown 同款行为）；
            // 只想下这一篇，在表格里只勾它即可。合集拉取失败则退回单篇。
            if !data.album_id.is_empty() {
                match album_probe(&state, &client, &data.album_biz, &data.album_id, &url).await {
                    Ok(mut probe) => {
                        // 用「文章输入」这个键也缓存一份：继续解析/重命名拿文章链接来找
                        state.cache_batch(
                            input_key(&SourceTarget::Article { url: url.clone() }),
                            BatchCache {
                                uid: probe.uid.clone(),
                                album_id: probe.bid.clone(),
                                kind: "album".into(),
                                total: probe.total,
                                cursor: 0,
                                items: probe.items.clone(),
                                exhausted: probe.exhausted,
                                url: url.clone(),
                                stale: 0,
                            },
                        );
                        probe.note = format!(
                            "文章属于合集《{}》——已展开整个合集；只下这一篇，勾选对应行即可。{}",
                            probe.title, probe.note
                        );
                        return Ok(probe);
                    }
                    Err(e) => {
                        wx_core::login::debug_log(&format!(
                            "展开文章所属合集失败，退回单篇：{e}"
                        ));
                    }
                }
            }
            let entry = article_to_entry(&data);
            Ok(Probe {
                kind: "article".into(),
                bid: entry.bid.clone(),
                mid: data.biz.clone(),
                title: entry.title.clone(),
                author: entry.author.clone(),
                uid: data.biz.clone(),
                cover: data.cover.clone(),
                note: format!(
                    "正文 {} 字 · {} 图 · {} 视频 · {} 语音",
                    data.text.chars().count(),
                    data.images.len(),
                    data.videos.len(),
                    data.audios.len()
                ),
                total: 1,
                loaded: 1,
                exhausted: true,
                qualities: quality_options(!data.videos.is_empty()),
                items: vec![entry],
            })
        }
        SourceTarget::Album { biz, album_id, url } => {
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
    let mut token = state.mp_token.lock().unwrap().clone();
    if token.is_empty() {
        let settings = state.settings_snapshot();
        token = std::fs::read_to_string(settings.data_root().join("mp_token.txt"))
            .unwrap_or_default()
            .trim()
            .to_string();
    }
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
    let mut delay = Duration::from_secs(5);
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
    let page = with_freq_retry(|| client.appmsg_page(&token, &fakeid, 0, 5), 5).await?;
    let account = hit.nickname.clone();
    let items: Vec<PostEntry> = page
        .items
        .iter()
        .map(|item| appmsg_item_to_entry(item, &account))
        .collect();
    let total = page.total as usize;
    // 同一份缓存记两个键:名称键(继续解析/重命名按输入找) + fakeid 键
    for key in [format!("account-query:{name}"), format!("account:{fakeid}")] {
        state.cache_batch(
            key,
            BatchCache {
                uid: fakeid.clone(),
                album_id: String::new(),
                kind: "account".into(),
                total,
                cursor: 5,
                items: items.clone(),
                exhausted: page.end,
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
            "公众号全部文章：{} 次发布已出 {} 篇；点「解析全部」逐页加载到底（平台限频，几百篇要几分钟）",
            total,
            items.len()
        ),
        total,
        loaded: items.len(),
        exhausted: page.end,
        qualities: Vec::new(),
        items,
    })
}

/// 取合集第一页并组装 Probe（缓存批量来源）。文章自动展开合集也走这里。
async fn album_probe(
    state: &State<'_, AppState>,
    client: &WxClient,
    biz: &str,
    album_id: &str,
    url: &str,
) -> Result<Probe, String> {
    let page = client
        .album_page(biz, album_id, 20, None)
        .await
        .map_err(err)?;
    let account = page.info.nickname.clone();
    let items: Vec<PostEntry> = page
        .items
        .iter()
        .map(|item| album_item_to_entry(item, &account))
        .collect();
    let total = if page.info.article_count > 0 {
        page.info.article_count as usize
    } else {
        items.len()
    };
    // 缓存整个来源（继续解析与重命名都要用）
    state.cache_batch(
        input_key(&SourceTarget::Album {
            biz: biz.to_string(),
            album_id: album_id.to_string(),
            url: url.to_string(),
        }),
        BatchCache {
            uid: biz.to_string(),
            album_id: album_id.to_string(),
            kind: "album".into(),
            total,
            cursor: 0,
            items: items.clone(),
            exhausted: !page.continue_flag,
            url: url.to_string(),
            stale: 0,
        },
    );
    Ok(Probe {
        kind: "album".into(),
        bid: album_id.to_string(),
        mid: biz.to_string(),
        title: page.info.title.clone(),
        author: account.clone(),
        uid: biz.to_string(),
        cover: page
            .items
            .iter()
            .find(|i| !i.cover.is_empty())
            .map(|i| i.cover.clone())
            .unwrap_or_default(),
        note: if page.info.description.is_empty() {
            format!("合集共 {} 篇", total)
        } else {
            format!("{}（共 {} 篇）", page.info.description, total)
        },
        total,
        loaded: items.len(),
        exhausted: !page.continue_flag,
        qualities: Vec::new(),
        items,
    })
}

/// 解析输入；遇到外层跳转短链先解一次重定向再解析。
async fn parse_probe_target(client: &WxClient, input: &str) -> Result<SourceTarget, String> {
    match parse_input(input) {
        Ok(target) => Ok(target),
        Err(e) => {
            let msg = e.to_string();
            if let Some(short) = msg.strip_prefix("NEED_RESOLVE:") {
                let resolved = client.resolve_redirect(short).await.map_err(err)?;
                parse_input(&resolved).map_err(err)
            } else {
                Err(msg)
            }
        }
    }
}

/// 继续解析：合集翻下一页。
#[tauri::command]
pub async fn probe_more(
    state: State<'_, AppState>,
    input: String,
    want: u32,
) -> Result<ProbeMore, String> {
    let client = state.client();
    let target = parse_probe_target(&client, &input).await?;
    let key = input_key(&target);

    let cache = {
        let batches = state.batches.lock().unwrap();
        batches.get(&key).cloned()
    };
    // 单篇文章没有"更多"可言；合集（含文章自动展开的合集）按缓存翻页
    let Some(mut cache) = cache else {
        if matches!(target, SourceTarget::Album { .. }) {
            return Err("这个来源的解析结果已过期，请重新解析后再试".into());
        }
        return Ok(ProbeMore {
            items: Vec::new(),
            loaded: 0,
            total: 0,
            exhausted: true,
            note: "这个来源没有更多内容".into(),
        });
    };
    if cache.kind == "account" {
        // 公众号全量：appmsgpublish 按发布批次翻页(cursor 为下一页 begin,每页 5 批,
        // 一批可含多篇文章);页间固定间隔 + 限频退避,翻完的标志是 publish_list 为空
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
        let mut cursor = cache.cursor;
        let mut exhausted = false;
        while new_items.len() < want {
            let page =
                match with_freq_retry(|| client.appmsg_page(&token, &fakeid, cursor, 5), 4).await {
                    Ok(page) => page,
                    Err(e) => {
                        note = format!("{e}；已加载的可先下载，等一两分钟再点「继续解析」补齐");
                        break;
                    }
                };
            if page.total > 0 {
                total = page.total as usize;
            }
            if page.end {
                exhausted = true;
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
            cursor += 5;
            // 页间节奏：猛拉必触发 freq control（实测）
            tokio::time::sleep(Duration::from_millis(2600)).await;
        }
        let loaded = cache.items.len() + new_items.len();
        cache.total = total;
        cache.cursor = cursor;
        cache.items.extend(new_items.clone());
        cache.exhausted = exhausted;
        state.cache_batch(key, cache);
        return Ok(ProbeMore {
            items: new_items,
            loaded,
            total,
            exhausted,
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
    if cache.exhausted {
        return Ok(ProbeMore {
            items: Vec::new(),
            loaded: cache.items.len(),
            total: cache.items.len(),
            exhausted: true,
            note: "已经全部加载".into(),
        });
    }
    let last = cache
        .items
        .last()
        .cloned()
        .ok_or_else(|| "缓存里没有条目，请重新解析".to_string())?;
    let begin_msgid = album_entry_msgid(&last);
    let page = client
        .album_page(
            &cache.uid,
            &cache.album_id,
            want.max(5).min(20),
            Some((&begin_msgid, "1")),
        )
        .await
        .map_err(err)?;
    let account = cache
        .items
        .last()
        .map(|i| i.author.clone())
        .unwrap_or_default();
    let new_items: Vec<PostEntry> = page
        .items
        .iter()
        .filter(|item| {
            // 去重：翻页参数失效时接口会重复给同一页
            let msgid = &item.msgid;
            !cache.items.iter().any(|e| e.bid == *msgid)
        })
        .map(|item| album_item_to_entry(item, &account))
        .collect();
    cache.items.extend(new_items.clone());
    cache.exhausted = !page.continue_flag;
    let loaded = cache.items.len();
    state.cache_batch(key, cache);
    Ok(ProbeMore {
        items: new_items,
        loaded,
        total: 0,
        exhausted: !page.continue_flag,
        note: if page.continue_flag {
            String::new()
        } else {
            "已到合集末尾".into()
        },
    })
}

fn album_entry_msgid(entry: &PostEntry) -> String {
    if !entry.bid.is_empty() {
        entry.bid.clone()
    } else {
        // 从 page_url 里抽 mid
        wx_core::parser::query_value(
            entry.page_url.split_once('?').map(|(_, q)| q).unwrap_or(""),
            "mid",
        )
        .unwrap_or_default()
    }
}

// ───────────────────────── 下载任务 ─────────────────────────

#[tauri::command]
pub async fn start_download(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    req: DownloadReq,
) -> Result<String, String> {
    let id = state.next_task_id();
    let task = TaskUpdate {
        id: id.clone(),
        title: req.title.clone(),
        quality_label: req.quality_label.clone(),
        status: "queued".into(),
        image_count: req.image_count,
        video_count: req.video_count,
        is_audio: false,
        message: "排队中".into(),
        ..Default::default()
    };
    emit_task(&app, &task);

    let pause = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let handle = tokio::spawn(run_task(app.clone(), req.clone(), id.clone()));
    state.tasks.lock().unwrap().insert(
        id.clone(),
        crate::state::TaskEntry {
            snapshot: Arc::new(Mutex::new(task)),
            abort: Some(handle.abort_handle()),
            req,
            pause,
        },
    );
    Ok(id)
}

#[tauri::command]
pub fn cancel_download(app: tauri::AppHandle, state: State<'_, AppState>, task_id: String) -> Result<(), String> {
    let snapshot = {
        let tasks = state.tasks.lock().unwrap();
        tasks.get(&task_id).map(|e| {
            if let Some(abort) = &e.abort {
                abort.abort();
            }
            e.snapshot.clone()
        })
    };
    if let Some(snapshot) = snapshot {
        let mut task = snapshot.lock().unwrap();
        // 必须写回快照本体：孤儿 reporter 每 500ms 会重播快照，
        // 只改克隆的话「已取消」立刻被覆盖回「下载中」
        if !matches!(task.status.as_str(), "done" | "failed" | "canceled") {
            task.status = "canceled".into();
            task.message = "已取消".into();
            task.speed_bps = 0.0;
        }
        emit_task(&app, &task.clone());
    }
    Ok(())
}

/// 一篇文章的下载流水线：
/// 抓文章 → 建条目文件夹 → 图片 → 视频/语音 → 正文 txt → 离线 HTML 存档。
async fn run_task(app: tauri::AppHandle, req: DownloadReq, id: String) {
    let state = app.state::<AppState>();
    let snapshot = state
        .tasks
        .lock()
        .unwrap()
        .get(&id)
        .map(|e| e.snapshot.clone());
    let Some(snapshot) = snapshot else { return };
    let pause = {
        let tasks = state.tasks.lock().unwrap();
        tasks
            .get(&id)
            .map(|e| e.pause.clone())
            .unwrap_or_else(|| Arc::new(std::sync::atomic::AtomicBool::new(false)))
    };
    // 创建后立刻被暂停（如「全部暂停」在排队期间触发）：直接进入暂停态
    if flag_set(&pause) {
        let task = {
            let mut task = snapshot.lock().unwrap();
            task.status = "paused".into();
            task.message = "已暂停".into();
            task.clone()
        };
        emit_task(&app, &task);
        return;
    }

    let settings = state.settings_snapshot();
    let client = state.client();
    let _slot = state.slots().acquire_owned().await;

    let app_done = app.clone();
    let snapshot_done = snapshot.clone();
    let done = move |status: &str, message: &str, output: &str| {
        let task = {
            // 必须写回快照本体：只改克隆的话，后端账本停在旧状态
            let mut t = snapshot_done.lock().unwrap();
            t.status = status.to_string();
            t.message = message.to_string();
            t.speed_bps = 0.0;
            if !output.is_empty() {
                t.output_path = output.to_string();
            }
            t.clone()
        };
        emit_task(&app_done, &task);
    };

    // ── 抓文章 ──
    // "文字+视频"类动态的文章页正文全靠 JS 渲染（服务端 HTML 里没有文字），
    // 解析失败时降级用合集接口带来的原文兜底，而不是让任务失败。
    let fetch_url = if req.page_url.is_empty() {
        return done("failed", "缺少文章地址", "");
    } else {
        req.page_url.clone()
    };
    let fallback = req.fallback_text.trim().to_string();
    let mut used_fallback = false;
    let (raw_html, mut article) = match client.fetch_article(&fetch_url).await {
        Ok((raw_html, data)) => {
            let mut data = data;
            // 页面解析成功但正文是空壳（动态页只有标点符号）→ 同样用兜底补正文
            let text_len = data.text.trim().chars().count();
            if text_len < 5 && data.images.is_empty() && !fallback.is_empty() {
                wx_core::login::debug_log(&format!(
                    "文章页正文为空壳（{} 字符），用合集接口原文兜底",
                    text_len
                ));
                if data.title.is_empty() {
                    data.title = wx_core::login::title_preview(&fallback, 30);
                }
                data.text = fallback.clone();
                used_fallback = true;
            }
            (raw_html, data)
        }
        Err(e) => {
            if fallback.is_empty() {
                done("failed", &err(e), "");
                return;
            }
            wx_core::login::debug_log(&format!(
                "文章页解析失败，用合集接口原文兜底：{}",
                err(e)
            ));
            let data = ArticleData {
                title: wx_core::login::title_preview(&fallback, 30),
                text: fallback.clone(),
                url: fetch_url.clone(),
                ..Default::default()
            };
            used_fallback = true;
            (String::new(), data)
        }
    };

    let title = {
        let t = naming::title_of(&article.title);
        if t.is_empty() { req.title.clone() } else { t }
    };
    let author = if req.author.is_empty() {
        article.account.clone()
    } else {
        req.author.clone()
    };
    let publish_ts = if article.publish_ts > 0 {
        article.publish_ts
    } else {
        req.naming
            .publish_date
            .parse::<i64>()
            .ok()
            .and_then(|_| date_to_unix(&req.naming.publish_date))
            .unwrap_or(0)
    };

    // ── 渲染路径 ──
    // 文件夹层级：{author}/{collection}（collection 空时退回 年-月）
    let (py, pm, _pd) = civil_from_unix(publish_ts + 8 * 3600);
    let collection = if !req.collection.is_empty() {
        req.collection.clone()
    } else if !article.publish_text.is_empty() || publish_ts > 0 {
        format!("{py}-{pm:02}")
    } else {
        "文章".to_string()
    };
    let mut vars: HashMap<String, String> = HashMap::new();
    vars.insert("title".into(), title.clone());
    vars.insert("author".into(), author.clone());
    vars.insert("collection".into(), collection);
    vars.insert("bid".into(), req.bid.clone());
    vars.insert("uid".into(), req.uid.clone());
    let publish_date = if publish_ts > 0 {
        let (y, m, d) = civil_from_unix(publish_ts + 8 * 3600);
        format!("{y}-{m:02}-{d:02}")
    } else {
        req.naming.publish_date.clone()
    };
    vars.insert("publish_date".into(), publish_date.clone());
    vars.insert("date".into(), req.naming.date.clone());
    vars.insert("year".into(), format!("{py}"));
    vars.insert("month".into(), format!("{pm:02}"));
    vars.insert("source_kind".into(), req.naming.source_kind.clone());
    vars.insert("index".into(), naming::padded_index(req.naming.index, req.naming.index_pad));
    vars.insert("ext".into(), String::new());

    let folder = naming::render(&settings.folder_template, &vars, true);
    let entry_name = naming::render(&settings.naming_template, &vars, true);
    let base_dir = settings.output_dir.join(&folder);
    if let Err(e) = tokio::fs::create_dir_all(&base_dir).await {
        done("failed", &format!("创建目录失败：{e}"), "");
        return;
    }

    // 条目是文件夹：重名策略沿用家族三选
    let conflict = settings.rename_conflict.clone();
    let entry_dir = |name: &str| -> PathBuf {
        let mut dir = base_dir.join(name);
        if conflict == "auto" {
            let mut seq = 1;
            while dir.exists() {
                dir = base_dir.join(format!("{name} ({seq})"));
                seq += 1;
            }
        }
        dir
    };
    let entry = entry_dir(&entry_name);

    // skip：条目文件夹里已有正文 → 整条跳过
    if conflict == "skip" && entry.join("正文.txt").exists() {
        let size = dir_size(&entry);
        {
            let mut task = snapshot.lock().unwrap();
            task.image_pct = 100.0;
            task.video_pct = 100.0;
            if size > 0 {
                task.downloaded = size;
                task.total = size;
            }
        }
        done("done", "文件已存在，跳过下载", &entry.to_string_lossy());
        return;
    }
    if let Err(e) = tokio::fs::create_dir_all(&entry).await {
        done("failed", &format!("创建条目文件夹失败：{e}"), "");
        return;
    }

    let opts = dl::DownloadOptions {
        retries: settings.retry_count as usize,
    };
    let mut downloaded_total: u64 = 0;

    // ── 阶段一：图片（01.jpg、02.png …）──
    let pic_total = article.images.len();
    let mut image_map: Vec<(String, String)> = Vec::new(); // (原 URL, 相对路径)
    if pic_total > 0 {
        {
            let mut task = snapshot.lock().unwrap();
            task.status = "downloading".into();
            task.message = "下载图片".into();
        }
        for (index, img) in article.images.iter().enumerate() {
            if flag_set(&pause) {
                return paused_here(&app, &snapshot, downloaded_total);
            }
            let rel = format!("{:02}.{}", index + 1, img.fmt);
            let dest = entry.join(&rel);
            image_map.push((img.url.clone(), rel.clone()));

            if dest.exists() {
                let size = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
                downloaded_total += size;
                {
                    let mut task = snapshot.lock().unwrap();
                    task.image_pct = (index + 1) as f64 / pic_total as f64 * 100.0;
                    emit_task(&app, &task.clone());
                }
                continue;
            }

            let snap = snapshot.clone();
            let app_c = app.clone();
            let base_before = downloaded_total;
            let pic_index = index;
            let pic_total_c = pic_total;
            let progress: dl::ProgressFn = Arc::new(move |p| {
                let mut task = snap.lock().unwrap();
                if matches!(task.status.as_str(), "done" | "failed" | "canceled" | "paused") {
                    return;
                }
                task.downloaded = base_before + p.downloaded;
                task.total = base_before + p.total;
                task.speed_bps = p.speed_bps;
                task.image_pct = (pic_index as f64 + p.downloaded as f64 / p.total.max(1) as f64)
                    / pic_total_c as f64
                    * 100.0;
                emit_task(&app_c, &task.clone());
            });

            match dl::download_resumable(
                &client.http,
                &img.url,
                &dest,
                &opts,
                progress,
                Some(pause.clone()),
            )
            .await
            {
                Ok(dl::DownloadEnd::Completed(size)) => {
                    downloaded_total += size;
                    {
                        let mut task = snapshot.lock().unwrap();
                        task.image_pct = (index + 1) as f64 / pic_total as f64 * 100.0;
                        emit_task(&app, &task.clone());
                    }
                }
                Ok(dl::DownloadEnd::Paused(size)) => {
                    let mut task = snapshot.lock().unwrap();
                    task.status = "paused".into();
                    task.message = "已暂停（点击继续以恢复）".into();
                    task.downloaded = downloaded_total + size;
                    task.speed_bps = 0.0;
                    emit_task(&app, &task.clone());
                    return;
                }
                Err(e) => {
                    let _ = tokio::fs::remove_file(&dest).await;
                    wx_core::login::debug_log(&format!(
                        "图片 {} 下载失败（{}）：{}",
                        index + 1,
                        wx_core::error::truncate(&img.url, 120),
                        e
                    ));
                    // 单图失败不终止整条任务：记占位，继续后面的图
                    image_map.pop();
                    {
                        let mut task = snapshot.lock().unwrap();
                        task.message = format!("第 {} 张图片下载失败，已跳过", index + 1);
                        emit_task(&app, &task.clone());
                    }
                }
            }
        }
        {
            let mut task = snapshot.lock().unwrap();
            task.image_pct = 100.0;
            task.speed_bps = 0.0;
            emit_task(&app, &task.clone());
        }
    }

    // ── 阶段二：视频（尽力解析直链；失败记占位不失败）──
    let mut video_map: Vec<(String, Option<String>)> = Vec::new(); // (iframe src, 相对路径)
    let mut unresolved: Vec<String> = Vec::new();
    if !article.videos.is_empty() {
        {
            let mut task = snapshot.lock().unwrap();
            task.status = "downloading".into();
            task.message = "解析并下载视频".into();
        }
        for (index, video) in article.videos.iter().enumerate() {
            if flag_set(&pause) {
                return paused_here(&app, &snapshot, downloaded_total);
            }
            let direct = client.resolve_video(video).await;
            let Some(url) = direct else {
                let label = if video.kind == "tencent" {
                    format!("腾讯视频 {}", video.vid)
                } else if !video.vid.is_empty() {
                    format!("微信视频 {}", video.vid)
                } else {
                    format!("视频 {}", index + 1)
                };
                unresolved.push(format!("{label}：{}", video.src));
                video_map.push((video.src.clone(), None));
                wx_core::login::debug_log(&format!(
                    "视频直链解析失败（{label}）：{}",
                    wx_core::error::truncate(&video.src, 160)
                ));
                continue;
            };
            let rel = format!("video-{:02}.mp4", index + 1);
            let dest = entry.join(&rel);

            let snap = snapshot.clone();
            let app_c = app.clone();
            let base_before = downloaded_total;
            let progress: dl::ProgressFn = Arc::new(move |p| {
                let mut task = snap.lock().unwrap();
                if matches!(task.status.as_str(), "done" | "failed" | "canceled" | "paused") {
                    return;
                }
                task.downloaded = base_before + p.downloaded;
                task.total = base_before + p.total;
                task.speed_bps = p.speed_bps;
                task.video_pct =
                    p.downloaded as f64 / p.total.max(1) as f64 * 100.0;
                emit_task(&app_c, &task.clone());
            });

            match dl::download_resumable(
                &client.http,
                &url,
                &dest,
                &opts,
                progress,
                Some(pause.clone()),
            )
            .await
            {
                Ok(dl::DownloadEnd::Completed(size)) => {
                    downloaded_total += size;
                    video_map.push((video.src.clone(), Some(rel)));
                    {
                        let mut task = snapshot.lock().unwrap();
                        task.video_pct = 100.0;
                        task.speed_bps = 0.0;
                        emit_task(&app, &task.clone());
                    }
                }
                Ok(dl::DownloadEnd::Paused(size)) => {
                    let mut task = snapshot.lock().unwrap();
                    task.status = "paused".into();
                    task.message = "已暂停（点击继续以恢复）".into();
                    task.downloaded = downloaded_total + size;
                    task.speed_bps = 0.0;
                    emit_task(&app, &task.clone());
                    return;
                }
                Err(e) => {
                    let _ = tokio::fs::remove_file(&dest).await;
                    unresolved.push(format!("视频 {}（下载失败）：{}", index + 1, video.src));
                    video_map.push((video.src.clone(), None));
                    wx_core::login::debug_log(&format!(
                        "视频下载失败（{}）：{e}",
                        wx_core::error::truncate(&url, 160)
                    ));
                }
            }
        }
    }

    // ── 阶段三：语音 ──
    let mut audio_map: Vec<(String, String)> = Vec::new();
    if !article.audios.is_empty() {
        {
            let mut task = snapshot.lock().unwrap();
            task.status = "downloading".into();
            task.message = "下载语音".into();
        }
        for (index, audio) in article.audios.iter().enumerate() {
            if flag_set(&pause) {
                return paused_here(&app, &snapshot, downloaded_total);
            }
            let url = WxClient::audio_url(&audio.voice_id);
            let rel = format!("voice-{:02}.mp3", index + 1);
            let dest = entry.join(&rel);
            let progress: dl::ProgressFn = Arc::new(|_| {});
            match dl::download_resumable(
                &client.http,
                &url,
                &dest,
                &opts,
                progress,
                Some(pause.clone()),
            )
            .await
            {
                Ok(dl::DownloadEnd::Completed(size)) => {
                    downloaded_total += size;
                    audio_map.push((audio.voice_id.clone(), rel));
                }
                Ok(dl::DownloadEnd::Paused(size)) => {
                    let mut task = snapshot.lock().unwrap();
                    task.status = "paused".into();
                    task.message = "已暂停（点击继续以恢复）".into();
                    task.downloaded = downloaded_total + size;
                    emit_task(&app, &task.clone());
                    return;
                }
                Err(e) => {
                    let _ = tokio::fs::remove_file(&dest).await;
                    unresolved.push(format!("语音 {}（下载失败）", index + 1));
                    wx_core::login::debug_log(&format!(
                        "语音 {} 下载失败：{e}",
                        index + 1
                    ));
                }
            }
        }
    }

    // ── 阶段四：正文 txt ──
    if settings.download_text || true {
        {
            let mut task = snapshot.lock().unwrap();
            task.status = "saving".into();
            task.message = "保存正文与存档".into();
            task.video_pct = 100.0;
            emit_task(&app, &task.clone());
        }
        let date_text = if !article.publish_text.is_empty() {
            article.publish_text.clone()
        } else if publish_ts > 0 {
            wx_core::article::format_datetime_local(publish_ts)
        } else {
            String::new()
        };
        let mut body = format!(
            "{}\n\n作者：{}{}\n发布时间：{}\n原文：{}\n下载于：WXdown v{}\n\n——\n\n",
            article.title,
            article.account,
            if article.author.is_empty() {
                String::new()
            } else {
                format!("（作者：{}）", article.author)
            },
            date_text,
            article.url,
            APP_VERSION
        );
        body.push_str(&article.text);
        body.push_str("\n\n——\n");
        body.push_str(&format!(
            "\n本文件夹：{} 张图片、{} 个视频、{} 段语音",
            image_map.len(),
            video_map.iter().filter(|(_, p)| p.is_some()).count(),
            audio_map.len()
        ));
        if !unresolved.is_empty() {
            body.push_str("\n\n未能保存的媒体（直链解析失败，可点链接在微信中查看）：");
            for line in &unresolved {
                body.push_str(&format!("\n· {line}"));
            }
        }
        body.push('\n');
        let dest = entry.join("正文.txt");
        if let Err(e) = tokio::fs::write(&dest, body).await {
            done("failed", &format!("正文写入失败：{e}"), "");
            return;
        }
    }

    // ── 阶段五：离线 HTML 存档 ──
    let archive = build_archive_html(&raw_html, &image_map, &video_map, &audio_map);
    let archive_path = entry.join("index.html");
    if let Err(e) = tokio::fs::write(&archive_path, archive).await {
        // 存档失败不算任务失败（正文与媒体已完整）
        wx_core::login::debug_log(&format!("HTML 存档写入失败：{e}"));
    }

    {
        let mut task = snapshot.lock().unwrap();
        task.downloaded = task.total.max(downloaded_total);
    }
    wx_core::login::debug_log(&format!(
        "任务完成：{} → {}",
        title,
        entry.display()
    ));
    done("done", "已完成", &entry.to_string_lossy());
}

fn paused_here(
    app: &tauri::AppHandle,
    snapshot: &Arc<Mutex<TaskUpdate>>,
    downloaded: u64,
) {
    let task = {
        let mut task = snapshot.lock().unwrap();
        task.status = "paused".into();
        task.message = "已暂停".into();
        task.downloaded = downloaded;
        task.speed_bps = 0.0;
        task.clone()
    };
    emit_task(app, &task);
}

/// 目录大小（跳过下载的条目回填字节数用）。
fn dir_size(dir: &std::path::Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        if let Ok(entries) = std::fs::read_dir(&d) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if let Ok(meta) = entry.metadata() {
                    total += meta.len();
                }
            }
        }
    }
    total
}

/// "YYYY-MM-DD" → Unix 秒（北京时间）。
fn date_to_unix(date: &str) -> Option<i64> {
    let re = regex::Regex::new(r"(\d{4})-(\d{2})-(\d{2})").ok()?;
    let c = re.captures(date)?;
    let y: i64 = c.get(1)?.as_str().parse().ok()?;
    let m: i64 = c.get(2)?.as_str().parse().ok()?;
    let d: i64 = c.get(3)?.as_str().parse().ok()?;
    Some(wx_core::article::parse_datetime_local(&format!(
        "{y}-{m}-{d} 00:00"
    )))
}

// ───────────────────────── HTML 离线存档 ─────────────────────────

/// 原始文章 HTML → 本地化存档：
/// 去 script/noscript、解隐藏、图片 src 指到本地文件、
/// 视频 iframe 换本地 <video>（未解析则给链接占位）、语音换 <audio>。
fn build_archive_html(
    raw_html: &str,
    images: &[(String, String)],
    videos: &[(String, Option<String>)],
    audios: &[(String, String)],
) -> String {
    let mut html = raw_html.to_string();

    // 1) 去 script / noscript（存档页不需要 JS；noscript 内容会顶出来）
    let re_script = regex::Regex::new(r"(?si)<script\b[^>]*>.*?</script>").unwrap();
    html = re_script.replace_all(&html, "").into_owned();
    let re_noscript = regex::Regex::new(r"(?si)<noscript\b[^>]*>.*?</noscript>").unwrap();
    html = re_noscript.replace_all(&html, "").into_owned();

    // 2) 解除正文隐藏（页面靠 JS 把 js_content 显出来；无 JS 环境要保持可见）
    html = html.replace("visibility: hidden", "visibility: visible");
    html = html.replace("visibility:hidden", "visibility:visible");
    html = html.replace("opacity: 0;", "opacity: 1;");
    html = html.replace("opacity:0;", "opacity:1;");

    // 3) 图片：img 标签的 src 指到本地文件（data-src 保留无妨）
    let img_tag = regex::Regex::new(r#"(?si)<img\b[^>]*>"#).unwrap();
    html = img_tag
        .replace_all(&html, |caps: &regex::Captures| -> String {
            let tag = caps.get(0).unwrap().as_str();
            let data_src = attr_of(tag, "data-src")
                .map(|s| wx_core::article::decode_entities(&s))
                .unwrap_or_default();
            let Some(rel) = images.iter().find(|(u, _)| *u == data_src) else {
                return tag.to_string();
            };
            let local = format!("src=\"{}\"", rel.1);
            if let Some(src) = attr_span(tag, "src") {
                tag.replacen(&src, &local, 1)
            } else {
                // 没有 src 属性：补在标签尾
                if let Some(stripped) = tag.strip_suffix('>') {
                    format!("{} {}>", stripped.trim_end_matches('/'), local)
                } else {
                    tag.to_string()
                }
            }
        })
        .into_owned();

    // 4) 视频：iframe 换本地 <video> 或占位链接
    let iframe_tag = regex::Regex::new(r#"(?si)<iframe\b[^>]*video_iframe[^>]*>.*?</iframe>"#).unwrap();
    let iframe_bare = regex::Regex::new(r#"(?si)<iframe\b[^>]*video_iframe[^>]*/?>"#).unwrap();
    let replace_iframe = |tag: &str| -> String {
        let src = attr_of(tag, "data-src")
            .or_else(|| attr_of(tag, "src"))
            .map(|s| wx_core::article::decode_entities(&s))
            .unwrap_or_default();
        let Some((_, rel)) = videos.iter().find(|(s, _)| *s == src) else {
            return tag.to_string();
        };
        match rel {
            Some(local) => format!(
                "<video controls preload=\"metadata\" src=\"{local}\" style=\"max-width:100%;display:block;margin:8px auto;\"></video>"
            ),
            None => format!(
                "<div style=\"padding:12px;margin:8px 0;border:1px dashed #bbb;border-radius:6px;color:#888;font-size:14px;\">视频未能解析直链，<a href=\"{src}\" target=\"_blank\">点此在微信中查看</a></div>"
            ),
        }
    };
    html = iframe_tag
        .replace_all(&html, |caps: &regex::Captures| replace_iframe(caps.get(0).unwrap().as_str()))
        .into_owned();
    html = iframe_bare
        .replace_all(&html, |caps: &regex::Captures| replace_iframe(caps.get(0).unwrap().as_str()))
        .into_owned();

    // 5) 语音：mpvoice 标签换 <audio>
    let mpvoice_tag = regex::Regex::new(r#"(?si)<mpvoice\b[^>]*>"#).unwrap();
    html = mpvoice_tag
        .replace_all(&html, |caps: &regex::Captures| -> String {
            let tag = caps.get(0).unwrap().as_str();
            let vid = attr_of(tag, "voice_encode_fileid").unwrap_or_default();
            let Some((_, rel)) = audios.iter().find(|(v, _)| *v == vid) else {
                return tag.to_string();
            };
            format!(
                "<audio controls preload=\"none\" src=\"{rel}\" style=\"display:block;margin:8px 0;\"></audio>"
            )
        })
        .into_owned();

    // 6) 全局替换剩余的图片 URL 引用（meta/canonical 里的 data-src 等）
    for (orig, rel) in images {
        html = html.replace(orig.as_str(), rel);
    }
    html
}

/// 取标签里某属性的值（`attr="value"`）。
fn attr_of(tag: &str, name: &str) -> Option<String> {
    let re = regex::Regex::new(&format!(r#"(?i)\b{}="([^"]*)""#, regex::escape(name))).unwrap();
    re.captures(tag)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

/// 取标签里某属性完整片段（`attr="value"`，含引号）。
fn attr_span(tag: &str, name: &str) -> Option<String> {
    let re = regex::Regex::new(&format!(r#"(?i)\b{}="[^"]*""#, regex::escape(name))).unwrap();
    re.find(tag).map(|m| m.as_str().to_string())
}

// ───────────────────────── 暂停 / 继续 ─────────────────────────

fn set_paused(app: &tauri::AppHandle, state: &State<'_, AppState>, task_id: &str) -> Result<(), String> {
    let tasks = state.tasks.lock().unwrap();
    let Some(entry) = tasks.get(task_id) else {
        return Err("任务不存在或已结束".into());
    };
    entry.pause.store(true, std::sync::atomic::Ordering::Relaxed);
    let mut task = entry.snapshot.lock().unwrap();
    if matches!(task.status.as_str(), "queued" | "downloading" | "saving") {
        task.status = "paused".into();
        task.message = "已暂停".into();
        task.speed_bps = 0.0;
        // 排队中的任务没有下载循环替我们发事件，这里立即广播暂停态
        emit_task(app, &task.clone());
    }
    Ok(())
}

#[tauri::command]
pub fn pause_download(app: tauri::AppHandle, state: State<'_, AppState>, task_id: String) -> Result<(), String> {
    set_paused(&app, &state, &task_id)
}

#[tauri::command]
pub fn pause_all_downloads(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<usize, String> {
    let tasks = state.tasks.lock().unwrap();
    let mut count = 0;
    for entry in tasks.values() {
        let mut task = entry.snapshot.lock().unwrap();
        if matches!(task.status.as_str(), "queued" | "downloading" | "saving") {
            entry.pause.store(true, std::sync::atomic::Ordering::Relaxed);
            task.status = "paused".into();
            task.message = "已暂停".into();
            task.speed_bps = 0.0;
            emit_task(&app, &task.clone());
            count += 1;
        }
    }
    Ok(count)
}

/// 从暂停态恢复：用保存的请求重跑流水线（媒体走 .part 断点续传）。
/// 必须是 async：同步命令跑在主线程，tokio::spawn 在主线程会 panic（应用闪退）。
#[tauri::command]
pub async fn resume_download(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    task_id: String,
) -> Result<(), String> {
    let (req, snapshot) = {
        let tasks = state.tasks.lock().unwrap();
        let Some(entry) = tasks.get(&task_id) else {
            return Err("任务不存在或已结束".into());
        };
        if entry.snapshot.lock().unwrap().status != "paused" {
            return Err("任务不在暂停状态".into());
        }
        (entry.req.clone(), entry.snapshot.clone())
    };
    {
        let mut task = snapshot.lock().unwrap();
        task.status = "queued".into();
        task.message = "排队中".into();
        // 立即广播：恢复的任务可能要等并发槽，不发事件 UI 会一直停在「已暂停」
        emit_task(&app, &task.clone());
    }
    // 先换新旗标再 spawn：重跑的 run_task 起始就读旗标，
    // 旧旗标还停在 true 时会立即再次暂停（表现为「点了继续没反应」）
    let fresh_pause = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let mut tasks = state.tasks.lock().unwrap();
        let Some(entry) = tasks.get_mut(&task_id) else {
            return Err("任务不存在或已结束".into());
        };
        entry.pause = fresh_pause;
        // 先中止旧的 run_task 再 spawn：旧任务的收尾窗口里还占着并发槽
        if let Some(old) = entry.abort.take() {
            old.abort();
        }
    }
    let handle = tokio::spawn(run_task(app, req, task_id.clone()));
    if let Some(entry) = state.tasks.lock().unwrap().get_mut(&task_id) {
        entry.abort = Some(handle.abort_handle());
    }
    Ok(())
}

#[tauri::command]
pub async fn resume_all_downloads(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<usize, String> {
    let paused: Vec<String> = {
        let tasks = state.tasks.lock().unwrap();
        tasks
            .iter()
            .filter(|(_, e)| e.snapshot.lock().unwrap().status == "paused")
            .map(|(id, _)| id.clone())
            .collect()
    };
    let count = paused.len();
    for task_id in paused {
        let _ = resume_download(app.clone(), state.clone(), task_id).await;
    }
    Ok(count)
}

// ───────────────────────── 重命名 ─────────────────────────

/// 一次重命名动作的结果（预演与实操共用）。
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamePlan {
    pub renamed: usize,
    pub skipped: usize,
    pub missing: usize,
    pub details: Vec<String>,
    pub dry_run: bool,
}

/// 按当前命名规则，把已下载的条目重命名成新编号。
/// 条目是文件夹：匹配"去掉数字前缀后与标题相同"的文件夹，原地改名。
#[tauri::command]
pub async fn rename_downloaded(
    state: State<'_, AppState>,
    input: String,
    total: usize,
    dry_run: bool,
) -> Result<RenamePlan, String> {
    let settings = state.settings_snapshot();
    let client = state.client();
    let key = match parse_probe_target_sync(&client, &input) {
        Some(SourceTarget::Album { biz, album_id, .. }) => format!("album:{biz}:{album_id}"),
        _ => input.trim().to_string(),
    };
    let cache = {
        let batches = state.batches.lock().unwrap();
        batches.get(&key).cloned()
    };
    let Some(cache) = cache else {
        return Err("这个来源的解析结果已过期，请重新解析后再试".into());
    };
    let pad = format!("{total}").len().max(2);

    let collection_hint = input_collection_hint(&input);
    let mut plan = RenamePlan { dry_run, ..Default::default() };

    for (position, item) in cache.items.iter().enumerate() {
        let index = if total > 0 { std::cmp::max(total - position, 1) } else { position + 1 };
        let mut vars: HashMap<String, String> = HashMap::new();
        vars.insert("title".into(), item.title.clone());
        vars.insert("author".into(), item.author.clone());
        vars.insert("collection".into(), collection_hint.clone());
        vars.insert("bid".into(), item.bid.clone());
        vars.insert("uid".into(), cache.uid.clone());
        let (y, m, d) = {
            let secs = item.created_at + 8 * 3600;
            civil_from_unix(secs)
        };
        vars.insert("publish_date".into(), format!("{y}-{m:02}-{d:02}"));
        vars.insert("year".into(), format!("{y}"));
        vars.insert("month".into(), format!("{m:02}"));
        vars.insert("date".into(), format!("{y}-{m:02}-{d:02}"));
        vars.insert("index".into(), naming::padded_index(index as u32, pad));

        // 条目名是文件夹（无扩展名）
        let wanted_stem = naming::render(&settings.naming_template, &vars, true);

        let found =
            find_downloaded_by_title(&settings.output_dir, &item.title, &wanted_stem).await;
        match found {
            Some(found) => {
                if found
                    .file_name()
                    .map(|s| s.to_string_lossy() == wanted_stem)
                    .unwrap_or(false)
                {
                    plan.skipped += 1;
                    continue;
                }
                let target = found.with_file_name(&wanted_stem);
                if target.exists() {
                    plan.skipped += 1;
                    plan.details.push(format!("跳过（同名已存在）：{wanted_stem}"));
                    continue;
                }
                let from = found
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                plan.details.push(format!("{from}  →  {wanted_stem}"));
                if !dry_run {
                    tokio::fs::rename(&found, &target)
                        .await
                        .map_err(|e| format!("重命名失败 {from}: {e}"))?;
                }
                plan.renamed += 1;
            }
            None => {
                plan.missing += 1;
            }
        }
    }
    Ok(plan)
}

/// 同步版来源判定（重命名入口用：短链无法在线解析时按原文匹配）。
fn parse_probe_target_sync(_client: &WxClient, input: &str) -> Option<SourceTarget> {
    parse_input(input).ok()
}

/// 从输入里取合集名提示（重命名预览用；拿不到给空）。
fn input_collection_hint(input: &str) -> String {
    let _ = input;
    String::new()
}

/// 在输出目录里递归找"去掉数字前缀后与标题相同"的文件夹或文件。
async fn find_downloaded_by_title(
    root: &std::path::Path,
    title: &str,
    wanted: &str,
) -> Option<std::path::PathBuf> {
    let wanted_stem = wanted.to_string();
    let wanted_title = strip_number_prefix(&wanted_stem);
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "index.html" || name == "正文.txt" {
                continue;
            }
            let is_dir = entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                // 条目文件夹本身命中才算（公众号/合集层不参与匹配）
                let bare = strip_number_prefix(&name);
                if bare == wanted_title || bare == title || name == wanted_stem {
                    // 确认是条目文件夹（含 正文.txt 或媒体文件），排除巧合的合集层
                    if path.join("正文.txt").exists() || has_media_file(&path) {
                        return Some(path);
                    }
                }
                stack.push(path);
                continue;
            }
        }
    }
    None
}

fn has_media_file(dir: &std::path::Path) -> bool {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries.flatten().any(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.ends_with(".jpg")
                    || name.ends_with(".png")
                    || name.ends_with(".gif")
                    || name.ends_with(".webp")
                    || name.ends_with(".mp4")
                    || name.ends_with(".mp3")
            })
        })
        .unwrap_or(false)
}

/// 去掉开头的"编号 + 分隔符"：`001 标题` / `12-标题` / `3 标题` 都还原成标题。
fn strip_number_prefix(name: &str) -> String {
    let trimmed = name.trim_start();
    let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return trimmed.trim().to_string();
    }
    trimmed[digits.len()..]
        .trim_start_matches([' ', '-', '_', '.', '、', '·'])
        .trim()
        .to_string()
}

// ───────────────────────── 内容库（本地扫描） ─────────────────────────

#[tauri::command]
pub async fn library_folders(
    state: State<'_, AppState>,
) -> Result<LibraryFolders, String> {
    let settings = state.settings_snapshot();
    let root = settings.output_dir.clone();
    let mut created: Vec<FavItem> = Vec::new();
    let mut id = 0u64;

    // 两层：公众号 / 合集（或年月），合集层统计条目文件夹数与最近时间
    if let Ok(accounts) = std::fs::read_dir(&root) {
        for account in accounts.flatten() {
            if !account.path().is_dir() {
                continue;
            }
            if let Ok(collections) = std::fs::read_dir(account.path()) {
                for collection in collections.flatten() {
                    let cpath = collection.path();
                    if !cpath.is_dir() {
                        continue;
                    }
                    let mut count = 0u32;
                    let mut latest = 0i64;
                    if let Ok(items) = std::fs::read_dir(&cpath) {
                        for item in items.flatten() {
                            if item.path().is_dir() {
                                count += 1;
                                if let Ok(meta) = item.metadata() {
                                    if let Ok(modified) = meta.modified() {
                                        if let Ok(secs) = modified.duration_since(std::time::UNIX_EPOCH) {
                                            latest = latest.max(secs.as_secs() as i64);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if count == 0 {
                        continue;
                    }
                    id += 1;
                    created.push(FavItem {
                        id,
                        bid: String::new(),
                        title: format!(
                            "{} · {}（{} 篇）",
                            account.file_name().to_string_lossy(),
                            collection.file_name().to_string_lossy(),
                            count
                        ),
                        created_at: latest,
                        pics: count,
                        video: false,
                        url: cpath.to_string_lossy().to_string(),
                        kind: "folder".into(),
                    });
                }
            }
        }
    }
    let note = if created.is_empty() {
        "还没有下载记录：解析一篇文章或合集开始使用".to_string()
    } else {
        format!("本地已下载 {} 个合集/分组", created.len())
    };
    Ok(LibraryFolders {
        mid: String::new(),
        created,
        subscribed: Vec::new(),
        fav_note: note,
        follow_note: String::new(),
    })
}

// ───────────────────────── 登录（内嵌网页 · 公众平台） ─────────────────────────

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
    // token 落盘:重启后不必等隐藏窗恢复完就能用(会话过期时由登录检测兜底)
    {
        let settings = state.settings_snapshot();
        let _ = std::fs::write(settings.data_root().join("mp_token.txt"), &token);
    }

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

/// 启动时恢复平台登录：隐藏窗口打开平台首页,已登录会直接落到带 token 的后台页。
async fn restore_mp_login(app: tauri::AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.settings_snapshot();
    let Ok(window) = ensure_work_window(&app, &settings).await else {
        return;
    };
    let url: tauri::Url = "https://mp.weixin.qq.com/".parse().unwrap();
    if window.navigate(url).is_err() {
        return;
    }
    // 已登录的会话会 302 到 /cgi-bin/home?...&token=;未登录停在登录页
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let Some(cur) = window.url().map(|u| u.to_string()).ok() else {
            continue;
        };
        if cur.contains("mp.weixin.qq.com") {
            if let Some(token) = token_from_url(&cur) {
                let cookies = harvest_mp_cookies(&window);
                wx_core::login::debug_log(&format!(
                    "启动恢复平台登录成功(token={}…,Cookie {} 个)",
                    &token[..token.len().min(4)],
                    cookies.len()
                ));
                let _ = finish_mp_login(&app, token, cookies, false).await;
                return;
            }
            if cur.contains("/cgi-bin/") || cur.contains("login") {
                // 停在登录页 = 会话过期,静默收场(界面保持未登录)
                wx_core::login::debug_log("启动恢复平台登录：会话已过期");
                let _ = window.hide();
                return;
            }
        }
    }
    let _ = window.hide();
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
}

// ───────────────────────── 应用更新 ─────────────────────────

/// 更新源仓库：发布 Release 后此处即可在线检测（tag 形如 v0.2.0）。
const UPDATE_REPO: &str = "yungumax/WXdown";

/// 三段式版本号比较：a >= b 返回 true（逐段数值比较，缺段按 0）。
fn version_at_least(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.trim_start_matches('v')
            .split('.')
            .map(|p| p.trim().parse().unwrap_or(0))
            .collect()
    };
    let (va, vb) = (parse(a), parse(b));
    for i in 0..3 {
        let x = va.get(i).copied().unwrap_or(0);
        let y = vb.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    true
}

#[tauri::command]
pub async fn check_updates() -> Result<UpdateCheck, String> {
    let current = APP_VERSION.to_string();
    let url = format!("https://api.github.com/repos/{UPDATE_REPO}/releases/latest");
    let http = reqwest::Client::builder()
        .user_agent(format!("WXdown/{APP_VERSION}"))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(err)?;
    let resp = http
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(err)?;
    let status = resp.status();
    if status.as_u16() == 404 {
        return Ok(UpdateCheck {
            current,
            latest: String::new(),
            up_to_date: true,
            error: format!("仓库 {UPDATE_REPO} 尚未发布 Release"),
        });
    }
    if !status.is_success() {
        return Ok(UpdateCheck {
            current,
            latest: String::new(),
            up_to_date: false,
            error: format!("检测失败 HTTP {status}"),
        });
    }
    let value: serde_json::Value = resp.json().await.map_err(err)?;
    let tag = value
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let latest = tag.trim_start_matches('v').to_string();
    if latest.is_empty() {
        return Ok(UpdateCheck {
            current,
            latest: String::new(),
            up_to_date: true,
            error: "Release 未打版本标签".into(),
        });
    }
    let up_to_date = version_at_least(&current, &latest);
    Ok(UpdateCheck {
        current,
        latest,
        up_to_date,
        error: String::new(),
    })
}

// ───────────────────────── 命名预览 ─────────────────────────

/// 界面日志：前端把关键状态写进同一个调试日志，排查时不用猜界面拿到了什么。
#[tauri::command]
pub fn ui_log(text: String) {
    wx_core::login::debug_log(&format!("[UI] {text}"));
}

#[tauri::command]
pub fn naming_variables() -> Vec<NamingVar> {
    naming::VARIABLES
        .iter()
        .map(|(token, label, section, hint)| NamingVar {
            token: token.to_string(),
            label: label.to_string(),
            section: section.to_string(),
            hint: hint.to_string(),
        })
        .collect()
}

fn preview_vars(date: &str, publish_date: &str, ext: &str) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    vars.insert("title".into(), "示例文章标题".into());
    vars.insert("author".into(), "示例公众号".into());
    vars.insert("collection".into(), "示例合集".into());
    vars.insert("bid".into(), "f553dd20b542".into());
    vars.insert("uid".into(), "MzYzOTU1NTUzNw==".into());
    vars.insert("date".into(), date.to_string());
    vars.insert("publish_date".into(), publish_date.to_string());
    let (year, month) = publish_date
        .split_once('-')
        .map(|(y, rest)| (y.to_string(), rest.split('-').next().unwrap_or("01").to_string()))
        .unwrap_or((date[..4.min(date.len())].to_string(), "01".to_string()));
    vars.insert("year".into(), year);
    vars.insert("month".into(), month);
    vars.insert("source_kind".into(), "合集".into());
    vars.insert("index".into(), "07".into());
    vars.insert("ext".into(), ext.to_string());
    vars
}

#[tauri::command]
pub fn preview_naming(
    template: String,
    date: Option<String>,
    publish_date: Option<String>,
    ext: Option<String>,
    dir: Option<bool>,
) -> Result<String, String> {
    let today = {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let (y, m, d) = civil_from_unix(now + 8 * 3600);
        format!("{y}-{m:02}-{d:02}")
    };
    let vars = preview_vars(
        &date.unwrap_or_else(|| today.clone()),
        &publish_date.unwrap_or_else(|| "2026-10-01".into()),
        &ext.unwrap_or_else(|| if dir.unwrap_or(false) { String::new() } else { "txt".into() }),
    );
    Ok(naming::render(&template, &vars, dir.unwrap_or(false)))
}

#[tauri::command]
pub fn preview_names(
    items: Vec<serde_json::Value>,
    ext: Option<String>,
) -> Result<Vec<String>, String> {
    let today = {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let (y, m, d) = civil_from_unix(now + 8 * 3600);
        format!("{y}-{m:02}-{d:02}")
    };
    let mut out = Vec::new();
    for item in items {
        let mut vars = preview_vars(&today, &publish_of(&item), &ext.clone().unwrap_or_default());
        if let Some(title) = item.get("title").and_then(|v| v.as_str()) {
            vars.insert("title".into(), naming::title_of(title));
        }
        if let Some(naming) = item.get("naming").and_then(|v| v.as_object()) {
            for (key, value) in naming {
                if let Some(text) = value.as_str() {
                    if !text.is_empty() {
                        vars.insert(key.clone(), text.to_string());
                    }
                } else if let Some(num) = value.as_u64() {
                    if *key == "index" {
                        let pad = item
                            .pointer("/naming/index_pad")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(2) as usize;
                        vars.insert("index".into(), naming::padded_index(num as u32, pad));
                    } else {
                        vars.insert(key.clone(), num.to_string());
                    }
                }
            }
        }
        out.push(naming::render(
            &crate::state::Settings::load().naming_template,
            &vars,
            true,
        ));
    }
    Ok(out)
}

fn publish_of(item: &serde_json::Value) -> String {
    item.pointer("/naming/publish_date")
        .and_then(|v| v.as_str())
        .unwrap_or("2026-10-01")
        .to_string()
}

// ───────────────────────── 系统交互与维护 ─────────────────────────

#[tauri::command]
pub async fn choose_output_dir(app: tauri::AppHandle) -> Result<String, String> {
    let folder = app
        .dialog()
        .file()
        .blocking_pick_folder()
        .map(|path| path.to_string());
    match folder {
        Some(path) => Ok(path),
        // 用户取消不报错：返回当前设置的原值由前端兜底
        None => Err("已取消选择".into()),
    }
}

#[tauri::command]
pub fn open_path(path: String) -> Result<(), String> {
    // 规范化：统一反斜杠 + 解析成绝对路径（混入的正斜杠会让资源管理器行为异常）
    let mut target = std::fs::canonicalize(&path).unwrap_or_else(|_| std::path::PathBuf::from(&path));
    // canonicalize 会加 Win32 命名空间前缀 \\?\：剥掉它，普通路径形式对 explorer 更友好
    let s = target.to_string_lossy().to_string();
    if let Some(stripped) = s.strip_prefix(r"\\?\UNC\") {
        target = PathBuf::from(format!(r"\\{stripped}"));
    } else if let Some(stripped) = s.strip_prefix(r"\\?\") {
        target = PathBuf::from(stripped.to_string());
    }
    wx_core::login::debug_log(&format!("open_path：{} → {}", path, target.display()));
    if !target.exists() {
        return Err(format!("路径不存在：{}", target.display()));
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const NO_WINDOW: u32 = 0x0800_0000;
        if target.is_dir() {
            // 目录：资源管理器打开目录
            std::process::Command::new("explorer.exe")
                .arg(&target)
                .creation_flags(NO_WINDOW)
                .spawn()
                .map_err(err)?;
        } else {
            // 文件：系统默认应用打开
            std::process::Command::new("cmd")
                .args(["/C", "start", "", &target.to_string_lossy()])
                .creation_flags(NO_WINDOW)
                .spawn()
                .map_err(err)?;
        }
        return Ok(());
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("xdg-open").arg(target).spawn().map_err(err)?;
        Ok(())
    }
}

/// 资源管理器定位：打开文件所在文件夹并选中该文件（目录则直接打开目录）。
#[tauri::command]
pub fn reveal_path(path: String) -> Result<(), String> {
    // 规范化并剥掉 canonicalize 加的 \\?\ 前缀：
    // 不剥的话资源管理器解析不了（回退打开"此电脑"，实测踩过）
    let mut target = std::fs::canonicalize(&path)
        .unwrap_or_else(|_| std::path::PathBuf::from(&path));
    let s = target.to_string_lossy().to_string();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        target = std::path::PathBuf::from(format!(r"\\{rest}"));
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        target = std::path::PathBuf::from(rest.to_string());
    }
    wx_core::login::debug_log(&format!(
        "reveal_path：{} → {}",
        path,
        target.display()
    ));
    if !target.exists() {
        return Err(format!("路径不存在：{}", target.display()));
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        if target.is_dir() {
            std::process::Command::new("explorer.exe")
                .arg(&target)
                .creation_flags(0x08000000)
                .spawn()
                .map_err(err)?;
        } else {
            std::process::Command::new("explorer.exe")
                .args(["/select,", &target.to_string_lossy()])
                .creation_flags(0x08000000)
                .spawn()
                .map_err(err)?;
        }
        return Ok(());
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("xdg-open")
            .arg(target.parent().unwrap_or(&target))
            .spawn()
            .map_err(err)?;
        Ok(())
    }
}

#[tauri::command]
pub fn resume_pending() -> Result<u32, String> {
    // 公众号媒体是小文件直下，任务不落盘跨会话续传；保留接口与前端对齐。
    Ok(0)
}

#[tauri::command]
pub fn cleanup_temp(state: State<'_, AppState>) -> Result<u32, String> {
    let settings = state.settings_snapshot();
    let mut removed = 0;
    if let Ok(entries) = std::fs::read_dir(&settings.output_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(".wxtmp") {
                if std::fs::remove_dir_all(entry.path()).is_ok() {
                    removed += 1;
                }
            }
        }
    }
    // .part 残片（暂停后未继续的半截文件）
    if let Ok(entries) = std::fs::read_dir(&settings.output_dir) {
        let mut stack: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        while let Some(dir) = stack.pop() {
            if let Ok(children) = std::fs::read_dir(&dir) {
                for child in children.flatten() {
                    let path = child.path();
                    if path.is_dir() {
                        stack.push(path);
                    } else if path
                        .extension()
                        .map(|e| e.to_string_lossy() == "part")
                        .unwrap_or(false)
                    {
                        if std::fs::remove_file(&path).is_ok() {
                            removed += 1;
                        }
                    }
                }
            }
        }
    }
    Ok(removed)
}

#[tauri::command]
pub fn export_diagnostics(state: State<'_, AppState>) -> Result<String, String> {
    let settings = state.settings_snapshot();
    let dir = settings.data_root();
    std::fs::create_dir_all(&dir).map_err(err)?;
    let cookies = state.client().export_cookies();
    let names: Vec<&str> = cookies.iter().map(|(n, _)| n.as_str()).collect();
    let text = format!(
        "WXdown v{}\n时间：{}\n\n设置：\n{}\n\n会话 Cookie 名称：{:?}\n",
        APP_VERSION,
        {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let (y, m, d) = civil_from_unix(now + 8 * 3600);
            format!("{y}-{m:02}-{d:02}")
        },
        serde_json::to_string_pretty(&settings).unwrap_or_default(),
        names,
    );
    let path = dir.join("diagnostics.txt");
    std::fs::write(&path, text).map_err(err)?;
    Ok(path.to_string_lossy().to_string())
}

// ───────────────────────── 日期工具 ─────────────────────────

/// Unix 秒 → （年, 月, 日）（东八区口径由调用方加好偏移）。
fn civil_from_unix(secs: i64) -> (i64, u32, u32) {
    let days = secs.div_euclid(86400);
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_number_prefix_shapes() {
        assert_eq!(strip_number_prefix("001 标题"), "标题");
        assert_eq!(strip_number_prefix("12-标题"), "标题");
        assert_eq!(strip_number_prefix("3 标题"), "标题");
        assert_eq!(strip_number_prefix("标题"), "标题");
    }

    #[test]
    fn archive_rewrites_img_src() {
        let raw = r#"<div id="js_content" style="visibility: hidden;"><p>hi</p><img class="rich_pages" data-src="https://mmbiz.qpic.cn/a/640?wx_fmt=jpeg" data-type="jpeg"/></div>"#;
        let out = build_archive_html(
            raw,
            &[("https://mmbiz.qpic.cn/a/640?wx_fmt=jpeg".into(), "01.jpg".into())],
            &[],
            &[],
        );
        assert!(out.contains(r#"src="01.jpg""#), "图片 src 应指向本地：{out}");
        assert!(out.contains("visibility: visible"), "正文隐藏应被解除");
        assert!(!out.contains("<script"), "script 应被剥掉");
    }

    #[test]
    fn archive_video_placeholder_and_local() {
        let raw = r#"<iframe class="video_iframe rich_pages" data-src="https://v.qq.com/iframe/preview.html?vid=x1"></iframe><iframe class="video_iframe" data-src="https://mp.weixin.qq.com/mp/readtemplate?vid=wxv_1"></iframe>"#;
        let out = build_archive_html(
            raw,
            &[],
            &[
                ("https://v.qq.com/iframe/preview.html?vid=x1".into(), None),
                (
                    "https://mp.weixin.qq.com/mp/readtemplate?vid=wxv_1".into(),
                    Some("video-02.mp4".into()),
                ),
            ],
            &[],
        );
        assert!(out.contains("未能解析直链"), "未解析视频给占位：{out}");
        assert!(out.contains(r#"src="video-02.mp4""#), "解析成功的视频给本地标签");
    }

    #[test]
    fn date_to_unix_roundtrip() {
        let ts = date_to_unix("2026-01-02").unwrap();
        assert_eq!(ts, wx_core::article::parse_datetime_local("2026-01-02 00:00"));
    }
}
