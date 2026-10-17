//! 应用运行期状态：登录会话、任务表、可持久化的设置、批量来源缓存。

use crate::types::{LoginInfo, PostEntry, TaskUpdate};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};
use tokio::sync::Semaphore;
use wx_core::WxClient;

pub const DEFAULT_MAX_CONCURRENT_TASKS: usize = 2;
/// 文件夹层级的默认模板：作者 → 年月。
pub const DEFAULT_FOLDER_TEMPLATE: &str = "{author}/{collection}";

pub struct TaskEntry {
    pub snapshot: Arc<Mutex<TaskUpdate>>,
    pub abort: Option<tokio::task::AbortHandle>,
    /// 原始下载请求：暂停后「继续」需要重跑流水线
    pub req: crate::types::DownloadReq,
    /// 暂停开关：置 true 后下载循环停在下一个分块（进度留在 .part）
    pub pause: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

/// 批量来源（用户主页）的增量加载缓存：
/// 小红书网页端只内嵌最新一批笔记（无翻页接口），缓存主要供重命名用。
#[derive(Debug, Clone)]
pub struct BatchCache {
    pub uid: String,
    /// 合集 ID（继续解析翻页用；uid 是 __biz）
    pub album_id: String,
    /// 来源类型：album（合集/文章展开）/ account（公众号全量，album_id 为空按偏移翻页）
    pub kind: String,
    /// 来源总数（合集 article_count / 公众号发布批次数；展示用）
    pub total: usize,
    /// 公众号枚举的下一页 begin 偏移（按发布批次计，每页 5 批；合集不用）
    pub cursor: u32,
    pub items: Vec<PostEntry>,
    pub exhausted: bool,
    /// 来源页面地址：继续解析时要回到这一页滚动加载
    pub url: String,
    /// 连续"条数没增长"的轮数：够 3 轮就认为拉到底了
    pub stale: u32,
}

/// 一条命名模板预设。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]

pub struct NamingPreset {
    pub name: String,
    pub template: String,
}

/// 用户可配置项，全部持久化到 settings.json。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub output_dir: PathBuf,
    /// 文件夹层级模板：用 `/` 分层，为空表示不建层级。
    pub folder_template: String,
    pub max_concurrent_tasks: usize,
    /// 兼容字段：小红书媒体是小文件，分片参数不参与实际行为
    pub chunk_concurrency: usize,
    pub chunk_mb: u64,
    pub keep_temp: bool,
    pub naming_template: String,
    pub naming_presets: Vec<NamingPreset>,
    pub folder_presets: Vec<NamingPreset>,
    /// skip / overwrite / auto
    pub rename_conflict: String,
    /// large（原图）/ thumbnail（压缩图）
    pub image_format: String,
    /// auto / hd / sd
    pub video_quality: String,
    /// best / standard（保留兼容：小红书暂无纯音频内容）
    pub audio_quality: String,
    pub download_text: bool,
    pub retry_count: u32,
    pub speed_limit_mib: u32,
    pub resume_on_start: bool,
    pub parse_preset: String,
    pub parse_batch: u32,
    pub parse_batch_wait_ms: u32,
    pub parse_rest_every: u32,
    pub parse_rest_ms: u32,
    pub log_level: String,
    pub data_dir: String,
    /// 启动时静默检查更新（不自动下载或安装）
    pub update_check: bool,
    /// 首启引导是否已完成（false 时启动显示引导页）
    pub onboarded: bool,
    pub proxy: String,
    /// light / dark / system
    pub theme: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            output_dir: default_data_root().join("downloads"),
            folder_template: DEFAULT_FOLDER_TEMPLATE.to_string(),
            max_concurrent_tasks: DEFAULT_MAX_CONCURRENT_TASKS,
            chunk_concurrency: 4,
            chunk_mb: 4,
            keep_temp: false,
            naming_template: "{index} {title}.{ext}".to_string(),
            naming_presets: Vec::new(),
            folder_presets: Vec::new(),
            rename_conflict: "skip".to_string(),
            image_format: "large".to_string(),
            video_quality: "auto".to_string(),
            audio_quality: "best".to_string(),
            download_text: true,
            retry_count: 3,
            speed_limit_mib: 0,
            resume_on_start: false,
            parse_preset: "标准".to_string(),
            parse_batch: 8,
            parse_batch_wait_ms: 1000,
            parse_rest_every: 100,
            parse_rest_ms: 3000,
            log_level: "info".to_string(),
            data_dir: String::new(),
            update_check: false,
            onboarded: false,
            proxy: String::new(),
            theme: "system".to_string(),
        }
    }
}

