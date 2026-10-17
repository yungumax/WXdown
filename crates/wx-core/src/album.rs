//! 合集（appmsgalbum）接口：免登录 JSON，按 begin_msgid/begin_itemidx 翻页。
//!
//! 实测（2026-10）：
//! `GET /mp/appmsgalbum?__biz=…&action=getalbum&album_id=…&f=json&count=N`
//! 直接回 `{base_resp:{ret:0}, getalbum_resp:{article_list:[…], base_info:{…}, continue_flag:"1"}}`，
//! 桌面 UA 即可、无需 Cookie；翻页带上上一页最后一条的 `begin_msgid`/`begin_itemidx`。
//! 注意是 `action=getalbum`（页面里内嵌的旧 action），不是新版页面的 getappmsgalbum
//! （那个要微信客户端会话，外部调用回 ret:-3 no session——实测踩过）。

use crate::client::WxClient;
use crate::error::{Result, WxError};

/// 合集元信息。
#[derive(Debug, Clone, Default)]
pub struct AlbumInfo {
    /// 公众号名
    pub nickname: String,
    /// 合集名
    pub title: String,
    pub description: String,
    /// 文章总数（接口给的是字符串）
    pub article_count: u64,
}

/// 合集里的一篇文章条目。
#[derive(Debug, Clone, Default)]
pub struct AlbumItem {
    /// 标题（纯文字帖会塞全文，带换行——已折成单行，做展示/命名用）
    pub title: String,
    /// 文章完整链接（/s?__biz=… 长链）
    pub url: String,
    /// 发布时间 Unix 秒
    pub create_time: i64,
    pub msgid: String,
    pub itemidx: String,
    pub cover: String,
    /// 摘要（content_noencode 前 120 字，折成单行）
    pub digest: String,
    /// 接口原始全文（content_noencode 原样，保留换行）：
    /// "文字+视频"类动态的文章页正文全靠 JS 渲染，服务端 HTML 里没有文字，
    /// 解析失败/空内容时用这份兜底（实测踩过：美股大涨那条）。
    pub content: String,
}

/// 一页合集结果。
#[derive(Debug, Clone, Default)]
pub struct AlbumPage {
    pub info: AlbumInfo,
    pub items: Vec<AlbumItem>,
    /// true = 还有下一页
    pub continue_flag: bool,
}

fn str_field(value: &serde_json::Value, key: &str) -> String {
    match value.get(key) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

impl WxClient {
    /// 取一页合集条目。`begin` 传上一页最后一条的 (msgid, itemidx)。
    pub async fn album_page(
        &self,
        biz: &str,
        album_id: &str,
        count: u32,
        begin: Option<(&str, &str)>,
    ) -> Result<AlbumPage> {
        let mut url = format!(
            "https://mp.weixin.qq.com/mp/appmsgalbum?__biz={}&action=getalbum&album_id={}&f=json&count={}",
            urlencode(biz),
            urlencode(album_id),
            count.max(1).min(20),
        );
        if let Some((msgid, itemidx)) = begin {
            url.push_str(&format!(
                "&begin_msgid={}&begin_itemidx={}",
                urlencode(msgid),
                urlencode(itemidx)
            ));
        }
        let value = self.fetch_json(&url, crate::client::SITE_REFERER).await?;
        let ret = value
            .pointer("/base_resp/ret")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        if ret != 0 {
            let msg = value
                .pointer("/base_resp/errmsg")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            return Err(WxError::Api(format!(
                "合集接口返回异常 ret={ret} {msg}（合集可能被删除或仅微信内可见）"
            )));
        }
        let resp = value
            .get("getalbum_resp")
            .ok_or_else(|| WxError::Decode("合集响应里没有 getalbum_resp".into()))?;
        let base = resp.get("base_info").cloned().unwrap_or_default();
        let info = AlbumInfo {
            nickname: str_field(&base, "nickname"),
            title: str_field(&base, "title"),
            description: str_field(&base, "description"),
            article_count: str_field(&base, "article_count").parse().unwrap_or(0),
        };
        let continue_flag = str_field(resp, "continue_flag") == "1";
        let mut items = Vec::new();
        if let Some(list) = resp.get("article_list").and_then(|v| v.as_array()) {
            for entry in list {
                let title_raw = str_field(entry, "title");
                let title = title_raw
                    .lines()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                let digest_raw = str_field(entry, "content_noencode");
                let digest: String = digest_raw
                    .lines()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(120)
                    .collect();
                items.push(AlbumItem {
                    title,
                    url: str_field(entry, "url").replace("http://", "https://"),
                    create_time: entry
                        .get("create_time")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0),
                    msgid: str_field(entry, "msgid"),
                    itemidx: str_field(entry, "itemidx"),
                    cover: str_field(entry, "cover_img_1_1"),
                    digest,
                    content: str_field(entry, "content_noencode"),
                });
            }
        }
        Ok(AlbumPage {
            info,
            items,
            continue_flag,
        })
    }
}

/// 极简 URL 编码（合集参数都是安全字符，这里只处理少数字符）。
pub fn urlencode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_album_json() {
        let json = r#"{
          "base_resp": {"ret": 0},
          "getalbum_resp": {
            "article_list": [
              {"title": "标题A\n\n第二行", "url": "http://mp.weixin.qq.com/s?__biz=MzA&mid=1&idx=1&sn=a",
               "create_time": 1766547906, "msgid": 2247484749, "itemidx": 1,
               "content_noencode": "摘要文本", "cover_img_1_1": ""}
            ],
            "base_info": {"nickname": "示例号", "title": "示例合集", "description": "说明",
                          "article_count": "33"},
            "continue_flag": "1"
          }
        }"#;
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        // 走一遍字段抽取（album_page 的核心逻辑内联在这测不了网络，
        // 这里校验字段取值函数）
        let resp = &value["getalbum_resp"];
        assert_eq!(
            str_field(&resp["base_info"], "article_count"),
            "33"
        );
        let entry = &resp["article_list"][0];
        assert_eq!(str_field(entry, "msgid"), "2247484749");
        assert_eq!(
            entry.get("create_time").and_then(|v| v.as_i64()).unwrap(),
            1766547906
        );
    }

    #[test]
    fn urlencodes() {
        assert_eq!(urlencode("MzYzOTU1NTUzNw=="), "MzYzOTU1NTUzNw%3D%3D");
        assert_eq!(urlencode("abc-123"), "abc-123");
    }
}
