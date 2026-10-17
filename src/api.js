// 与 Rust 后端通信的唯一入口。
//
// 在浏览器里直接打开时（没有 Tauri 运行时）自动切换到假数据，
// 这样界面可以脱离桌面壳单独预览与调整。

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Tauri 框架层的报错是英文（如 invalid args / invalid type），
// 统一翻译成中文并保留原文，避免用户看到无从下手的提示。
function zhError(error) {
  const msg = String(error);
  if (/invalid args .* for command/i.test(msg)) {
    return new Error(`内部请求参数异常（程序缺陷，请反馈此场景）：${msg}`);
  }
  if (/invalid type/i.test(msg) && /expected/i.test(msg)) {
    return new Error(`参数类型不匹配（程序缺陷，请反馈此场景）：${msg}`);
  }
  return error;
}

async function invokeZh(command, args) {
  try {
    return await invoke(command, args);
  } catch (error) {
    throw zhError(error);
  }
}

export const hasTauri =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

const TASK_EVENT = "task://update";

export async function appStatus() {
  if (!hasTauri) return mock.status();
  return invokeZh("app_status");
}

export async function appSettings() {
  if (!hasTauri) return mock.settings();
  return invokeZh("app_settings");
}

export async function updateSettings(settings) {
  if (!hasTauri) return mock.updateSettings(settings);
  return invokeZh("update_settings", { settings });
}

// 解析来源：文章链接 → article；合集链接 → album
export async function probeSource(input) {
  if (!hasTauri) return mock.probe(input);
  return invokeZh("probe_source", { input });
}

// 继续解析：合集翻下一页（getalbum 分页）
export async function probeMore(input, want) {
  if (!hasTauri) return mock.probeMore(input, want);
  return invokeZh("probe_more", { input, want });
}

export async function startDownload(req) {
  if (!hasTauri) return mock.start(req);
  return invokeZh("start_download", { req });
}

export async function pauseDownload(taskId) {
  if (!hasTauri) return;
  return invokeZh("pause_download", { taskId });
}

export async function resumeDownload(taskId) {
  if (!hasTauri) return;
  return invokeZh("resume_download", { taskId });
}

export async function pauseAllDownloads() {
  if (!hasTauri) return;
  return invokeZh("pause_all_downloads");
}

export async function resumeAllDownloads() {
  if (!hasTauri) return;
  return invokeZh("resume_all_downloads");
}

export async function renameDownloaded(input, total, dryRun) {
  if (!hasTauri) return { renamed: 0, skipped: 0, missing: 0, details: [], dryRun: true };
  return invokeZh("rename_downloaded", { input, total, dryRun });
}

export async function cancelDownload(taskId) {
  if (!hasTauri) return mock.cancel(taskId);
  return invokeZh("cancel_download", { taskId });
}

export async function logout() {
  if (!hasTauri) return emptyLogin();
  return invokeZh("logout");
}

export async function chooseOutputDir() {
  if (!hasTauri) return mock.status().output_dir;
  return invokeZh("choose_output_dir");
}

// 「魔法变量」清单由后端提供，界面不再自己写一份——否则界面会列出后端不支持的变量。
export async function namingVariables() {
  if (!hasTauri) return mock.namingVariables();
  return invokeZh("naming_variables");
}

// 批量文件名预览：给每条内容算出文件名，与真实落盘共用同一个渲染器。
export async function previewNames(items, ext) {
  if (!hasTauri) return items.map((item) => `${item.title}.${ext || "txt"}`);
  return invokeZh("preview_names", { items, ext });
}

// 文件名预览走后端同一个渲染器，预览与真实落盘不会不一致。
export async function previewNaming(template, { date, publish_date, ext, dir } = {}) {
  if (!hasTauri) return mock.previewNaming(template, ext);
  return invokeZh("preview_naming", { template, date, publish_date, ext, dir });
}

/// 启动续传：公众号媒体是小文件直下，任务不跨会话续传（接口保留对齐）。
export async function resumePending() {
  if (!hasTauri) return mock.resumePending();
  return invokeZh("resume_pending");
}

/// 内容库：读账号里的「我的笔记」
export async function libraryFolders() {
  if (!hasTauri) return mock.libraryFolders();
  return invokeZh("library_folders");
}