/// 默认数据目录：%APPDATA%\wxdown；拿不到 APPDATA 时退到 exe 旁的 _data。
pub fn default_data_root() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        if !appdata.is_empty() {
            return PathBuf::from(appdata).join("wxdown");
        }
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("_data").join("wxdown")))
        .unwrap_or_else(|| PathBuf::from("_data").join("wxdown"))
}

impl Settings {
    /// 数据根目录：设置了 data_dir 用它，否则默认目录。
    pub fn data_root(&self) -> PathBuf {
        let trimmed = self.data_dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
        default_data_root()
    }

    pub fn cookies_path(&self) -> PathBuf {
        self.data_root().join("cookies.json")
    }

    pub fn settings_path(&self) -> PathBuf {
        self.data_root().join("settings.json")
    }

    pub fn load() -> Self {
        // 先读默认目录的设置（data_dir 本身也存那里）
        let mut settings: Settings = std::fs::read_to_string(default_data_root().join("settings.json"))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        settings.clamp();
        settings
    }

    pub fn save(&self) -> std::io::Result<()> {
        let dir = self.data_root();
        std::fs::create_dir_all(&dir)?;
        std::fs::write(self.settings_path(), serde_json::to_string_pretty(self).unwrap())?;
        Ok(())
    }

    /// 越界值拉回合法区间（老配置或手改 settings.json 后兜底）。
    pub fn clamp(&mut self) {
        self.max_concurrent_tasks = self.max_concurrent_tasks.clamp(1, 5);
        self.retry_count = self.retry_count.min(10);
        if !matches!(self.image_format.as_str(), "large" | "thumbnail") {
            self.image_format = "large".to_string();
        }
        if !matches!(self.video_quality.as_str(), "auto" | "hd" | "sd") {
            self.video_quality = "auto".to_string();
        }
        if !matches!(self.audio_quality.as_str(), "best" | "standard") {
            self.audio_quality = "best".to_string();
        }
        if !matches!(self.rename_conflict.as_str(), "skip" | "overwrite" | "auto") {
            self.rename_conflict = "skip".to_string();
        }
        if !matches!(self.theme.as_str(), "light" | "dark" | "system") {
            self.theme = "system".to_string();
        }
        if self.parse_batch == 0 {
            self.parse_batch = 8;
        }
        if self.naming_template.trim().is_empty() {
            self.naming_template = "{index} {title}.{ext}".to_string();
        }
        // 旧默认（无序号）等价升迁为新的内置默认【批量带序号】：
        // 渲染上单条 index=1、批量由旧到新，老用户打开即得到新默认行为
        if self.naming_template == "{title}.{ext}" {
            self.naming_template = "{index} {title}.{ext}".to_string();
        }
    }
}

pub struct AppState {
    /// 用 RwLock 包一层，改代理或登出时可以整体换成新的会话
    pub client: RwLock<Arc<WxClient>>,
    pub tasks: Mutex<HashMap<String, TaskEntry>>,
    pub settings: Mutex<Settings>,
    /// 并发下载槽位，会随设置变化重建
    pub slots: RwLock<Arc<Semaphore>>,
    pub counter: Mutex<u64>,
    /// 批量来源的缓存，按来源输入索引
    pub batches: Mutex<HashMap<String, BatchCache>>,
    /// 会话里是否已经热过身（访问过首页、拿到访客 Cookie）
    pub warmed: AtomicBool,
    /// 缓存的登录态（None = 未登录）
    pub login: RwLock<Option<LoginInfo>>,
    /// 浏览器取数窗口只有一个，取数必须串行（同窗导航会互相打断）
    pub fetch_lock: tokio::sync::Mutex<()>,
    /// 笔记详情的风控熔断器：(最近一次被挡的时间, 连续被挡次数)。
    /// 被站点定向限流时逐条硬试只会越撞越黑：连挡两条就停手并给用户明确提示。
    pub note_block: Mutex<(std::time::Instant, u32)>,
    /// 上一次笔记取数的时间：批量下载按最小间隔降速（猛拉详情最容易触发风控）
    pub last_note_fetch: Mutex<Option<std::time::Instant>>,
    /// WEB 窗口"用户最后在看的页面"（窗口可见时由状态轮询持续跟踪）。
    /// 再点 WEB 时恢复它，而不是每次都跳回发现页；空串=本会话还没看过（去发现页）。
    /// 只存内存：软件重启后第一次点 WEB 回发现页。
    pub last_web_url: Mutex<String>,
    /// 登录窗刚打开时的浏览器 Cookie 指纹：轮询时先比指纹，
    /// 没变化就不去请求主页（省请求、也少触发风控）
    pub login_cookie_baseline: Mutex<Option<String>>,
    /// 上一次做权威登录检测的时间
    pub last_login_check: Mutex<Option<std::time::Instant>>,
    /// 后端登录轮询是否在跑（同一时刻只允许一个）
    pub login_watch_running: AtomicBool,
    /// 登录窗口是否正开着（开着时取数不许抢窗口）
    pub login_window_open: AtomicBool,
    /// 公众平台会话 token（登录后从后台首页 URL 里抠出；随会话变化）
    pub mp_token: Mutex<String>,
}

