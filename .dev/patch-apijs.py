# -*- coding: utf-8 -*-
"""api.js mock 数据换成微信口径"""
import io
import sys

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")

p = "src/api.js"
s = open(p, encoding="utf-8").read()

# 注释
s = s.replace(
    """// 解析来源：单条笔记链接 → post；用户主页 → user""",
    """// 解析来源：文章链接 → article；合集链接 → album""",
)
s = s.replace(
    """// 继续解析：小红书网页端没有可匿名调用的翻页接口，
// 后端如实返回 exhausted=true，界面隐藏「继续解析」。""",
    """// 继续解析：合集翻下一页（getalbum 分页）""",
)

# mock 整段替换：从 "// 仅浏览器预览用的假数据（小红书口径）" 到文件尾的 return 块之前
start = s.index("// 仅浏览器预览用的假数据（小红书口径）")
end = s.index("  return {")
new_mock = '''// 仅浏览器预览用的假数据（公众号口径）
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
    output_dir: "D:\\\\Zcode\\\\_data\\\\wxdown\\\\downloads",
    cookies_path: "D:\\\\Zcode\\\\_data\\\\wxdown\\\\cookies.json",
  });

  const settings = () => ({
    settings: {
      output_dir: "D:\\\\Zcode\\\\_data\\\\wxdown\\\\downloads",
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
    cookies_path: "D:\\\\Zcode\\\\_data\\\\wxdown\\\\cookies.json",
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
        task.output_path = `${status().output_dir}\\\\示例公众号\\\\示例合集\\\\01 示例文章标题`;
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
    for (const raw of String(template ?? "").split(/[/\\\\]/)) {
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
        .replace(/[\\\\/:*?"<>|]/g, "_")
        .trim()
        .replace(/\\.+$/, "")
        .trim();
      if (cleaned && cleaned !== "." && cleaned !== "..") segments.push(cleaned);
    }
    if (!usedExt) {
      if (segments.length) segments[segments.length - 1] += `.${ext}`;
      else segments.push(`wxdown.${ext}`);
    }
    return segments.join("/");
  };

'''
s = s[:start] + new_mock + s[end:]
open(p, "w", encoding="utf-8").write(s)
print("api.js mock rewritten")
