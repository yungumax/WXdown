//! 公众号文章页解析：标题/公众号/时间/正文/图片/视频/语音。
//!
//! 文章页是服务端渲染的 HTML（桌面 UA 直取即可），正文在
//! `#js_content`（rich_media_content），图片挂 `data-src`（mmbiz 域，
//! wx_fmt 参数给格式），视频是 `iframe.video_iframe`（腾讯视频或
//! 微信原生 wxv_），语音是 `<mpvoice voice_encode_fileid=…>`。
//!
//! 解析全部用正则与字符串扫描（不引 DOM 依赖）：微信页面结构多年稳定，
//! 按已知标记取值 + 多级兜底，对改版的容错比整树反序列化好。

use crate::error::{Result, WxError};

/// 文章里的一张图（文档顺序，已去重）。
#[derive(Debug, Clone)]
pub struct ImageRef {
    pub url: String,
    /// jpg / png / gif / webp …（来自 wx_fmt 或 data-type，兜底 jpg）
    pub fmt: String,
}

/// 文章里的一个视频。
#[derive(Debug, Clone)]
pub struct VideoRef {
    /// tencent（腾讯视频 iframe）/ wxv（微信原生视频）/ other
    pub kind: String,
    /// iframe 的 data-src（解析播放地址的入口）
    pub src: String,
    /// 提取到的视频 ID（wxv_xxx / 腾讯 vid），可能为空
    pub vid: String,
}

/// 文章里的一段语音。
#[derive(Debug, Clone)]
pub struct AudioRef {
    pub voice_id: String,
    pub name: String,
}

/// 一篇文章解析出的干净结构。
#[derive(Debug, Clone, Default)]
pub struct ArticleData {
    pub title: String,
    /// 公众号名
    pub account: String,
    /// 作者名（文章内署名，常为空）
    pub author: String,
    /// 摘要
    pub digest: String,
    /// 发布时间文本（如 2026-01-02 12:34）
    pub publish_text: String,
    /// 发布时间 Unix 秒（解析失败为 0）
    pub publish_ts: i64,
    pub biz: String,
    pub sn: String,
    pub mid: String,
    pub idx: String,
    /// 文章地址（归一后的完整链接）
    pub url: String,
    /// 封面图（msg_cdn_url / og:image）
    pub cover: String,
    /// 正文纯文本（段落保留）
    pub text: String,
    /// 正文图片（文档顺序）
    pub images: Vec<ImageRef>,
    /// 正文视频
    pub videos: Vec<VideoRef>,
    /// 正文语音
    pub audios: Vec<AudioRef>,
    /// 动态类内容（无 js_content 的空壳页：文字/配图全靠 JS 在客户端内渲染，
    /// 外部只能从 og:title 或合集接口拿到文字，配图拿不到）
    pub is_dynamic: bool,
    /// 文章所属合集（页面内嵌的 appmsgalbum 链接里抠出；不属于合集为空）
    pub album_biz: String,
    pub album_id: String,
}

impl ArticleData {
    pub fn has_media(&self) -> bool {
        !self.images.is_empty() || !self.videos.is_empty() || !self.audios.is_empty()
    }
}