/// 浏览器数据目录：登录窗口与取数窗口共用同一个目录，
/// 登录态因此在两个窗口之间共享、也能跨应用重启保留。
pub fn webview_dir(settings: &Settings) -> PathBuf {
    settings.data_root().join("webview")
}

/// 退出登录时若目录被 WebView2 占用删不掉，留这个标记，下次启动先清。
pub fn webview_logout_marker(settings: &Settings) -> PathBuf {
    settings.data_root().join("webview.logout")
}

impl AppState {
    pub fn new() -> Result<Self, String> {
        let settings = Settings::load();

        // 上次退出登录时浏览器目录被占用没删掉：这里先清，避免"登出后又自动登录"
        let marker = webview_logout_marker(&settings);
        if marker.exists() {
            let _ = std::fs::remove_dir_all(webview_dir(&settings));
            let _ = std::fs::remove_file(&marker);
        }

        let client = WxClient::with_proxy(Some(&settings.proxy)).map_err(|e| e.to_string())?;

        // 注意：**不把 cookies.json 装到 reqwest 客户端**。
        // 小红书风控会把"同一会话来自另一个客户端"判成劫持并作废会话，
        // 连带杀掉浏览器里的登录态（实测：登录后 9 秒内会话失效）。
        // 登录态只活在浏览器窗口里，reqwest 只做匿名请求与 CDN 下载。

        let slots = Arc::new(Semaphore::new(settings.max_concurrent_tasks.max(1)));
        Ok(Self {
            client: RwLock::new(Arc::new(client)),
            tasks: Mutex::new(HashMap::new()),
            settings: Mutex::new(settings),
            slots: RwLock::new(slots),
            counter: Mutex::new(0),
            batches: Mutex::new(HashMap::new()),
            warmed: AtomicBool::new(false),
            login: RwLock::new(None),
            fetch_lock: tokio::sync::Mutex::new(()),
            // 初值：很久以前、0 次（elapsed 超过冷却期 ⇒ 不拦）
            note_block: Mutex::new((
                std::time::Instant::now() - std::time::Duration::from_secs(3600),
                0,
            )),
            last_note_fetch: Mutex::new(None),
            last_web_url: Mutex::new(String::new()),
            login_cookie_baseline: Mutex::new(None),
            last_login_check: Mutex::new(None),
            login_watch_running: AtomicBool::new(false),
            login_window_open: AtomicBool::new(false),
            mp_token: Mutex::new(String::new()),
        })
    }

    pub fn client(&self) -> Arc<WxClient> {
        self.client.read().unwrap().clone()
    }

    /// 启动期读设置用（与 settings_snapshot 同一实现，语义上更短）。
    pub fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    pub fn settings_snapshot(&self) -> Settings {
        self.settings()
    }

    pub fn slots(&self) -> Arc<Semaphore> {
        self.slots.read().unwrap().clone()
    }

    pub fn rebuild_slots(&self) {
        let max = self.settings_snapshot().max_concurrent_tasks.max(1);
        *self.slots.write().unwrap() = Arc::new(Semaphore::new(max));
    }

    /// 持久化当前会话 Cookie（网页登录收割后调用）。
    pub fn save_cookies(&self) {
        let settings = self.settings_snapshot();
        let cookies = wx_core::client::DomainCookies {
            web: self.client().export_cookies(),
        };
        let _ = std::fs::create_dir_all(settings.data_root());
        if let Ok(text) = serde_json::to_string_pretty(&cookies) {
            let _ = std::fs::write(settings.cookies_path(), text);
        }
    }

    pub fn cookies_saved(&self) -> bool {
        self.settings_snapshot().cookies_path().exists()
    }

    pub fn next_task_id(&self) -> String {
        let mut counter = self.counter.lock().unwrap();
        *counter += 1;
        format!("task-{counter}")
    }

    /// 缓存条目上限：超了丢最早的来源，防止长期占用。
    pub fn cache_batch(&self, input: String, cache: BatchCache) {
        let mut batches = self.batches.lock().unwrap();
        if batches.len() >= 8 && !batches.contains_key(&input) {
            if let Some(oldest) = batches.keys().next().cloned() {
                batches.remove(&oldest);
            }
        }
        batches.insert(input, cache);
    }
}
