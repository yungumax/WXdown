# -*- coding: utf-8 -*-
"""WXdown 前端适配补丁:小红书口径 → 微信公众号口径(一次性迁移脚本)"""
import re
import sys
import io

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")


def patch(path, pairs):
    s = open(path, encoding="utf-8").read()
    missed = []
    for old, new in pairs:
        if old not in s:
            missed.append(old[:70].replace("\n", "\\n"))
            continue
        s = s.replace(old, new)
    open(path, "w", encoding="utf-8").write(s)
    return missed


missed_all = {}

# ───────── styles.css: 微信绿主题 ─────────
missed_all["styles.css"] = patch(
    "src/styles.css",
    [
        (
            """     所以主色当文字色时用深红 --accent-ink；图标、描边、面用 --accent。 */
  --accent: #ff2442;
  --accent-dark: #e01a35;
  --accent-ink: #c4132c;
  --accent-soft: #ffeef1;
  --accent-line: #ffd0d8;""",
            """     所以主色当文字色时用深绿 --accent-ink；图标、描边、面用 --accent。 */
  --accent: #07c160;
  --accent-dark: #06ad56;
  --accent-ink: #058a46;
  --accent-soft: #e8f8ee;
  --accent-line: #b7eccb;""",
        ),
        (
            """  --accent: #ff2442;
  --accent-ink: #ff7a8c;
  --accent-dark: #ff3c58;
  --accent-soft: rgba(255, 36, 66, 0.16);""",
            """  --accent: #07c160;
  --accent-ink: #4ed88a;
  --accent-dark: #06ad56;
  --accent-soft: rgba(7, 193, 96, 0.16);""",
        ),
    ],
)

# ───────── download-request.js ─────────
missed_all["download-request.js"] = patch(
    "src/download-request.js",
    [
        (
            """export const KIND_LABELS = {
  post: "单条笔记",
  user: "用户主页",
  feed: "推荐流",
  search: "搜索结果",
};""",
            """export const KIND_LABELS = {
  article: "单篇文章",
  album: "合集",
  account: "公众号全部文章",
};""",
        ),
        (
            """/** 图片规格选项（唯一定义，设置页与下载弹层共用，避免两处文案漂移）。
 *
 * 实测（抽样多条笔记逐张解码 + 量体积）：站点对笔记图片只放出两个变体，
 * **分辨率完全相同**（1080 宽即网页端上限），差别只在压缩质量：
 * 同图原图 84 KB vs 压缩图 18 KB（约 1/4）。所以只有两档可选，
 * 不存在"更清晰"的第三档——网传的换域名/剥后缀取原图技巧实测全部 403。 */
export const IMAGE_CHOICES = [
  { value: "large", label: "原图（最佳画质，最高 1080 宽）" },
  { value: "thumbnail", label: "压缩图（省空间，约 1/4 体积）" },
];""",
            """/** 图片规格选项（唯一定义，设置页与下载弹层共用）。
 * 公众号文章图片只有页面放出的一档（mmbiz data-src 原图），
 * 没有清晰度可挑——选项保留一档，文案如实说明。 */
export const IMAGE_CHOICES = [
  { value: "large", label: "原图（微信放出的最高画质）" },
];""",
        ),
        (
            """/** 视频清晰度选项（唯一定义，引导页与设置页共用）。
 *  档位按视频**短边**命名；选具体档时取"不超过该档"的最高流（要 1080P 就不下 4K），
 *  该档不存在则自动退到最佳可用。实测单条视频最高有 2160P(4K)。 */
export const VIDEO_QUALITIES = [
  { value: "auto", label: "最佳可用（分辨率与码率都最高）" },
  { value: "2160p", label: "2160P（4K，单条可达 70 MB）" },
  { value: "1440p", label: "1440P（2K）" },
  { value: "1080p", label: "1080P（多数视频的最高档）" },
  { value: "720p", label: "720P（体积适中）" },
  { value: "480p", label: "480P（最小，省流量）" },
];""",
            """/** 视频清晰度选项（唯一定义，引导页与设置页共用）。
 *  文章内视频（微信原生/腾讯视频）由工具尽力解析直链，按能拿到的
 *  最高画质保存，没有档位可选。 */
export const VIDEO_QUALITIES = [
  { value: "auto", label: "最佳可用（自动解析最高画质直链）" },
];""",
        ),
        (
            """    source_kind: KIND_LABELS[probe.kind] ?? "笔记",""",
            """    source_kind: KIND_LABELS[probe.kind] ?? "文章",""",
        ),
        (
            """    mid: probe.mid || entry.mid || "",
    publish_date: localDate(created),""",
            """    mid: probe.mid || entry.mid || "",
    collection: "",
    publish_date: localDate(created),""",
        ),
        (
            """/** 批量条目的命名变量：主页按"由旧到新"给序号（position 由表格算好传入） */""",
            """/** 批量条目的命名变量：合集按"由旧到新"给序号（position 由表格算好传入） */""",
        ),
        (
            """    bid: entry.bid || "",
    mid: entry.mid || "",
    publish_date: localDate(entry.created_at),""",
            """    bid: entry.bid || "",
    mid: entry.mid || "",
    // 合集名进命名变量（文件夹模板 {collection} 用）
    collection: batch.kind === "album" ? batch.title || "" : "",
    publish_date: localDate(entry.created_at),""",
        ),
        (
            """    source_kind: KIND_LABELS[batch.kind] ?? "笔记",""",
            """    source_kind: KIND_LABELS[batch.kind] ?? "文章",""",
        ),
    ],
)