/// 解析文章页 HTML。
pub fn parse_article(html: &str, url: &str) -> Result<ArticleData> {
    if html.contains("该内容已被发布者删除")
        || html.contains("此内容因违规无法查看")
        || html.contains("该内容暂时无法查看")
    {
        return Err(WxError::Unavailable(
            "文章不可用：已被发布者删除或因违规无法查看".into(),
        ));
    }
    if html.contains("请在微信客户端打开链接") && !html.contains("id=\"js_content\"") {
        return Err(WxError::Unavailable(
            "此链接要求在微信客户端内打开（可能是受限分享）".into(),
        ));
    }

    let mut data = ArticleData {
        url: url.to_string(),
        ..Default::default()
    };

    // ── 标题：#activity-name 文本 → og:title → <title> ──
    data.title = element_text_by_id(html, "activity-name")
        .or_else(|| meta_content(html, "og:title"))
        .or_else(|| title_tag(html))
        .unwrap_or_default();
    data.title = decode_entities(&data.title);
    if data.title.len() > 120 {
        // og:title 有时被塞进整段正文（纯文字分享卡）：截断做标题
        data.title = data.title.chars().take(60).collect::<String>() + "…";
    }
    data.title = data.title.trim().to_string();

    // ── 公众号名：var nickname → #js_name → profile_nickname ──
    data.account = js_var_string(html, "nickname")
        .or_else(|| element_text_by_id(html, "js_name"))
        .or_else(|| {
            tag_span_inner(html, "profile_nickname")
        })
        .map(|s| decode_entities(&s).trim().to_string())
        .unwrap_or_default();

    // ── 作者：var author / #js_author_name ──
    data.author = js_var_string(html, "author")
        .or_else(|| element_text_by_id(html, "js_author_name"))
        .map(|s| decode_entities(&s).trim().to_string())
        .unwrap_or_default();

    data.digest = js_var_string(html, "msg_desc")
        .or_else(|| meta_content(html, "og:description"))
        .map(|s| decode_entities(&s))
        .unwrap_or_default();

    // ── 发布时间：#publish_time 文本（2026-01-02 12:34）→ var ct ──
    if let Some(text) = element_text_by_id(html, "publish_time") {
        data.publish_text = decode_entities(&text).trim().to_string();
    }
    if data.publish_text.is_empty() {
        if let Some(ct) = js_var_string(html, "ct") {
            data.publish_ts = ct.trim().parse().unwrap_or(0);
            data.publish_text = format_datetime_local(data.publish_ts);
        }
    }
    if data.publish_ts == 0 && !data.publish_text.is_empty() {
        data.publish_ts = parse_datetime_local(&data.publish_text);
    }

    // ── 标识：biz / sn / mid / idx ──
    data.biz = js_var_string(html, "biz").unwrap_or_default();
    if data.biz.is_empty() {
        if let Some(v) = first_capture(html, r#"__biz=([A-Za-z0-9+/=]{10,})"#) {
            data.biz = v;
        }
    }
    if let Some(link) = js_var_string(html, "msg_link") {
        let link = decode_entities(&link);
        let query = link.split_once('?').map(|(_, q)| q).unwrap_or("");
        data.sn = crate::parser::query_value(query, "sn").unwrap_or_default();
        data.mid = crate::parser::query_value(query, "mid").unwrap_or_default();
        data.idx = crate::parser::query_value(query, "idx").unwrap_or_default();
    }

    data.cover = js_var_string(html, "msg_cdn_url")
        .or_else(|| meta_content(html, "og:image"))
        .map(|s| decode_entities(&s))
        .unwrap_or_default();

    // ── 正文：#js_content 的内层 HTML ──
    let content_html = js_content_html(html).unwrap_or_default();
    data.is_dynamic = content_html.is_empty() && !data.title.is_empty();

    // ── 所属合集（内嵌链接，两种转义形态都认）──
    if let Some((biz, album_id)) = embedded_album(html) {
        data.album_biz = biz;
        data.album_id = album_id;
    }

    // 图片与视频从正文 HTML 抽（正文拿不到时退回全页抽，兜住改版）
    let scope = if content_html.is_empty() { html } else { &content_html };
    data.images = extract_images(scope);
    data.videos = extract_videos(scope);
    data.audios = extract_audios(scope);

    data.text = if content_html.is_empty() {
        // 纯文字分享卡：正文就是 og:title/digest 的长文本
        let long = meta_content(html, "og:title").unwrap_or_default();
        if long.chars().count() > 60 {
            decode_entities(&long)
        } else {
            data.digest.clone()
        }
    } else {
        html_to_text(&content_html)
    };

    if data.title.is_empty() && data.text.is_empty() && data.images.is_empty() {
        return Err(WxError::Api(
            "文章页解析不到内容（可能页面改版或链接失效）".into(),
        ));
    }
    Ok(data)
}

