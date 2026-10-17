//! 接口层：文章详情抓取 + 视频/语音直链解析（尽力而为）。
//!
//! 文章页免登录直取（见 article.rs）。视频两类来源：
//! - wxv_（微信原生）：readtemplate 页/ mp/video 接口里抠 mp4 直链
//! - v.qq.com（腾讯视频）：iframe 页与 getinfo 接口轮流试
//! 解析不出时上层降级为"记录占位"，不算任务失败。

use crate::article::{parse_article, ArticleData, VideoRef};
use crate::client::WxClient;
use crate::error::Result;

impl WxClient {
    /// 抓取文章页：返回（原始 HTML, 解析结果）。原始 HTML 供离线存档用。
    /// 短链（/s/xxx）会被 302 到最终地址，fetch_text 自动跟随。
    pub async fn fetch_article(&self, url: &str) -> Result<(String, ArticleData)> {
        let html = self.fetch_text(url, crate::client::SITE_REFERER).await?;
        let data = parse_article(&html, url)?;
        Ok((html, data))
    }

    /// 抓取并解析一篇文章（只要解析结果时用这个）。
    pub async fn article_detail(&self, url: &str) -> Result<ArticleData> {
        self.fetch_article(url).await.map(|(_, data)| data)
    }

    /// 尽力解析视频直链（mp4）。失败返回 None，上层记占位不失败。
    pub async fn resolve_video(&self, video: &VideoRef) -> Option<String> {
        match video.kind.as_str() {
            "wxv" => self.resolve_wxv(video).await,
            "tencent" => self.resolve_tencent(video).await,
            _ => {
                // 未知 iframe：直接抓页面找 mp4
                let html = self.fetch_text(&video.src, crate::client::SITE_REFERER).await.ok()?;
                    first_mp4(&html)
            }
        }
    }

    /// 微信原生视频（wxv_）：readtemplate 页里通常内嵌 mp4 地址。
    async fn resolve_wxv(&self, video: &VideoRef) -> Option<String> {
        // 入口一：iframe 的 data-src（readtemplate）
        if let Ok(html) = self.fetch_text(&video.src, crate::client::SITE_REFERER).await {
            if let Some(url) = first_mp4(&html) {
                return Some(url);
            }
        }
        // 入口二：vid 已知时直接打 video 接口
        if !video.vid.is_empty() {
            let url = format!(
                "https://mp.weixin.qq.com/mp/video?action=get_video_info&vid={}&format=json",
                video.vid
            );
            if let Ok(text) = self.fetch_text(&url, crate::client::SITE_REFERER).await {
                if let Some(u) = first_mp4(&text) {
                    return Some(u);
                }
                // 接口可能回 JSON：{"url":"..."} 或 jsonp
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim()) {
                    if let Some(u) = value
                        .pointer("/url")
                        .and_then(|v| v.as_str())
                        .filter(|s| s.contains(".mp4"))
                    {
                        return Some(u.to_string());
                    }
                }
            }
        }
        None
    }

    /// 腾讯视频：先试 iframe 页内嵌数据，再试 getinfo 接口。
    async fn resolve_tencent(&self, video: &VideoRef) -> Option<String> {
        if let Ok(html) = self.fetch_text(&video.src, crate::client::SITE_REFERER).await {
            if let Some(url) = first_mp4(&html) {
                return Some(url);
            }
        }
        let vid = (!video.vid.is_empty()).then(|| video.vid.clone())?;
        let url = format!(
            "https://vv.video.qq.com/getinfo?vids={vid}&encver=2&charge=0&platform=101001&otype=json&defnpayver=1&format=json"
        );
        if let Ok(text) = self.fetch_text(&url, "https://v.qq.com/").await {
            // jsonp 包裹：QZOutputJson= {...};
            let body = text
                .find('{')
                .map(|start| &text[start..text.rfind('}').map(|e| e + 1).unwrap_or(text.len())])
                .unwrap_or(text.as_str());
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(body) {
                // vl.vi[0].ul.ui[0].url + fn 拼接
                let base = value
                    .pointer("/vl/vi/0/ul/ui/0/url")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let file = value
                    .pointer("/vl/vi/0/fn")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if !base.is_empty() && !file.is_empty() {
                    return Some(format!("https://{base}/{file}"));
                }
            }
        }
        None
    }

    /// 语音直链（res.wx.qq.com/voice/getvoice，实测免登录可用）。
    pub fn audio_url(voice_id: &str) -> String {
        format!("https://res.wx.qq.com/voice/getvoice?mediaid={voice_id}")
    }
}

/// 从文本里抠第一个 mp4 直链（容错 json 转义与 &amp;）。
fn first_mp4(text: &str) -> Option<String> {
    // JSON 里常见的 \/ 转义先归一
    let normalized = text.replace("\\/", "/");
    let re = regex::Regex::new(r#"https?://[^"'\s\\<>]+?\.mp4[^"'\s\\<>]*"#).unwrap();
    let hit = re.find(&normalized)?;
    Some(hit.as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_mp4_in_variants() {
        assert_eq!(
            first_mp4(r#"var a = "https://example.com/v/abc.mp4?sign=1";"#),
            Some("https://example.com/v/abc.mp4?sign=1".into())
        );
        assert_eq!(
            first_mp4(r#"{"url":"https:\/\/cdn.example.com\/x.mp4?k=2"}"#),
            Some("https://cdn.example.com/x.mp4?k=2".into())
        );
        assert_eq!(first_mp4("没有视频地址"), None);
    }
}