# ───────── App.vue ─────────
missed_all["App.vue"] = patch(
    "src/App.vue",
    [
        (
            """import * as api from "./api";
import { collectCount, collected } from "./collect";""",
            """import * as api from "./api";""",
        ),
        (
            """/** WEB 窗口可见性（每 1.5 秒同步一次）：驱动标题栏 WEB 按钮的状态点与开/收切换。
 *  轮询本身还兼职让后端记录"用户最后在看的页面"（点 WEB 恢复用），别删。 */
const webBar = ref({ url: "", visible: false });
let webBarTimer = null;

async function pollWebBar() {
  try {
    const state = await api.webBarState();
    webBar.value.visible = !!state?.visible;
    webBar.value.url = state?.url || "";
  } catch {
    // 忽略：窗口可能正在关闭
  }
}

""",
            "",
        ),
        (
            """let unlistenTask = null;
let unlistenLogin = null;
let unlistenFromWeb = null;
let unlistenCollect = null;
let unlistenCollectDone = null;
let toastTimer = null;""",
            """let unlistenTask = null;
let unlistenLogin = null;
let toastTimer = null;""",
        ),
        (
            """  await pollWebBar();
  webBarTimer = setInterval(pollWebBar, 1500);

""",
            "",
        ),
        (
            """  // WEB 地址栏的「解析」按钮：把当前地址送到解析页
  unlistenFromWeb = await api.onWebToParse((url, fill) => {
    if (url) openSource(url, !fill);
  });

  // 网页内收集：进度与完成
  unlistenCollect = await api.onCollectUpdate((count) => {
    collectCount.value = count;
    if (count > 0) showToast(`网页收集中：已收集 ${count} 条`);
  });
  // 完成提示的措辞由后端统一生成（同一条也显示在网页内横幅），前端只负责展示
  unlistenCollectDone = await api.onCollectDone((items, auto, total, stateUniq, message) => {
    collectCount.value = 0;
    if (!items.length) {
      showToast("网页收集：没有收集到内容");
      return;
    }
    collected.value = { items, stamp: Date.now() };
    showToast(message || `网页收集完成：${items.length} 条，已送入解析结果`);
  });
""",
            "",
        ),
        (
            """onUnmounted(() => {
  if (webBarTimer) clearInterval(webBarTimer);
  if (unlistenTask) unlistenTask();
  if (unlistenLogin) unlistenLogin();
  if (unlistenFromWeb) unlistenFromWeb();
  if (unlistenCollect) unlistenCollect();
  if (unlistenCollectDone) unlistenCollectDone();
  clearTimeout(toastTimer);
});""",
            """onUnmounted(() => {
  if (unlistenTask) unlistenTask();
  if (unlistenLogin) unlistenLogin();
  clearTimeout(toastTimer);
});""",
        ),
        (
            """  folder_template: "{author}/{year}-{month}",""",
            """  folder_template: "{author}/{collection}",""",
        ),
        (
            """  showToast(loginInfo.logged_in ? `已登录：${loginInfo.uname || "小红书账号"}` : "已退出登录");""",
            """  showToast(loginInfo.logged_in ? `已登录：${loginInfo.uname || "微信账号"}` : "已退出登录");""",
        ),
        (
            """/** 标题栏 WEB 按钮：网页窗口开着就收起，没开就打开（地址/导航在网页窗口内完成） */
async function openWeb() {
  try {
    if (webBar.value.visible) {
      await api.webBarHide();
    } else {
      await api.openWebHome();
    }
  } catch (error) {
    showToast(String(error));
  }
}

""",
            "",
        ),
        (
            """    await api.webLoginOpen();
    await api.webLoginWatch();
    showToast("已打开小红书官方登录页，登录成功后这里会自动完成绑定");""",
            """    await api.webLoginOpen();
    await api.webLoginWatch();
    showToast("已发起登录，登录成功后这里会自动完成绑定");""",
        ),
        (
            """    <TitleBar
      :login="login"
      :version="version"
      :theme="settings?.theme || 'system'"
      :web-active="webBar.visible"
      @login="openLogin"
      @set-theme="setTheme"
      @web="openWeb"
    />""",
            """    <TitleBar
      :login="login"
      :version="version"
      :theme="settings?.theme || 'system'"
      @login="openLogin"
      @set-theme="setTheme"
    />""",
        ),
        (
            """    <!-- WEB 窗口的地址/导航/解析都在它窗口内的地址栏里完成，
         主窗口不再重复一条工具栏；WEB 按钮（标题栏）负责开/收 -->
""",
            "",
        ),
    ],
)