export async function cleanupTemp() {
  if (!hasTauri) return 0;
  return invokeZh("cleanup_temp");
}

// 更新检测（优先 updater 插件：带签名校验，返回 Update 对象可下载安装；
// 插件不可用返回 null 由调用方回退 GitHub API 只读检测）
export async function updateCheckPlugin() {
  if (!hasTauri) return null;
  const { check } = await import("@tauri-apps/plugin-updater");
  return check();
}

// 下载并安装更新（onProgress 收字节流进度）
export async function updateDownloadAndInstall(update, onProgress) {
  let received = 0;
  let total = 0;
  await update.downloadAndInstall((event) => {
    switch (event.event) {
      case "Started":
        total = event.data.contentLength || 0;
        break;
      case "Progress":
        received += event.data.chunkLength;
        break;
      case "Finished":
        break;
    }
    if (onProgress) onProgress({ received, total });
  });
}

// 安装完成后重启应用
export async function relaunchApp() {
  const { relaunch } = await import("@tauri-apps/plugin-process");
  return relaunch();
}

// 更新检测：后端查 GitHub Releases 最新 tag 并与当前版本比较（只读回退路径）
export async function checkUpdates() {
  if (!hasTauri) return { current: "0.1.0", latest: "", up_to_date: true, error: "" };
  return invokeZh("check_updates");
}

export async function exportDiagnostics() {
  if (!hasTauri) return "";
  return invokeZh("export_diagnostics");
}

/** 公众平台登录（阶段二提供）：打开内嵌官方登录页子窗口 */
export async function webLoginOpen() {
  if (!hasTauri) throw new Error("浏览器预览不支持网页登录");
  return invokeZh("web_login_open");
}

/** 网页登录：收割登录 Cookie，返回 LoginInfo（未登录时抛错） */
export async function webLoginCookies() {
  if (!hasTauri) throw new Error("未登录");
  return invokeZh("web_login_cookies");
}

/** 界面日志：把关键状态写进后端调试日志（排查用） */
export async function uiLog(text) {
  if (!hasTauri) return;
  try {
    await invokeZh("ui_log", { text });
  } catch {
    // 日志失败不影响功能
  }
}

/** 网页登录：让后端开始轮询登录状态（后端定时器不受窗口遮挡降频影响） */
export async function webLoginWatch() {
  if (!hasTauri) return;
  return invokeZh("web_login_watch");
}

/** 登录成功事件（后端检测到登录后推送 LoginInfo） */
export async function onLoginConfirmed(handler) {
  if (!hasTauri) return () => {};
  return listen("login://confirmed", (event) => handler(event.payload));
}

/** 网页工具栏：当前网页地址与可见状态 */
export async function webBarState() {
  if (!hasTauri) return { url: "", visible: false };
  return invokeZh("web_bar_state");
}

/** 网页工具栏：跳转 / 后退前进刷新 / 收起 */
export async function webBarGo(url) {
  if (!hasTauri) return;
  return invokeZh("web_bar_go", { url });
}

export async function webBarCmd(cmd) {
  if (!hasTauri) return;
  return invokeZh("web_bar_cmd", { cmd });
}

export async function webBarHide() {
  if (!hasTauri) return;
  return invokeZh("web_bar_hide");
}

/** 网页内收集：进度（条数） */
export async function onCollectUpdate(handler) {
  if (!hasTauri) return () => {};
  return listen("collect://update", (event) => handler(event.payload?.count || 0));
}

/** 网页内收集：完成（整份列表 + 主页自报总数 + 页面状态去重数 + 统一措辞的完成提示） */
export async function onCollectDone(handler) {
  if (!hasTauri) return () => {};
  return listen("collect://done", (event) =>
    handler(
      event.payload?.items || [],
      !!event.payload?.auto,
      Number(event.payload?.total) || 0,
      Number(event.payload?.state_uniq) || 0,
      String(event.payload?.message || ""),
    ),
  );
}

/** WEB 地址栏点「解析」：把当前网页地址送到解析页 */
export async function onWebToParse(handler) {
  if (!hasTauri) return () => {};
  return listen("web://to-parse", (event) =>
    handler(String(event.payload?.url || ""), !!event.payload?.fill),
  );
}

