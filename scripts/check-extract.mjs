// 注入脚本自检：语法 + 抽取行为回归。
//
// 起因（真实事故）：EXTRACT_JS 里曾写死"主页笔记最多 300 条 / 展平上限 600"，
// 而收集流程是"每轮全量抽取 + 跨轮合并"，抽取比页面已加载的少时合并数不再增长，
// 被误判成"到底了"——用户看到的现象就是"只能收到 300 条"。
// 这个脚本用伪造的分页嵌套状态守住这条线：上限、增量回传、换页回全量。
//
// 用法：node scripts/check-extract.mjs
import { readFileSync, writeFileSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const src = readFileSync(join(root, "src-tauri/src/commands.rs"), "utf8");
const work = mkdtempSync(join(tmpdir(), "wxdown-check-"));

const scripts = ["PAGE_BAR_JS", "EXTRACT_JS", "SCROLL_BOTTOM_JS", "QR_KEEPER_JS"];
const extracted = {};
for (const name of scripts) {
  const m = src.match(new RegExp(`const ${name}: &str = r#"([\\s\\S]*?)"#;`));
  if (!m) throw new Error(`commands.rs 里找不到 ${name}`);
  extracted[name] = m[1];
  const file = join(work, `${name}.js`);
  writeFileSync(file, m[1], "utf8");
  execFileSync(process.execPath, ["--check", file]); // 语法错了就在这里抛出
  console.log(`语法 OK  ${name}（${m[1].length} 字符）`);
}

// ── 行为仿真：伪造主页状态，验证抽取脚本 ──
const code = extracted.EXTRACT_JS;
let captured = "";
global.location = {
  href: "https://www.xiaohongshu.com/user/profile/66f3c481000000001c002a76",
  replace: (u) => { captured = u; },
};
// 回传通道：EXTRACT_JS 用 history.replaceState 上报（实测 location.replace 会触发
// 笔记页反篡改、立刻被弹回首页），桩也走这里
global.history = {
  replaceState: (_a, _b, u) => { captured = u; },
};
global.window = global;
global.document = { title: "无期迷途 - 小红书", querySelector: () => null, querySelectorAll: () => [] };

function buildPages(pages) {
  const out = [];
  for (let p = 0; p < pages; p++) {
    const pageObj = {};
    for (let i = 0; i < 20; i++) {
      const n = p * 20 + i;
      pageObj[String(i)] = {
        id: ("a" + n.toString(16)).padStart(24, "0"),
        xsecToken: "AB" + "x".repeat(50) + n,
        time: 1700000000000 + n * 1000,
        noteCard: {
          displayTitle: "第 " + n + " 条笔记标题",
          type: n % 3 === 0 ? "video" : "normal",
          user: { nickname: "无期迷途" },
        },
      };
    }
    out.push(pageObj);
  }
  return out;
}

function run(pages, posted) {
  window.__INITIAL_STATE__ = {
    user: {
      // 实测结构：总数在 userPageData.posted；notes 是"分页嵌套"的列表
      userPageData: { posted, nickname: "无期迷途" },
      notes: buildPages(pages),
    },
  };
  captured = "";
  eval(code);
  // 回传通道实测切到 history.replaceState（location.replace 写一次就把笔记页弹走）
  const m = captured.match(/#wxdownid=[^~]*~(.+)$/);
  if (!m) throw new Error("没有捕获到上报片段");
  return JSON.parse(decodeURIComponent(m[1]));
}

let failed = 0;
const check = (name, got, want) => {
  const ok = got === want;
  if (!ok) failed += 1;
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}: ${got}（期望 ${want}）`);
};

const first = run(75, 1500);
check("首轮全量回传（旧代码会卡在 300）", first.notes.length, 1500);
check("总数上报 posted", first.notesCount, 1500);

const second = run(80, 1600);
check("增长后只回增量（附 40 条重叠）", second.notes.length, 140);

const third = run(80, 1600);
check("无增长时只回重叠段", third.notes.length, 40);

global.location.href = "https://www.xiaohongshu.com/user/profile/000000000000000000000001";
const fourth = run(10, 200);
check("换主页后回全量", fourth.notes.length, 200);

global.location.href = "https://www.xiaohongshu.com/user/profile/66f3c481000000001c002a76";
const fifth = run(32, 625);
check("收全场景条数（32 页 × 20）", fifth.notes.length, 640);
check("收全场景总数", fifth.notesCount, 625);

if (failed) {
  console.error(`\n${failed} 项失败`);
  process.exit(1);
}
console.log("\n全部 PASS");