# ───────── TitleBar.vue: 去掉 WEB 按钮 ─────────
s = open("src/components/TitleBar.vue", encoding="utf-8").read()
s = s.replace(
    """  /** 网页窗口是否开着：开着时 WEB 按钮点亮（点击=收起） */
  webActive: { type: Boolean, default: false },
""",
    "",
)
s = s.replace(
    """const emit = defineEmits(["login", "set-theme", "web"]);""",
    """const emit = defineEmits(["login", "set-theme"]);""",
)
s = re.sub(r'\s*<button\s+class="web-chip"[\s\S]*?</button>\n', "\n", s, count=1)
open("src/components/TitleBar.vue", "w", encoding="utf-8").write(s)

# ───────── ParsePage.vue: 业务口径替换 ─────────
m = patch(
    "src/pages/ParsePage.vue",
    [
        # kind 值
        ('.kind === "post"', '.kind === "article"'),
        ('kind === "post"', 'kind === "article"'),
        ('probe.kind === "user"', 'probe.kind === "album"'),
        ('`post:${probe.bid}` : `user:${probe.uid}`', '`article:${probe.bid}` : `album:${probe.uid}`'),
        ('`post:${item.probe.bid || item.input}`', '`article:${item.probe.bid || item.input}`'),
        # 来源标签
        (
            """const sources = [
  { label: "单条笔记", icon: "link" },
  { label: "视频笔记", icon: "play" },
  { label: "图文/多图", icon: "photo" },
  { label: "用户主页", icon: "user" },
  { label: "分享文本", icon: "clipboard" },
  { label: "推荐流", icon: "broadcast" },
  { label: "搜索结果", icon: "search" },
  { label: "多行批量", icon: "listLine" },
];""",
            """const sources = [
  { label: "单篇文章", icon: "link" },
  { label: "视频文章", icon: "play" },
  { label: "图文/多图", icon: "photo" },
  { label: "合集", icon: "listLine" },
  { label: "分享文本", icon: "clipboard" },
  { label: "多行批量", icon: "listLine" },
];""",
        ),
        # 表头
        ("""              <th>文案</th>""", """              <th>标题</th>"""),
        ("""              <th class="col-owner">博主</th>""", """              <th class="col-owner">公众号</th>"""),
        # 输入区
        (
            """      <p class="lead">粘贴小红书笔记链接或 App 分享文本（一行一个），解析后再选择要下载的内容。</p>""",
            """      <p class="lead">粘贴公众号文章或合集链接（一行一个），解析后再选择要下载的内容。</p>""",
        ),
        (
            'placeholder="https://www.xiaohongshu.com/explore/66b7d3a7000000001e01c8b3?xsec_token=…&#10;https://www.xiaohongshu.com/user/profile/58c8014a6a6a690dbb0c84c1&#10;http://wxlink.com/a1B2c3&#10;66b7d3a7000000001e01c8b3"',
            'placeholder="https://mp.weixin.qq.com/s/xxxxxxxxxxxx&#10;https://mp.weixin.qq.com/s?__biz=Mz…&mid=…&idx=1&sn=…&#10;https://mp.weixin.qq.com/mp/appmsgalbum?__biz=…&album_id=…"',
        ),
        (
            """      <p class="hint">
        每行一个来源；支持笔记链接（含 App 分享文本整段粘贴）、用户主页与 wxlink 短链。
        用户主页读取最新一批笔记；更早的内容请在 App 里分享具体笔记链接。
      </p>""",
            """      <p class="hint">
        每行一个来源；支持文章链接（微信里「复制链接」可直接整段粘贴）与合集链接。
        合集自动翻页加载全部文章；「公众号全部文章」与「小程序资源」将在后续版本提供。
      </p>""",
        ),
        # 帮助面板整体重写
        (
            """          <div class="guide-sec">
            <h3>一次完整的下载</h3>
            <ol class="guide-steps">
              <li><b>打开网页</b>：点右上角 <b>WEB</b> 打开小红书网页，登录一次（之后保持登录）。</li>
              <li><b>网页收集</b>：在网页里打开用户主页（左下角显示「共 N 条」），点页面顶部红色「开始解析」，网页自动滚动收集，进度实时可见；收集完自动回到这里。</li>
              <li><b>粘贴链接</b>：单条内容也可以在 App 里分享复制链接（或点网页地址栏「复制」直接送进解析栏），一行一个、可批量，点「开始解析」或 Ctrl + Enter。</li>
              <li><b>勾选与下载</b>：在「选择内容」页勾选条目，可在「下载设置」里调整本批的视频清晰度 / 图片规格，点「下载所选」。</li>
              <li><b>查看进度</b>：到「传输」页看进度，行内可暂停 / 继续 / 取消，完成后「打开位置」定位文件。</li>
            </ol>
          </div>
          <div class="guide-sec">
            <h3>链接会怎么解析</h3>
            <ul>
              <li>用户主页：读取主页最新一批笔记；更早的内容网页端不提供，请在 App 里分享具体笔记链接。</li>
              <li>笔记链接：支持 App 分享文本整段粘贴、wxlink 短链、探索页链接；带 xsec_token 的链接成功率更高。</li>
              <li>探索页 / 搜索结果：解析当前页的卡片；更多内容用网页收集滚动拿全。</li>
            </ul>
          </div>
          <div class="guide-sec">
            <h3>小贴士</h3>
            <ul>
              <li>重复收集不会产生重复条目：收集按笔记去重，再收一次只用来补尾部没加载出的条目。</li>
              <li>少数笔记（仅自己可见 / 审核中）网页端不放出——总数比可收数多几条属正常；想要的单条在 App 里分享链接即可下载。</li>
              <li>图片只有两档（分辨率相同、最高 1080 宽，区别在压缩质量）；视频清晰度按短边分级，最高 2160P/4K，所选档不存在时自动取最佳可用。</li>
              <li>重复下载不会产生副本：已存在的文件自动跳过。</li>
            </ul>
          </div>""",
            """          <div class="guide-sec">
            <h3>一次完整的下载</h3>
            <ol class="guide-steps">
              <li><b>复制链接</b>：在微信里打开文章（或合集页），右上角「…」→「复制链接」。</li>
              <li><b>粘贴解析</b>：回到这里粘贴（可多行批量），点「开始解析」或 Ctrl + Enter；整段分享文本直接粘贴也行，链接会自动识别。</li>
              <li><b>勾选与下载</b>：合集会列出全部文章，点「解析全部」翻完，勾选后点「下载所选」；单篇文章直接确认下载。</li>
              <li><b>查看进度</b>：到「传输」页看进度，行内可暂停 / 继续 / 取消，完成后「打开位置」定位文件夹。</li>
            </ol>
          </div>
          <div class="guide-sec">
            <h3>每篇文章会保存什么</h3>
            <ul>
              <li>每篇一个文件夹：<b>正文.txt</b>（标题/作者/时间/原文链接 + 正文）、<b>01.jpg 02.png…</b>（按序号的原图）、<b>index.html</b>（图片本地化的离线原版式存档，双击可看）。</li>
              <li>文章里的视频与语音尽力解析直链保存；解析不出的会在正文与存档里留原文链接（微信限制，不算失败）。</li>
              <li>目录结构默认「公众号名 / 合集名 / 序号 标题」，可在设置里改模板。</li>
            </ul>
          </div>
          <div class="guide-sec">
            <h3>小贴士</h3>
            <ul>
              <li>合集第一次解析加载前 20 篇，点「解析全部」自动翻页到末尾；大合集翻页要几十秒属正常。</li>
              <li>重复下载不会产生副本：已存在的文章文件夹自动跳过。</li>
              <li>「公众号全部文章」（UP主式全量）与「小程序视频图片」将在后续版本提供。</li>
            </ul>
          </div>""",
        ),
        # startDownload 请求加 collection
        (
            """      const probe = row.source.probe;
      const single = probe.kind === "article";
      await api.startDownload({
        bid: row.entry?.bid ?? probe.bid,
        mid: row.entry?.mid ?? probe.mid,
        source: probe.kind,""",
            """      const probe = row.source.probe;
      const single = probe.kind === "article";
      await api.startDownload({
        bid: row.entry?.bid ?? probe.bid,
        mid: row.entry?.mid ?? probe.mid,
        source: probe.kind,
        collection: probe.kind === "album" ? probe.title || "" : "",""",
        ),
        # 杂项文案
        ('''source_kind: KIND_LABELS[row.source.probe.kind] ?? "笔记",''',
         '''source_kind: KIND_LABELS[row.source.probe.kind] ?? "文章",'''),
        ('''          : "笔记",''', '''          : "文章",'''),
        ('''    // 小红书网页端不给总数：只有后端返回了非 0 的总数才覆盖（0 = 沿用主页声明的笔记总数）''',
         '''    // 合集接口给真实总数：后端返回非 0 才覆盖'''),
        ('''/** 下载：清单里勾选的行；只有一条笔记时直接下这一条 */''',
         '''/** 下载：清单里勾选的行；只有一篇文章时直接下这一条 */'''),
        ('''      <!-- 底部统计只对批量清单有意义，单条笔记不渲染这条空栏 -->''',
         '''      <!-- 底部统计只对批量清单有意义，单篇文章不渲染这条空栏 -->'''),
    ],
)
missed_all["ParsePage.vue"] = m

