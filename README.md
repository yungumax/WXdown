# WXdown

微信公众号内容下载工具（Windows 桌面端）。界面沿用
[BILIdown](https://github.com/yungumax) 家族（BILIdown / WeBodown / XHSdown）的
设计体系，主题色为微信绿。

- **技术栈**：Tauri 2 · Rust · Vue 3（前端 Vite 构建）
- **下载能力**：每篇文章一个文件夹——`正文.txt`（标题/作者/时间/原文链接 + 全文）、
  `01.jpg…` 序号原图、`video-*.mp4` / `voice-*.mp3`（尽力解析直链）、
  `index.html`（图片本地化的离线原版式存档，双击可看）；
  并发下载 · 暂停与断点续传（`.part` 续传）· 重名自动跳过
- **内容来源**：公众号文章链接（微信里「复制链接」可整段粘贴）· 合集链接 ·
  **贴任意一篇属于合集的文章会自动展开整个合集**（B 站式体验）
- **目录结构**：默认「公众号名 / 合集名 / 序号 标题」，模板可在设置里改
- **自动更新**：`tauri-plugin-updater` + GitHub Releases（签名校验，
  端点 `releases/latest/download/latest.json`）

## 使用说明

1. 安装后首次打开进入引导页，设置保存目录。
2. 「解析」页粘贴文章或合集链接（一行一个，可批量）→ 开始解析 →
   合集点「解析全部」翻完 → 勾选 → 下载所选。
3. 文章和合集**无需登录**。少数"文字+视频"类动态的配图只在微信客户端内下发，
   工具会下载其文字部分并在正文里注明。

## 边界说明（为什么没有"公众号全部文章"）

微信于 2026 年 7 月关闭了公众平台的文章枚举接口（`appmsg` / `appmsgpublish`
全社区同步失效），"登录后枚举任意公众号全部文章"这条路对所有工具都已关闭。
本工具的替代方案是**文章自动展开合集**——持续更新的公众号内容大多组织在合集里，
贴一篇即可带出整个合集。平台登录设施保留在代码中，若微信重开接口可快速接回。

## 开发与构建

```bash
npm install                 # 前端依赖
npm run build               # 构建前端到 dist/
# 编译桌面端（需要 MSVC 链接环境；Git Bash 下先 source 便携工具链）
source scripts/msvc-env.sh
cargo build --features custom-protocol          # 调试版 → target/debug/wxdown.exe
npx tauri build                                  # 发布版 → NSIS 安装包 + 更新包
```

注意构建顺序：**先 `npm run build` 再 `cargo build`**（反了会把旧 dist 嵌进去）。

Windows PowerShell / cmd 用户可直接用 `dev.bat`（自动装配便携 MSVC 链接环境）。

## 发布与自动更新

仓库发布流程（`.github/workflows/release.yml`）：推送 `v*` 标签 →
CI 构建 NSIS 安装包 → 用私钥签名 → 建 Release 并上传
`setup.exe` / `.sig` / `latest.json`。

应用内「设置 · 应用更新」检测的就是该 Release 的 `latest.json`；安装包有签名校验，
密钥不匹配会拒绝安装。

**首次配置（只需一次）**：在 GitHub 仓库 `Settings → Secrets and variables → Actions`
添加 `TAURI_SIGNING_PRIVATE_KEY`，值为本地签名私钥文件内容：
`D:\Zcode\_data\tauri-keys\wxdown.key`（**该私钥不入库**，丢失后已发布的更新包将无法被旧版本接受）。
公钥已写入 `src-tauri/tauri.conf.json`。

## 项目结构

```
crates/wx-core/       # 核心库：会话、文章页解析(article)、合集接口(album)、
                      # 视频语音直链(api)、下载引擎(download)
src-tauri/            # Tauri 壳与命令层（commands.rs 的 run_task 是文章下载管线）
src/                  # Vue 3 前端（pages/ 各栏目，components/ 通用件）
scripts/              # 图标生成、便携 MSVC 链接环境、页面结构验证脚本
.dev/sniffer-backup/  # 已移除的本地代理嗅探方案备份（未编译进应用）
```

接口踩坑记录（合集 getalbum、动态页兜底、视频解析等）见 `AGENTS.md`。

## 免责声明

仅供个人学习与离线收藏使用。请尊重作者版权，不要用于传播或商业用途。