/** 打开网页窗口：登录了就开当前账号主页，没登录就开推荐流 */
export async function openWebHome() {
  if (!hasTauri) return;
  return invokeZh("open_web_home");
}

/** 网页登录：关闭内嵌窗口 */
export async function webLoginClose() {
  if (!hasTauri) return;
  return invokeZh("web_login_close").catch(() => {});
}

export async function openPath(path) {
  if (!hasTauri) return;
  return invokeZh("open_path", { path });
}

// 打开文件所在文件夹并选中该文件（资源管理器定位）
export async function revealPath(path) {
  if (!hasTauri) return;
  return invokeZh("reveal_path", { path });
}

export async function onTaskUpdate(handler) {
  if (!hasTauri) return mock.onUpdate(handler);
  return listen(TASK_EVENT, (event) => handler(event.payload));
}

export async function readClipboard() {
  if (!hasTauri) return "";
  try {
    return await navigator.clipboard.readText();
  } catch {
    return "";
  }
}


// ---- 窗口控制（自定义标题栏用）----

async function windowApi() {
  if (!hasTauri) return null;
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  return getCurrentWindow();
}

export async function minimizeWindow() {
  const win = await windowApi();
  await win?.minimize();
}

export async function toggleMaximizeWindow() {
  const win = await windowApi();
  await win?.toggleMaximize();
}

export async function closeWindow() {
  const win = await windowApi();
  await win?.close();
}

export async function setWindowBackground(color) {
  if (!hasTauri) return;
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().setBackgroundColor(color);
  } catch {
    // 旧运行时不支持时忽略；页面不透明，底色仅影响边缘
  }
}

export async function startWindowDrag() {
  const win = await windowApi();
  await win?.startDragging();
}

function emptyLogin() {
  return { logged_in: false, uname: "", face: "", mid: "", vip: false, vip_label: "" };
}

