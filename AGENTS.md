# WXdown 项目规则（对任何代理/会话永久生效）

## 发布规则（用户明确要求）

- **禁止自动发布装机版**：不得在未经用户明确确认的情况下创建 GitHub Release、
  打 tag 触发发布工作流、或对外分发任何安装包。
- 构建（cargo build / tauri build）仅在本地产出，属开发行为，不受此限。
- 用户确认发布后再执行：改版本号 → tag → 推送 → 等待 CI 产出安装包 → 核对 Release。
- **签名私钥不入库**：私钥在 `D:\Zcode\_data\tauri-keys\wxdown.key`；
  CI 用 GitHub Secret `TAURI_SIGNING_PRIVATE_KEY`。丢失私钥 = 旧版本无法接受新更新包。

## 测试版规则

- **每轮代码修改完成后，默认重新构建并打开调试版（测试版）exe** 供用户验收：
  `cmd //c start "" "D:\Zcode\WXdown\target\debug\wxdown.exe"`
- 构建顺序：先 `npm run build` 再 `cargo build --features custom-protocol`（反了会嵌旧 dist）。
- 构建环境：先 `source scripts/msvc-env.sh`（便携 MSVC 工具链，见脚本注释；
  本仓库无 `.lldbin` 时自动借用 `D:\Zcode\WeBodown\.lldbin`）。
- cargo 联网拉依赖若被系统代理拦（报 mirrors.tuna 连不上）：`export no_proxy="*"` 绕过。

## 微信接口的坑（改代码前先读）

- **合集用旧 action**：`GET /mp/appmsgalbum?__biz=…&action=getalbum&album_id=…&f=json&count=N`
  免登录、桌面 UA 即可；翻页带上页末条目的 `begin_msgid`/`begin_itemidx`，
  `continue_flag:"1"` 表示还有。**不是** `action=getappmsgalbum`（那个要微信客户端
  会话，外部调用回 `ret:-3 no session`——实测踩过）。
- **合集页 HTML 是 SPA 壳**：新版页面（Vite 构建）条目全靠 JS 拉，HTML 里没有
  条目数据；别去解析页面，直接走上面的 JSON 接口。
- **文章页是服务端渲染**：`#activity-name`（标题）、`#js_name`/`var nickname`
  （公众号名）、`#publish_time`（发布时间）、`#js_content`（正文）。
  正文嵌套极深，取内层 HTML 必须 div 配平扫描（`article::js_content_html`），
  正则截到第一个 `</div>` 会截半。图片在 `img[data-src]`（mmbiz 域，wx_fmt 定格式）。
- **纯文字分享卡没有 js_content**：og:title 里塞的是整段正文（article1 样例），
  解析要兜底这条路径。
- **文章内视频两类**：`iframe.video_iframe` 的 data-src——v.qq.com（腾讯视频）与
  wxv_/readtemplate（微信原生）。直链解析是尽力而为：readtemplate 页/视频接口里
  抠 mp4；抠不到就记占位（正文与 HTML 存档里留原文链接），**不算任务失败**。
- **语音**：`<mpvoice voice_encode_fileid=…>` → `res.wx.qq.com/voice/getvoice?mediaid=…`
  免登录可用。
- **HTML 离线存档**（index.html）：剥 script/noscript、`visibility: hidden`→`visible`
  （页面靠 JS 显出正文，无 JS 环境必须手动解隐）、img 的 src 改本地相对路径、
  视频 iframe 换本地 `<video>` 或占位链接、mpvoice 换 `<audio>`。单测
  `archive_rewrites_img_src` 守着这几条。
- **每篇文章一个文件夹**：`正文.txt` + `01.jpg…` 序号图 + `video-01.mp4` +
  `voice-01.mp3` + `index.html`。skip 判定 = 文件夹里已有 `正文.txt`。
- **文件夹模板默认 `{author}/{collection}`**：collection 来自合集名；单篇文章
  没有合集时回退 `年-月`。命名模板渲染条目**文件夹**名（dir=true，不带扩展名）。
- **阶段二（已做但路线已死）**：公众平台登录 + 枚举任意公众号全部文章。
  登录设施完好可用（内嵌 webview 扫码 → 收割 Cookie+token → 落盘 mp_token.txt，
  重启自动恢复）。但**枚举接口 2026-07 底被微信整体关闭**（appmsg 回 ret 200009
  not found；继任者 appmsgpublish 从第一次调用就恒回 200013 freq control——不是
  限频冷却,是接口封禁,wechat-article-exporter issue #199 全社区同日失效）。
  searchbiz（按名查号）仍可用。`/mp/profile_ext` 需要**微信客户端会话**
  （uin/key/pass_ticket,平台 Cookie/匿名都回 no session）——凭据只能从微信
  客户端流量里抓（走阶段三的本地代理）。"UP主全量"场景并入阶段三嗅探。
- **阶段三（嗅探）已按用户要求移除**（2026-10-14）：本地 MITM 代理方案用户不要
  （不想装证书/接管系统代理）。完整代码备份在 `.dev/sniffer-backup/`
  （wx-sniffer crate + sniffer.rs + SnifferPage.vue），要恢复时拷回去并按
  git 历史式对照 lib.rs/state.rs 的接线点装回。技术结论仍然有效：hudsucker
  0.25 + rcgen 0.14 可行、请求侧嗅探可行、profile_ext 凭据收割可行——
  移除是产品决策不是技术失败。
- **公众号全量/小程序媒体：用户最终选择「维持现状」（2026-10-14）**——
  即不做直链下载/手动凭据/缓存解析，也不恢复嗅探。产品边界定格为：
  公众号文章 + 合集（贴文章自动展开所属合集）下载。平台登录设施保留在
  代码里（万一微信重开枚举接口）。替代路线方案（直链/凭据/缓存/嗅探恢复）
  都有过设计，真要加时翻会话记录或 .dev/sniffer-backup。

## 结构（BILIdown 家族三代同款）

- `crates/wx-core`：纯核心库（client 会话 / parser 输入识别 / article 文章页解析 /
  album 合集接口 / api 视频语音直链 / download 下载引擎）。网络冒烟测试：
  `cargo test -p wx-core --test network -- --nocapture`（样例链接失效就换公开新样例）。
- `src-tauri`：命令层（commands.rs 的 run_task 是文章下载管线）+ naming + state。
- `src`：Vue 前端（api.js 是唯一 IPC 入口，浏览器模式走 mock）。
- 数据目录 `%APPDATA%\wxdown`（cookies.json / settings.json / wx_debug.log）。