# ───────── SettingsPage.vue ─────────
missed_all["SettingsPage.vue"] = patch(
    "src/pages/SettingsPage.vue",
    [
        ('''<span class="env-name">{{ login.logged_in ? login.uname : "小红书账号" }}</span>''',
         '''<span class="env-name">{{ login.logged_in ? login.uname : "微信账号" }}</span>'''),
        ('''{{ login.logged_in ? `ID ${login.mid}` : "网页登录后可读取自己的主页笔记" }}''',
         '''{{ login.logged_in ? `ID ${login.mid}` : "公众平台登录后可枚举任意公众号全部文章（下一版本提供）" }}'''),
        ('''              图片规格决定多图笔记下载原图还是压缩图；视频清晰度不可用时自动取最佳可用档''',
         '''              公众号图片按微信放出的最高画质保存；文章内视频自动解析最高画质直链'''),
        ('''                同一条笔记的媒体共用同一个名字前缀。''',
         '''                同一篇文章的媒体共用同一个名字前缀。'''),
        ('''                年月取自笔记发布时间，同一作者的内容自动归档到一起。''',
         '''                合集名取自合集，同一公众号的内容自动归档到一起。'''),
        ('''                单条笔记直链也会进作者目录，除非模板里不写作者变量。''',
         '''                单篇文章链接也会进公众号目录，除非模板里不写作者变量。'''),
        ('''                想区分来源，可在模板里用 <code>{source_kind}</code>（单条笔记 / 用户主页）。''',
         '''                想区分来源，可在模板里用 <code>{source_kind}</code>（单篇文章 / 合集）。'''),
    ],
)