// 仅浏览器预览用的假数据（公众号口径）
const mock = (() => {
  const listeners = new Set();
  const timers = new Map();

  const status = () => ({
    version: "0.1.0",
    login: {
      logged_in: false,
      uname: "",
      face: "",
      mid: "",
      vip: false,
      vip_label: "",
    },
    output_dir: "D:\\Zcode\\_data\\wxdown\\downloads",
    cookies_path: "D:\\Zcode\\_data\\wxdown\\cookies.json",
  });

  const settings = () => ({
    settings: {
      output_dir: "D:\\Zcode\\_data\\wxdown\\downloads",
      max_concurrent_tasks: 2,
      chunk_concurrency: 4,
      chunk_mb: 4,
      keep_temp: false,
      naming_template: "{index} {title}.{ext}",
      naming_presets: [
        { name: "示例：带公众号", template: "{author} - {title}.{ext}" },
      ],
      folder_template: "{author}/{collection}",
      folder_presets: [
        { name: "示例：只按公众号", template: "{author}" },
      ],
      rename_conflict: "skip",
      image_format: "large",
      video_quality: "auto",
      audio_quality: "best",
      download_text: true,
      retry_count: 3,
      speed_limit_mib: 0,
      resume_on_start: false,
      parse_preset: "标准",
      parse_batch: 8,
      parse_batch_wait_ms: 1000,
      parse_rest_every: 100,
      parse_rest_ms: 3000,
      log_level: "info",
      data_dir: "",
      update_check: false,
      onboarded: true,
      proxy: "",
      theme: "system",
    },
    cookies_path: "D:\\Zcode\\_data\\wxdown\\cookies.json",
    cookies_saved: false,
    version: "0.1.0",
  });

  const updateSettings = async (next) => {
    const base = settings();
    return { ...base, settings: { ...base.settings, ...next } };
  };

  const qualities = [
    { value: "auto", label: "最佳可用", available: true, hint: "" },
  ];

  function makeArticle(id, title, account, created, pics, video) {
    return {
      bid: id,
      mid: "",
      title,
      author: account,
      created_at: created,
      pics,
      has_video: video,
      duration: video ? 62 : 0,
      is_retweet: false,
      media_unknown: false,
    };
  }

  const ALL_ARTICLES = Array.from({ length: 33 }, (_, i) =>
    makeArticle(
      `2247484${(750 - i).toString()}`,
      i % 3 === 0 ? `示例文章标题 第${i}篇` : i % 3 === 1 ? `教程：如何做某事（${i}）` : `周报 ${i}`,
      "示例公众号",
      1766500000 - i * 86400,
      i % 4 === 3 ? 0 : (i % 5) + 1,
      i % 6 === 0
    )
  );

  const probe = async (input) => {
    const lower = (input || "").toLowerCase();
    if (lower.includes("appmsgalbum")) {
      return {
        kind: "album",
        bid: "4319761724883959819",
        mid: "",
        title: "示例合集",
        author: "示例公众号",
        uid: "",
        cover: "",
        note: "日常一些零碎想法、看法的随手记录（共 33 篇）",
        total: 33,
        loaded: 20,
        exhausted: false,
        qualities: [],
        items: ALL_ARTICLES.slice(0, 20),
      };
    }
    return {
      kind: "article",
      bid: "f553dd20b542",
      mid: "",
      title: "示例单篇文章（含 3 图 1 视频）",
      author: "示例公众号",
      uid: "",
      cover: "",
      note: "正文 3062 字 · 8 图 · 0 视频 · 0 语音",
      total: 1,
      loaded: 1,
      exhausted: true,
      qualities,
      items: [makeArticle("f553dd20b542", "示例单篇文章（含 3 图 1 视频）", "示例公众号", 1766500000, 3, true)],
    };
  };

  function emit(task) {
    listeners.forEach((fn) => fn({ ...task }));
  }

  let counter = 0;

  const start = async (req) => {
    const id = `mock-${++counter}`;
    const imageCount = req.image_count ?? 3;
    const videoCount = req.video_count ?? 1;
    const task = {
      id,
      title: req.title,
      quality_label: req.quality_label || (videoCount ? "视频" : `${imageCount} 图`),
      status: "queued",
      image_pct: 0,
      video_pct: 0,
      image_count: imageCount,
      video_count: videoCount,
      downloaded: 0,
      total: 24.6 * 1024 * 1024,
      speed_bps: 0,
      output_path: "",
      message: "排队中",
    };
    const imageTotal = 9.2 * 1024 * 1024;
    const videoTotal = 15.4 * 1024 * 1024;
    emit(task);

    const state = { image: 0, video: 0, phase: 0 };
    const timer = setInterval(() => {
      if (state.phase === 0) {
        state.image += imageTotal * 0.18;
        if (state.image >= imageTotal) {
          state.image = imageTotal;
          state.phase = 1;
        }
        task.image_pct = imageCount ? (state.image / imageTotal) * 100 : 100;
        task.status = "downloading";
        task.message = "下载图片";
        task.speed_bps = 3.1 * 1024 * 1024;
      } else if (state.phase === 1) {
        state.video += videoTotal * 0.16;
        if (state.video >= videoTotal) {
          state.video = videoTotal;
          state.phase = 2;
        }
        task.video_pct = videoCount ? (state.video / videoTotal) * 100 : 100;
        task.message = "解析并下载视频";
      } else {
        task.image_pct = 100;
        task.video_pct = 100;
        task.status = "saving";
        task.message = "保存正文与存档";
        task.speed_bps = 0;
        task.output_path = `${status().output_dir}\\示例公众号\\示例合集\\01 示例文章标题`;
        clearInterval(timer);
        timers.delete(id);
        emit({ ...task });
        setTimeout(() => {
          task.status = "done";
          task.message = "已完成";
          emit({ ...task });
        }, 700);
        return;
      }
      task.downloaded = state.image + state.video;
      emit({ ...task });
    }, 260);
    timers.set(id, timer);
    return id;
  };

  const cancel = async (taskId) => {
    const timer = timers.get(taskId);
    if (timer) clearInterval(timer);
    timers.delete(taskId);
  };

  // 浏览器预览用：从已解析的假清单里继续往后取
  const moreState = new Map();
  const probeMore = async (input, want) => {
    const all = ALL_ARTICLES;
    const from = moreState.get(input) ?? 20;
    const to = Math.min(from + want, all.length);
    moreState.set(input, to);
    return {
      items: all.slice(from, to),
      loaded: to,
      total: 0,
      exhausted: to >= all.length,
      note: to >= all.length ? "已到合集末尾" : "",
    };
  };

  const onUpdate = async (handler) => {
    listeners.add(handler);
    return () => listeners.delete(handler);
  };

  // 仅浏览器预览用的兜底：真值在 Rust 的 naming::VARIABLES，
  // 桌面端一律走 naming_variables 命令，这份副本只影响脱离桌面壳的预览。
  const VARIABLES = [
    ["title", "标题", "标题与作者", "文章标题（超长自动截断）"],
    ["author", "公众号名", "标题与作者", "发布文章的公众号"],
    ["collection", "合集名", "来源", "来自合集时是合集名；单篇文章时是年月"],
    ["bid", "文章标识", "标识", "sn / msgid（文章的唯一标识）"],
    ["uid", "公众号 ID", "标识", "__biz（公众号的 base64 标识）"],
    ["publish_date", "发布日期", "时间", "文章发布那天"],
    ["date", "下载日期", "时间", "任务创建那天"],
    ["year", "发布年", "时间", "如 2026"],
    ["month", "发布月", "时间", "如 01（补零）"],
    ["source_kind", "来源类型", "来源", "单篇文章 / 合集"],
    ["index", "序号", "来源", "批次内由旧到新；单条链接为 1"],
    ["ext", "扩展名", "来源", "正文 / 图片 / 视频各自的后缀"],
  ];

  const SAMPLE = {
    title: "示例文章标题",
    author: "示例公众号",
    collection: "示例合集",
    bid: "f553dd20b542",
    uid: "MzYzOTU1NTUzNw==",
    date: "2026-10-01",
    publish_date: "2026-09-20",
    year: "2026",
    month: "09",
    source_kind: "合集",
    index: 7,
  };

  const resumePending = async () => 0;

  const libraryFolders = async () => ({
    mid: "",
    created: [
      {
        id: 1,
        bid: "",
        title: "示例公众号 · 日常随笔（33 篇）",
        created_at: 1766500000,
        pics: 33,
        video: false,
        url: "",
        kind: "folder",
      },
    ],
    subscribed: [],
    fav_note: "本地已下载 1 个合集/分组",
    follow_note: "",
  });

  const namingVariables = () =>
    VARIABLES.map(([token, label, section, hint]) => ({ token, label, section, hint }));

  // opts: { ext, sample }——sample 提供变量样例值（标题/作者/序号等）
  const previewNaming = (template, opts = {}) => {
    const ext = typeof opts === "string" ? opts : opts.ext || "txt";
    const sample = (typeof opts === "object" && opts.sample) || {};
    const merged = { ...SAMPLE, ...sample };
    const segments = [];
    let usedExt = false;
    for (const raw of String(template ?? "").split(/[/\\]/)) {
      let out = "";
      let rest = raw;
      while (true) {
        const start = rest.indexOf("{");
        if (start === -1) {
          out += rest;
          break;
        }
        out += rest.slice(0, start);
        const end = rest.indexOf("}", start);
        if (end === -1) {
          out += rest.slice(start);
          break;
        }
        const token = rest.slice(start + 1, end);
        if (token === "ext") {
          usedExt = true;
          out += ext;
        } else if (token in merged) {
          out += merged[token] === "" ? "" : String(merged[token]);
        } else {
          out += `{${token}}`;
        }
        rest = rest.slice(end + 1);
      }
      const cleaned = out
        .replace(/[\\/:*?"<>|]/g, "_")
        .trim()
        .replace(/\.+$/, "")
        .trim();
      if (cleaned && cleaned !== "." && cleaned !== "..") segments.push(cleaned);
    }
    if (!usedExt) {
      if (segments.length) segments[segments.length - 1] += `.${ext}`;
      else segments.push(`wxdown.${ext}`);
    }
    return segments.join("/");
  };

  return {
    status,
    settings,
    updateSettings,
    probe,
    probeMore,
    start,
    cancel,
    onUpdate,
    resumePending,
    libraryFolders,
    namingVariables,
    previewNaming,
  };
})();
