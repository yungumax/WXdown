// 一次性验证脚本：用真实抓取的小红书探索页 HTML 验证 __INITIAL_STATE__ 抽取
// 与卡片字段结构（与 Rust 端 client.rs/api.rs 的同一套逻辑）。
import { readFileSync } from "node:fs";

const file = process.argv[2];
const html = readFileSync(file, "utf8");

const marker = "window.__INITIAL_STATE__";
const start = html.indexOf(marker);
console.log("marker at", start);
if (start < 0) process.exit(1);

const jsonStart = html.indexOf("{", start);
const end = html.indexOf("</script>", jsonStart);
const raw = html.slice(jsonStart, end).trimEnd().replace(/;$/, "");
// 与 Rust 端同一处理：裸 undefined → null
const fixed = raw.replace(/([:,\[]\s*)undefined/g, "$1null");

let state;
try {
  state = JSON.parse(fixed);
  console.log("JSON.parse OK");
} catch (e) {
  console.log("PARSE FAIL:", e.message.slice(0, 160));
  process.exit(1);
}

const feeds = state?.feed?.feeds ?? [];
console.log("feeds:", feeds.length);
const f = feeds[0] ?? {};
console.log("feed[0] keys:", Object.keys(f).join(","));
console.log("id:", f.id);
console.log("title:", (f.noteCard?.displayTitle ?? "").slice(0, 24));
console.log("type:", f.noteCard?.type);
console.log("xsecToken(顶层):", (f.xsecToken ?? "").slice(0, 20) + "…");
console.log("user.nickname:", f.noteCard?.user?.nickname);
console.log("cover urlDefault:", (f.noteCard?.cover?.urlDefault ?? "").slice(0, 60));