# ───────── LibraryPage.vue ─────────
missed_all["LibraryPage.vue"] = patch(
    "src/pages/LibraryPage.vue",
    [
        ('''/** 条目对应的笔记链接（含 xsec_token，解析时必需） */''',
         '''/** 条目对应的本地文件夹路径（点卡片打开） */'''),
        ('''  return `我的笔记 · ${formatTime(item.created_at)}`;''',
         '''  return `已下载合集 · ${formatTime(item.created_at)}`;'''),
        ('''          <h2>我的笔记</h2>''', '''          <h2>下载记录</h2>'''),
        ('''          <p class="meta">主页最新一批笔记 · 点卡片去解析该条笔记</p>''',
         '''          <p class="meta">按 公众号 / 合集 统计的本地下载记录 · 点卡片打开文件夹</p>'''),
        ('''placeholder="搜索笔记文案"''', '''placeholder="搜索公众号或合集"'''),
        ('''        <p v-if="loading && !account" class="hint pad">正在读取主页笔记…</p>''',
         '''        <p v-if="loading && !account" class="hint pad">正在读取下载记录…</p>'''),
        ('''              {{ query ? "没有匹配的笔记" : "这个账号还没有公开笔记" }}''',
         '''              {{ query ? "没有匹配的记录" : "还没有下载记录" }}'''),
        ('''              {{ query ? "换个关键词试试。" : "发布一条公开笔记后，这里就能看到。" }}''',
         '''              {{ query ? "换个关键词试试。" : "解析一篇文章或合集开始使用。" }}'''),
    ],
)

