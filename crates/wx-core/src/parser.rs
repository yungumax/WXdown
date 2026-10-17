//! 输入解析：把用户粘贴的微信分享文本/链接归一成来源目标。
//!
//! 公众号文章的常见形态：
//! - `https://mp.weixin.qq.com/s/xxxxxxxx`（短链，分享出来最常见）
//! - `https://mp.weixin.qq.com/s?__biz=…&mid=…&idx=…&sn=…`（长链）
//! - 合集：`https://mp.weixin.qq.com/mp/appmsgalbum?__biz=…&album_id=…`
//! - 外层短链（如 weui 相关节选跳转）交给上层 resolve_redirect 后重试

use crate::error::{Result, WxError};

/// 一条输入归一后的来源目标。
#[derive(Debug, Clone, PartialEq)]
pub enum SourceTarget {
    /// 单篇文章（url 是文章完整地址）
    Article { url: String },
    /// 合集（biz 与 album_id 从查询串取）
    Album {
        biz: String,
        album_id: String,
        url: String,
    },
    /// 公众号名称查询（阶段二：登录公众平台后枚举该号全部文章）
    AccountQuery { name: String },
}

/// 解析一行输入。支持整段分享文本里混排的链接。
pub fn parse_input(input: &str) -> Result<SourceTarget> {
    let text = input.trim();
    if text.is_empty() {
        return Err(WxError::InvalidInput("链接是空的".into()));
    }

    // 从整段文本里抽 URL（分享文本里链接常混在文案中间）
    let url_re = regex::Regex::new(r#"https?://[^\s"'<>，,；;）)】\]]+"#).unwrap();
    let candidate = url_re
        .find(text)
        .map(|m| m.as_str())
        .unwrap_or(text.trim());

    let lowered = candidate.to_lowercase();
    let is_wx = lowered.contains("mp.weixin.qq.com");
    if !is_wx {
        // 外层跳转短链：无法离线判断目标，交由上层 resolve_redirect 后重试
        if lowered.contains("weixin.qq.com") || lowered.contains("wechat.com") {
            return Err(WxError::InvalidInput(format!("NEED_RESOLVE:{candidate}")));
        }
        // 没有链接的纯文本：当公众号名称查询（搜索由登录后的平台接口完成，
        // 找不到会得到明确报错，所以这里不设过多门槛）
        if !candidate.contains("://") {
            let name = text.trim();
            let len = name.chars().count();
            if (2..=40).contains(&len) {
                return Ok(SourceTarget::AccountQuery {
                    name: name.to_string(),
                });
            }
        }
        return Err(WxError::InvalidInput(format!(
            "认不出这个输入：{}（支持公众号文章/合集链接，或直接输入公众号名称）",
            crate::error::truncate(text, 60)
        )));
    }

    let path_part = candidate
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(candidate);
    let (path, query) = path_part.split_once('?').unwrap_or((path_part, ""));
    let segments: Vec<&str> = path
        .split('#')
        .next()
        .unwrap_or(path)
        .trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    // 合集页：/mp/appmsgalbum?__biz=…&album_id=…
    if segments.iter().any(|s| *s == "appmsgalbum") {
        let biz = query_value(query, "__biz").unwrap_or_default();
        let album_id = query_value(query, "album_id").unwrap_or_default();
        if !album_id.is_empty() {
            return Ok(SourceTarget::Album {
                biz,
                album_id,
                url: candidate.to_string(),
            });
        }
    }

    // 文章短链：/s/xxxxxxxx
    if let Some(i) = segments.iter().position(|s| *s == "s") {
        if let Some(id) = segments.get(i + 1) {
            if !id.is_empty() {
                return Ok(SourceTarget::Article {
                    url: candidate.to_string(),
                });
            }
        }
        // /s?__biz=…（长链，path 就是 /s，query 带全参）
        return Ok(SourceTarget::Article {
            url: candidate.to_string(),
        });
    }

    Err(WxError::InvalidInput(format!(
        "认不出这个链接：{}（支持公众号文章、合集链接）",
        crate::error::truncate(text, 60)
    )))
}

/// 从查询串里取第一个非空值（URL 解码）。
pub fn query_value(query: &str, key: &str) -> Option<String> {
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            if k.trim() == key && !v.trim().is_empty() {
                return Some(percent_decode(v.trim()));
            }
        }
    }
    None
}

/// 简易百分号解码（+ 当空格）。
pub fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            // %XX：需要 i+1、i+2 都是合法下标
            b'%' if i + 2 < bytes.len() => {
                let hex = &input[i + 1..i + 3];
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_short_article_link() {
        match parse_input("https://mp.weixin.qq.com/s/7FbNHbDzMnI9E5_-TBS22Q").unwrap() {
            SourceTarget::Article { url } => {
                assert!(url.contains("/s/7FbNHbDzMnI9E5_-TBS22Q"));
            }
            other => panic!("应为 Article，实际 {other:?}"),
        }
    }

    #[test]
    fn parses_long_article_link_in_share_text() {
        let input = "看看这篇文章 https://mp.weixin.qq.com/s?__biz=MzA==&mid=1&idx=1&sn=abc&chksm=xyz 很不错";
        match parse_input(input).unwrap() {
            SourceTarget::Article { url } => {
                assert!(url.starts_with("https://mp.weixin.qq.com/s?"));
            }
            other => panic!("应为 Article，实际 {other:?}"),
        }
    }

    #[test]
    fn parses_album_link() {
        match parse_input(
            "https://mp.weixin.qq.com/mp/appmsgalbum?__biz=MzYzOTU1NTUzNw==&album_id=4319761724883959819&count=10#wechat_redirect",
        )
        .unwrap()
        {
            SourceTarget::Album { biz, album_id, url } => {
                assert_eq!(biz, "MzYzOTU1NTUzNw==");
                assert_eq!(album_id, "4319761724883959819");
                assert!(url.starts_with("https://mp.weixin.qq.com/mp/appmsgalbum"));
            }
            other => panic!("应为 Album，实际 {other:?}"),
        }
    }

    #[test]
    fn plain_name_is_account_query() {
        match parse_input("静小姐的文字室").unwrap() {
            SourceTarget::AccountQuery { name } => assert_eq!(name, "静小姐的文字室"),
            other => panic!("应为 AccountQuery，实际 {other:?}"),
        }
        // 太短/太长不当名称
        assert!(parse_input("a").is_err());
    }

    #[test]
    fn non_wechat_needs_resolve_or_rejects() {
        // 微信域短链交给上层解析
        let err = parse_input("https://weixin.qq.com/r/abc").unwrap_err();
        assert!(err.to_string().contains("NEED_RESOLVE:"), "{err}");
        // 无关链接直接拒绝；纯文本现在当公众号名称（见 plain_name_is_account_query）
        assert!(parse_input("https://www.baidu.com/").is_err());
        assert!(parse_input("").is_err());
        // 超长文本不当名称
        assert!(parse_input(&"这".repeat(41)).is_err());
    }

    #[test]
    fn decodes_percent() {
        assert_eq!(percent_decode("a%2Bb%20c+d"), "a+b c d");
        assert_eq!(percent_decode("plain"), "plain");
        assert_eq!(percent_decode("%E4%B8%AD%E6%96%87"), "中文");
    }
}
