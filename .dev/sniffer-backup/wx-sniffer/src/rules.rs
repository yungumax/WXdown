//! 嗅探规则：URL/来源分类。

/// 一条媒体命中的分类。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaKind {
    Video,
    Live,
    Image,
    Audio,
}

impl MediaKind {
    pub fn label(&self) -> &'static str {
        match self {
            MediaKind::Video => "视频",
            MediaKind::Live => "直播流",
            MediaKind::Image => "图片",
            MediaKind::Audio => "音频",
        }
    }

    /// 下载时的默认扩展名（URL 上认不出时兜底）
    pub fn default_ext(&self) -> &'static str {
        match self {
            MediaKind::Video => "mp4",
            MediaKind::Live => "flv",
            MediaKind::Image => "jpg",
            MediaKind::Audio => "mp3",
        }
    }
}

/// URL → 媒体分类。匹配保守（扩展名/明确路径特征），宁可漏不可吵：
/// 误报会把整个代理日志灌满，漏掉的资源用户还能手动复制地址。
pub fn classify_media(url: &str) -> Option<MediaKind> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let lower = path.to_ascii_lowercase();

    // 直播流优先认（.flv/.m3u8 都可能是直播）
    if lower.ends_with(".m3u8") || lower.ends_with(".flv") {
        return Some(MediaKind::Live);
    }
    if lower.ends_with(".mp4")
        || lower.ends_with(".m4v")
        || lower.ends_with(".mov")
        || lower.ends_with(".ts")
        || lower.ends_with(".mpd")
    {
        return Some(MediaKind::Video);
    }
    if lower.ends_with(".mp3")
        || lower.ends_with(".m4a")
        || lower.ends_with(".aac")
        || lower.ends_with(".amr")
        || lower.ends_with(".wav")
    {
        return Some(MediaKind::Audio);
    }
    // 小程序 CDN 常见无扩展名的视频路径：/video/ 段 + 长十六进制/哈希名
    if is_media_cdn_video(url) {
        return Some(MediaKind::Video);
    }
    if lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".png")
        || lower.ends_with(".webp")
        || lower.ends_with(".gif")
        || lower.ends_with(".bmp")
    {
        // 文章图 CDN（mmbiz）走正常文章下载,嗅探里排除,免得刷屏
        if url.contains("mmbiz.qpic.cn") || url.contains("mmbiz.qlogo.cn") {
            return None;
        }
        // 微信生态里的静态 UI 小图也排除
        if url.contains("res.wx.qq.com") && !url.contains("/video/") {
            return None;
        }
        return Some(MediaKind::Image);
    }
    None
}

/// 微信系视频 CDN 的无扩展名视频（形如 cdn.mp.weixin.qq.com/.../video/xxx）。
fn is_media_cdn_video(url: &str) -> bool {
    if !url.contains(".qq.com") && !url.contains("weixin") && !url.contains("wechat") {
        return false;
    }
    let path = url.split(['?', '#']).next().unwrap_or(url);
    if !path.contains("/video/") && !path.contains("/live/") {
        return false;
    }
    // 路径段要有实质内容（排除 /video/ 空挂）
    path.rsplit('/').next().map(|s| s.len() >= 8).unwrap_or(false)
}

/// 是否是公众号文章列表接口（微信客户端浏览历史文章时自己发出的请求）。
pub fn is_profile_ext(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    if !path.contains("/mp/profile_ext") {
        return false;
    }
    url.contains("action=getmsg") || url.contains("action=home")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_forms() {
        assert_eq!(classify_media("https://cdn.example.com/a/b.mp4?token=1"), Some(MediaKind::Video));
        assert_eq!(classify_media("https://x/live/abc.m3u8"), Some(MediaKind::Live));
        assert_eq!(classify_media("https://x/a.mp3"), Some(MediaKind::Audio));
        assert_eq!(classify_media("https://x/img/abc.jpeg"), Some(MediaKind::Image));
        // 文章图 CDN 排除
        assert_eq!(classify_media("https://mmbiz.qpic.cn/mmbiz_jpg/a/640?wx_fmt=jpeg"), None);
        // 无扩展名小程序视频
        assert_eq!(
            classify_media("https://mpvideo.qpic.cn/abc123def456/1000?format=204"),
            None,
        );
        assert_eq!(
            classify_media("https://cdn.mp.weixin.qq.com/channel/8a3f/video/1a2b3c4d5e"),
            Some(MediaKind::Video)
        );
        // 非媒体
        assert_eq!(classify_media("https://x.com/api/data.json"), None);
        assert_eq!(classify_media("https://x.com/page"), None);
    }

    #[test]
    fn profile_ext_detect() {
        assert!(is_profile_ext(
            "https://mp.weixin.qq.com/mp/profile_ext?action=getmsg&__biz=MzA&f=json&offset=0&count=10"
        ));
        assert!(is_profile_ext(
            "https://mp.weixin.qq.com/mp/profile_ext?action=home&__biz=MzA"
        ));
        assert!(!is_profile_ext("https://mp.weixin.qq.com/s/abc"));
    }
}