/// 取 `id="xxx"` 元素的开始/结束标签之间的文本（剥一层嵌套标签）。
/// regex crate 不支持向后引用（`</\1>`），开标签与同名闭标签分两步找。
fn element_text_by_id(html: &str, id: &str) -> Option<String> {
    let re = regex::Regex::new(&format!(
        r#"(?si)<(\w+)\b[^>]*\bid="{}"[^>]*>"#,
        regex::escape(id)
    ))
    .unwrap();
    let caps = re.captures(html)?;
    let tag = caps.get(1)?.as_str().to_lowercase();
    let open_end = caps.get(0).unwrap().end();
    let close = format!("</{tag}");
    let close_at = html[open_end..].find(&close)? + open_end;
    let inner = &html[open_end..close_at];
    let text = strip_tags(inner);
    let text = decode_entities(&text);
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// 取 <meta property="…" content="…"> / <meta name="…" content="…">。
fn meta_content(html: &str, key: &str) -> Option<String> {
    let re = regex::Regex::new(&format!(
        r#"(?si)<meta[^>]*(?:property|name)=["']{}["'][^>]*content=["']([^"']*)["']"#,
        regex::escape(key)
    ))
    .unwrap();
    re.captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .or_else(|| {
            // content 在前、property 在后的顺序也支持
            let re2 = regex::Regex::new(&format!(
                r#"(?si)<meta[^>]*content=["']([^"']*)["'][^>]*(?:property|name)=["']{}["']"#,
                regex::escape(key)
            ))
            .unwrap();
            re2.captures(html)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
        })
}

/// <title> 文本。
fn title_tag(html: &str) -> Option<String> {
    let re = regex::Regex::new(r"(?s)<title[^>]*>(.*?)</title>").unwrap();
    re.captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
}

/// `var name = htmlDecode("…")` / `var name = "…"` 的字符串值。
fn js_var_string(html: &str, name: &str) -> Option<String> {
    let re = regex::Regex::new(&format!(
        r#"(?s)var\s+{}\s*=\s*(?:htmlDecode\()?\s*["']([^"']*)["']"#,
        regex::escape(name)
    ))
    .unwrap();
    re.captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .filter(|s| !s.trim().is_empty())
}

/// class 含 xxx 的标签内层文本（如 profile_nickname）。
/// 同名闭标签分两步找（regex crate 无向后引用）。
fn tag_span_inner(html: &str, class: &str) -> Option<String> {
    let re = regex::Regex::new(&format!(
        r#"(?si)<(\w+)\b[^>]*class="[^"]*{}[^"]*"[^>]*>"#,
        regex::escape(class)
    ))
    .unwrap();
    let caps = re.captures(html)?;
    let tag = caps.get(1)?.as_str().to_lowercase();
    let open_end = caps.get(0).unwrap().end();
    let close = format!("</{tag}");
    let close_at = html[open_end..].find(&close)? + open_end;
    let inner = &html[open_end..close_at];
    let text = strip_tags(inner);
    let text = decode_entities(&text);
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// 抽 `#js_content` 元素的外层 HTML（含标签自身）。
/// 用 div 配平扫描找闭合标签：微信正文嵌套极深，正则到 `</div>` 会截半。
pub fn js_content_html(html: &str) -> Option<String> {
    let marker = regex::Regex::new(r#"<div\b[^>]*\bid="js_content"[^>]*>"#).unwrap();
    let open = marker.find(html)?;
    let start = open.start();
    // 配平：从自身这个 <div …> 开始计数
    let div_open = regex::Regex::new(r"(?i)<div\b").unwrap();
    let div_close = regex::Regex::new(r"(?i)</div").unwrap();
    let mut depth = 0usize;
    let mut pos = start;
    // 逐个 token 扫描（<div 开标签 / </div 闭标签）
    let mut next_open = div_open.find_at(html, pos);
    let mut next_close = div_close.find_at(html, pos);
    loop {
        match (next_open, next_close) {
            (Some(o), Some(c)) if o.start() < c.start() => {
                depth += 1;
                pos = o.end();
                next_open = div_open.find_at(html, pos);
            }
            (_, Some(c)) => {
                depth = depth.saturating_sub(1);
                pos = c.end();
                next_close = div_close.find_at(html, pos);
                if depth == 0 {
                    // 找到闭合 </div> 的结束
                    let end = html[pos..].find('>')? + pos + 1;
                    return Some(html[start..end].to_string());
                }
            }
            _ => return None,
        }
    }
}

/// 抽正文图片：`<img … data-src="…">`（文档顺序，URL 去重）。
pub fn extract_images(scope: &str) -> Vec<ImageRef> {
    let img_re = regex::Regex::new(r#"(?si)<img\b[^>]*>"#).unwrap();
    let mut out: Vec<ImageRef> = Vec::new();
    for tag in img_re.captures_iter(scope) {
        let tag = tag.get(0).unwrap().as_str();
        let attr = |name: &str| -> Option<String> {
            let re = regex::Regex::new(&format!(
                r#"(?i)\b{name}="([^"]*)""#,
                name = regex::escape(name)
            ))
            .unwrap();
            re.captures(tag)
                .and_then(|c| c.get(1))
                .map(|m| decode_entities(m.as_str()))
                .filter(|s| !s.is_empty())
        };
        let Some(url) = attr("data-src") else { continue };
        if !url.starts_with("http") {
            continue;
        }
        let query = url.split_once('?').map(|(_, q)| q).unwrap_or("");
        let fmt = crate::parser::query_value(query, "wx_fmt")
            .or_else(|| attr("data-type"))
            .unwrap_or_else(|| {
                // 路径里 mmbiz_jpg / mmbiz_png 的兜底
                if url.contains("mmbiz_png") {
                    "png".into()
                } else if url.contains("mmbiz_gif") {
                    "gif".into()
                } else {
                    "jpg".into()
                }
            })
            .to_ascii_lowercase();
        if out.iter().any(|img| img.url == url) {
            continue;
        }
        out.push(ImageRef { url, fmt });
    }
    out
}

/// 抽正文视频：`<iframe class="video_iframe …" data-src="…">`（含普通 <video src>）。
pub fn extract_videos(scope: &str) -> Vec<VideoRef> {
    let iframe_re = regex::Regex::new(r#"(?si)<iframe\b[^>]*>|<video\b[^>]*>"#).unwrap();
    let mut out: Vec<VideoRef> = Vec::new();
    for tag in iframe_re.captures_iter(scope) {
        let tag = tag.get(0).unwrap().as_str();
        let attr = |name: &str| -> Option<String> {
            let re = regex::Regex::new(&format!(r#"(?i)\b{}="([^"]*)""#, regex::escape(name))).unwrap();
            re.captures(tag)
                .and_then(|c| c.get(1))
                .map(|m| decode_entities(m.as_str()))
                .filter(|s| !s.is_empty())
        };
        let src = attr("data-src").or_else(|| attr("src")).unwrap_or_default();
        if src.is_empty() || !src.starts_with("http") {
            continue;
        }
        let is_video = tag.to_lowercase().contains("video_iframe")
            || tag.to_lowercase().starts_with("<video")
            || src.contains("v.qq.com")
            || src.contains("readtemplate")
            || src.contains("wxv_");
        if !is_video {
            continue;
        }
        let kind = if src.contains("v.qq.com") {
            "tencent"
        } else if src.contains("wxv_") || src.contains("readtemplate") || src.contains("/mp/video") {
            "wxv"
        } else {
            "other"
        };
        let vid = if kind == "tencent" {
            first_capture(&src, r"vid=([A-Za-z0-9]+)").unwrap_or_default()
        } else {
            first_capture(&src, r"(wxv_[A-Za-z0-9]+)").unwrap_or_default()
        };
        if out.iter().any(|v| v.src == src) {
            continue;
        }
        out.push(VideoRef { kind: kind.to_string(), src, vid });
    }
    out
}

/// 抽正文语音：`<mpvoice voice_encode_fileid="…">`。
pub fn extract_audios(scope: &str) -> Vec<AudioRef> {
    let re = regex::Regex::new(r#"(?si)<mpvoice\b[^>]*>"#).unwrap();
    let mut out: Vec<AudioRef> = Vec::new();
    for tag in re.captures_iter(scope) {
        let tag = tag.get(0).unwrap().as_str();
        let attr = |name: &str| -> Option<String> {
            let re = regex::Regex::new(&format!(r#"(?i)\b{}="([^"]*)""#, regex::escape(name))).unwrap();
            re.captures(tag)
                .and_then(|c| c.get(1))
                .map(|m| decode_entities(m.as_str()))
        };
        let Some(voice_id) = attr("voice_encode_fileid").filter(|s| !s.is_empty()) else {
            continue;
        };
        if out.iter().any(|a| a.voice_id == voice_id) {
            continue;
        }
        out.push(AudioRef {
            voice_id,
            name: attr("name").unwrap_or_default(),
        });
    }
    out
}

/// 正文 HTML → 纯文本：块级标签换行、剥标签、解码实体。
pub fn html_to_text(html: &str) -> String {
    // 去 script/style（regex crate 无向后引用，分开写两条）
    let re_script = regex::Regex::new(r"(?si)<script\b[^>]*>.*?</script>").unwrap();
    let re_style = regex::Regex::new(r"(?si)<style\b[^>]*>.*?</style>").unwrap();
    let cleaned = re_script.replace_all(html, "");
    let cleaned = re_style.replace_all(&cleaned, "");
    // 块级边界 → 换行（开/闭都算，<br> 系列）
    let re_nl = regex::Regex::new(
        r#"(?i)</?(p|div|section|blockquote|h[1-6]|li|tr|table|ul|ol|pre|figure|figcaption|hr)\b[^>]*>|<br\s*/?>"#,
    )
    .unwrap();
    let with_nl = re_nl.replace_all(&cleaned, "\n");
    let text = strip_tags(&with_nl);
    let text = decode_entities(&text);
    // 归一空行：连续空行折成一个，去掉行尾空白
    let mut out = String::with_capacity(text.len());
    let mut blank = 0;
    for line in text.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            blank += 1;
            if blank <= 1 {
                out.push('\n');
            }
        } else {
            blank = 0;
            out.push_str(line);
            out.push('\n');
        }
    }
    out.trim().to_string()
}

/// 剥掉全部标签。
pub fn strip_tags(html: &str) -> String {
    let re = regex::Regex::new(r"(?s)<[^>]*>").unwrap();
    re.replace_all(html, "").into_owned()
}

/// HTML 实体解码（命名 + 十进制 + 十六进制）。
pub fn decode_entities(input: &str) -> String {
    if !input.contains('&') {
        return input.to_string();
    }
    let re = regex::Regex::new(r"&(#[0-9]+|#[xX][0-9A-Fa-f]+|[A-Za-z][A-Za-z0-9]{1,30});").unwrap();
    re.replace_all(input, |caps: &regex::Captures| -> String {
        let body = caps.get(1).unwrap().as_str();
        if let Some(num) = body.strip_prefix('#') {
            let code = if let Some(hex) = num.strip_prefix('x').or_else(|| num.strip_prefix('X')) {
                u32::from_str_radix(hex, 16).ok()
            } else {
                num.parse::<u32>().ok()
            };
            if let Some(code) = code {
                if let Some(ch) = char::from_u32(code) {
                    return ch.to_string();
                }
            }
            return caps.get(0).unwrap().as_str().to_string();
        }
        match body {
            "amp" => "&",
            "lt" => "<",
            "gt" => ">",
            "quot" => "\"",
            "apos" => "'",
            "nbsp" => " ",
            "ensp" => " ",
            "emsp" => " ",
            "thinsp" => " ",
            "ldquo" => "\u{201C}",
            "rdquo" => "\u{201D}",
            "lsquo" => "\u{2018}",
            "rsquo" => "\u{2019}",
            "mdash" => "\u{2014}",
            "ndash" => "\u{2013}",
            "hellip" => "\u{2026}",
            "middot" => "\u{00B7}",
            "bull" => "\u{2022}",
            "times" => "\u{00D7}",
            "copy" => "\u{00A9}",
            "reg" => "\u{00AE}",
            "trade" => "\u{2122}",
            "zwnj" => "",
            "zwj" => "",
            "shy" => "",
            _ => return caps.get(0).unwrap().as_str().to_string(),
        }
        .to_string()
    })
    .into_owned()
}

/// 从文章页里抠内嵌的合集标识。
/// 页面里的形态（实测两种文章模板一致）：
/// 从文章页里抠内嵌的合集标识。
/// 页面里的形态（实测两种文章模板一致）：
/// `appmsgalbum?__biz=Mz…==\x26amp;action=getalbum\x26amp;album_id=17…#wechat_redirect`
/// （`\x26amp;` 是 JS 转义的 &；普通文章页还可能出 &amp; 形态，一并归一）。
pub fn embedded_album(html: &str) -> Option<(String, String)> {
    let start = html.find("appmsgalbum?")?;
    let end = html[start..]
        .find(|c: char| c == '"' || c == '\'' || c == '<' || c == ' ' || c == '#')
        .map(|i| start + i)
        .unwrap_or(html.len().min(start + 400));
    let raw = &html[start..end];
    let normalized = raw.replace(r"\x26amp;", "&").replace("&amp;", "&");
    let query = normalized.split_once('?').map(|(_, q)| q).unwrap_or("");
    let biz = crate::parser::query_value(query, "__biz")?;
    let album_id = crate::parser::query_value(query, "album_id")?;
    (!biz.is_empty() && !album_id.is_empty()).then_some((biz, album_id))
}

fn first_capture(text: &str, pattern: &str) -> Option<String> {
    let re = regex::Regex::new(pattern).ok()?;
    re.captures(text).and_then(|c| c.get(1)).map(|m| m.as_str().to_string())
}

/// "2026-01-02 12:34"（北京时间）→ Unix 秒；解析失败返回 0。
pub fn parse_datetime_local(text: &str) -> i64 {
    let re = regex::Regex::new(r"(\d{4})[-/年](\d{1,2})[-/月](\d{1,2})").unwrap();
    let Some(c) = re.captures(text) else { return 0 };
    let y: i64 = c.get(1).unwrap().as_str().parse().unwrap_or(0);
    let m: i64 = c.get(2).unwrap().as_str().parse().unwrap_or(1);
    let d: i64 = c.get(3).unwrap().as_str().parse().unwrap_or(1);
    let (h, min) = {
        let re2 = regex::Regex::new(r"(\d{1,2}):(\d{2})").unwrap();
        match re2.captures(text) {
            Some(c2) => (
                c2.get(1).unwrap().as_str().parse().unwrap_or(0),
                c2.get(2).unwrap().as_str().parse().unwrap_or(0),
            ),
            None => (0, 0),
        }
    };
    if y < 2000 {
        return 0;
    }
    let days = days_from_civil(y, m, d);
    days * 86400 + h * 3600 + min * 60 - 8 * 3600
}

/// Unix 秒 → "YYYY-MM-DD HH:MM"（北京时间）。
pub fn format_datetime_local(secs: i64) -> String {
    let (y, m, d) = civil_from_unix(secs + 8 * 3600);
    let secs_of_day = (secs + 8 * 3600).rem_euclid(86400);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60
    )
}

/// Howard Hinnant 的 civil 算法：年月日 → 自纪元天数。
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// 自纪元天数 → 年月日（civil_from_unix 的日期部分）。
fn civil_from_unix(secs: i64) -> (i64, i64, i64) {
    let days = secs.div_euclid(86400);
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
    <html><head>
    <meta property="og:title" content="示例文章标题"/>
    <meta property="og:description" content="这是摘要"/>
    </head><body>
    <h1 class="rich_media_title" id="activity-name">  示例文章标题 </h1>
    <div class="profile_meta"><span class="profile_nickname">示例公众号</span></div>
    <em id="publish_time" class="rich_media_meta_text">2026-01-02 12:34</em>
    <div class="rich_media_content" id="js_content">
      <p>第一段</p>
      <section><p>第二段&amp;更多</p>
        <img class="rich_pages" data-src="https://mmbiz.qpic.cn/mmbiz_jpg/ABC/640?wx_fmt=jpeg" data-type="jpeg"/>
        <img data-src="https://mmbiz.qpic.cn/mmbiz_png/DEF/640?wx_fmt=png"/>
        <iframe class="video_iframe rich_pages" data-src="https://v.qq.com/iframe/preview.html?vid=x0012abc&amp;width=500"></iframe>
        <mpvoice voice_encode_fileid="MzA3" name="语音1"></mpvoice>
      </section>
      <div>嵌套一层<div>再嵌套</div>尾部文本</div>
    </div>
    <script>var nickname = htmlDecode("示例公众号"); var biz = "MzIzOTU="; var ct = "1767336840";</script>
    </body></html>
    "#;

    #[test]
    fn parses_sample_article() {
        let data = parse_article(SAMPLE, "https://mp.weixin.qq.com/s/abc").unwrap();
        assert_eq!(data.title, "示例文章标题");
        assert_eq!(data.account, "示例公众号");
        assert_eq!(data.digest, "这是摘要");
        assert_eq!(data.images.len(), 2);
        assert_eq!(data.images[0].fmt, "jpeg");
        assert_eq!(data.images[1].fmt, "png");
        assert_eq!(data.videos.len(), 1);
        assert_eq!(data.videos[0].kind, "tencent");
        assert_eq!(data.videos[0].vid, "x0012abc");
        assert_eq!(data.audios.len(), 1);
        assert_eq!(data.audios[0].voice_id, "MzA3");
        assert_eq!(data.biz, "MzIzOTU=");
        assert!(data.text.contains("第一段"));
        assert!(data.text.contains("尾部文本"), "配平扫描要取到嵌套后的文本：{}", data.text);
    }

    #[test]
    fn datetime_roundtrip() {
        let ts = parse_datetime_local("2026-01-02 12:34");
        assert!(ts > 0);
        assert_eq!(format_datetime_local(ts), "2026-01-02 12:34");
        assert_eq!(parse_datetime_local("不是日期"), 0);
    }

    #[test]
    fn deleted_article_is_error() {
        let err = parse_article("<html>该内容已被发布者删除</html>", "u").unwrap_err();
        assert!(err.to_string().contains("不可用"));
    }

    #[test]
    fn embedded_album_both_escape_forms() {
        // JS 转义形态（动态页与普通页一致）
        let js_escaped = r#"var a = "appmsgalbum?__biz=MzYzOTU1NTUzNw==&amp;action=getalbum&amp;album_id=4319761724883959819#wechat_redirect";"#;
        assert_eq!(
            embedded_album(js_escaped).unwrap(),
            ("MzYzOTU1NTUzNw==".to_string(), "4319761724883959819".to_string())
        );
        // HTML 转义形态
        let html_escaped = r#"<a href="/mp/appmsgalbum?__biz=MzIzOA==&amp;album_id=99">合集</a>"#;
        assert_eq!(
            embedded_album(html_escaped).unwrap(),
            ("MzIzOA==".to_string(), "99".to_string())
        );
        // 没有合集
        assert!(embedded_album("<html>无合集文章</html>").is_none());
    }

    #[test]
    fn entities() {
        assert_eq!(decode_entities("a&amp;b&lt;c&#39;d&#x41;"), "a&b<c'dA");
        assert_eq!(decode_entities("&nbsp;X&hellip;"), " X…");
    }
}