# ───────── LoginDialog.vue ─────────
missed_all["LoginDialog.vue"] = patch(
    "src/components/LoginDialog.vue",
    [
        ('''// 账号面板（小红书版）：只在已登录时打开——显示昵称、头像与退出登录。''',
         '''// 账号面板：只在已登录时打开——显示昵称、头像与退出登录。'''),
        ('''        <h2>{{ login.logged_in ? "账号" : "登录小红书" }}</h2>''',
         '''        <h2>{{ login.logged_in ? "账号" : "登录微信公众平台" }}</h2>'''),
        ('''          <p class="uname">{{ login.uname || "已登录的小红书账号" }}</p>''',
         '''          <p class="uname">{{ login.uname || "已登录的公众账号" }}</p>'''),
        ('''          已登录状态下可解析笔记、读取自己的主页笔记；部分仅粉丝可见的内容需关注关系。''',
         '''          公众平台登录用于「公众号全部文章」枚举（UP主式全量下载，下一版本提供）；文章与合集下载无需登录。'''),
    ],
)

# ───────── Onboarding.vue ─────────
missed_all["Onboarding.vue"] = patch(
    "src/components/Onboarding.vue",
    [
        ('''          <b>怎么用</b>：点右上角 <b>WEB</b> 打开小红书网页并登录 → 在主页里点页面顶部的''',
         '''          <b>怎么用</b>：在微信里复制文章或合集链接 → 粘贴到解析栏 →'''),
    ],
)

# ───────── AboutPage.vue ─────────
missed_all["AboutPage.vue"] = patch(
    "src/pages/AboutPage.vue",
    [
        ('''        小红书图片与视频下载工具，支持单条笔记解析、用户主页批量下载，
        登录后可读取自己的主页笔记；界面沿用 WeBodown 的设计体系。''',
         '''        微信公众号内容下载工具，支持单篇文章与合集批量下载
        （正文 · 图片 · 视频 · 离线存档）；界面沿用 BILIdown 家族的设计体系。'''),
        ('''          <dd>笔记链接（含 App 分享文本）· 用户主页 · wxlink 短链</dd>''',
         '''          <dd>公众号文章链接 · 合集链接（可多行批量）</dd>'''),
        ('''          <dd>与 WeBodown 同一套设计体系（平面化 · 浅色/深色主题 · 小红书主题红）</dd>''',
         '''          <dd>与 BILIdown 同一套设计体系（平面化 · 浅色/深色主题 · 微信主题绿）</dd>'''),
        ('''        仅供个人学习与离线收藏使用，请遵守小红书用户协议，不要用于传播或商业用途。''',
         '''        仅供个人学习与离线收藏使用，请尊重作者版权，不要用于传播或商业用途。'''),
    ],
)

# ───────── TaskRow.vue ─────────
missed_all["TaskRow.vue"] = patch(
    "src/components/TaskRow.vue",
    [
        ('''/** 三个阶段各自的状态，驱动进度条与状态点（小红书版：图片流 | 视频流 | 文案与收尾） */''',
         '''/** 三个阶段各自的状态，驱动进度条与状态点（公众号版：图片 | 视频语音 | 正文与存档） */'''),
    ],
)

print("done, missed:")
for k, v in missed_all.items():
    if v:
        print(f"  [{k}]")
        for item in v:
            print(f"    - {item}")
