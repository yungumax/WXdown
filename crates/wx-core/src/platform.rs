//! 公众平台接口（阶段二：UP主式全量枚举）。
//!
//! 登录走 src-tauri 的内嵌网页（扫码），登录态收割成 Cookie 装进 reqwest
//! 会话后，这里直接 GET 公众平台后台的两个接口（wechat-article-exporter
//! 同款路线，实测可用 cookie+token 直调，没有 JS 签名）：
//! - `/cgi-bin/searchbiz` 按名称查公众号 → fakeid
//! - `/cgi-bin/appmsg?fakeid=…&type=9` 枚举该号全部文章（5 条/页，begin 偏移翻页）
//!
//! 频控：连拉太快回 `ret 200013 freq control`——上层用固定间隔 + 退避重试，
//! 枚举几百篇需要几分钟属正常。

use crate::client::WxClient;
use crate::error::{Result, WxError};

/// searchbiz 命中的一个公众号。
#[derive(Debug, Clone, Default)]
pub struct BizHit {
    pub nickname: String,
    pub alias: String,
    pub fakeid: String,
    pub signature: String,
}

/// appmsg 里的一篇文章。
#[derive(Debug, Clone, Default)]
pub struct AppmsgItem {
    pub aid: String,
    pub title: String,
    pub link: String,
    pub digest: String,
    /// 发布时间 Unix 秒
    pub create_time: i64,
    pub cover: String,
}

/// 一页 appmsgpublish 结果。
#[derive(Debug, Clone, Default)]
pub struct AppmsgPage {
    /// 本页文章（一次发布可含多篇，已摊平）
    pub items: Vec<AppmsgItem>,
    /// 总发布批次数（不等于文章数：一次发布可含多篇；展示用）
    pub total: u64,
    /// true = publish_list 为空，已翻完
    pub end: bool,
}

/// 平台接口的业务错误（ret 非 0）。
#[derive(Debug, Clone)]
pub enum PlatformError {
    /// 未登录 / 会话过期（ret 200002 等）：要重新扫码
    NotLoggedIn(String),
    /// 频控（ret 200013）：等一会再试
    FreqControl(String),
    Other(String),
}

impl std::fmt::Display for PlatformError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlatformError::NotLoggedIn(m) => write!(f, "公众平台登录已失效（{m}），请重新扫码登录"),
            PlatformError::FreqControl(m) => write!(f, "公众平台接口限频（{m}），稍等几秒自动重试"),
            PlatformError::Other(m) => write!(f, "公众平台接口异常：{m}"),
        }
    }
}

fn str_field(value: &serde_json::Value, key: &str) -> String {
    match value.get(key) {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// 把 base_resp 转成业务错误。
fn biz_error(ret: i64, errmsg: &str) -> PlatformError {
    match ret {
        200013 => PlatformError::FreqControl(errmsg.to_string()),
        200002 | -1 => PlatformError::NotLoggedIn(format!("ret={ret} {errmsg}")),
        _ => PlatformError::Other(format!("ret={ret} {errmsg}")),
    }
}

impl WxClient {
    /// 按名称搜索公众号。
    pub async fn search_biz(&self, token: &str, query: &str) -> std::result::Result<Vec<BizHit>, PlatformError> {
        let url = format!(
            "https://mp.weixin.qq.com/cgi-bin/searchbiz?action=search_biz&token={}&lang=zh_CN&f=json&ajax=1&query={}&begin=0&count=5",
            crate::album::urlencode(token),
            crate::album::urlencode(query),
        );
        let value = self
            .fetch_json(&url, "https://mp.weixin.qq.com/")
            .await
            .map_err(|e| PlatformError::Other(e.to_string()))?;
        let ret = value.pointer("/base_resp/ret").and_then(|v| v.as_i64()).unwrap_or(0);
        if ret != 0 {
            return Err(biz_error(ret, value.pointer("/base_resp/errmsg").and_then(|v| v.as_str()).unwrap_or("")));
        }
        let mut hits = Vec::new();
        if let Some(list) = value.get("list").and_then(|v| v.as_array()) {
            for entry in list {
                hits.push(BizHit {
                    nickname: str_field(entry, "nickname"),
                    alias: str_field(entry, "alias"),
                    fakeid: str_field(entry, "fakeid"),
                    signature: str_field(entry, "signature"),
                });
            }
        }
        Ok(hits)
    }

    /// 枚举公众号文章一页（appmsgpublish，按"发布批次"翻页：begin 偏移、
    /// 每页 count ≤ 5 个批次，一次发布可含多篇 → 摊平返回）。
    /// 旧的 cgi-bin/appmsg 已被微信下线（ret 200009 not found，实测踩过）。
    pub async fn appmsg_page(
        &self,
        token: &str,
        fakeid: &str,
        begin: u32,
        count: u32,
    ) -> std::result::Result<AppmsgPage, PlatformError> {
        let url = format!(
            "https://mp.weixin.qq.com/cgi-bin/appmsgpublish?sub=list&search_field=null&begin={begin}&count={count}&query=&fakeid={fakeid}&type=101_1&free_publish_type=1&sub_action=list_ex&token={token}&lang=zh_CN&f=json&ajax=1",
            begin = begin,
            count = count.clamp(1, 5),
            fakeid = crate::album::urlencode(fakeid),
            token = crate::album::urlencode(token),
        );
        let value = self
            .fetch_json(&url, "https://mp.weixin.qq.com/")
            .await
            .map_err(|e| PlatformError::Other(e.to_string()))?;
        let ret = value.pointer("/base_resp/ret").and_then(|v| v.as_i64()).unwrap_or(0);
        if ret != 0 {
            let msg = value
                .pointer("/base_resp/err_msg")
                .or_else(|| value.pointer("/base_resp/errmsg"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            return Err(biz_error(ret, msg));
        }
        // publish_page 是内嵌 JSON 字符串：{total_count, publish_list:[{publish_info:"…"}]}
        let page_raw = str_field(&value, "publish_page");
        if page_raw.is_empty() {
            return Err(PlatformError::Other("响应里没有 publish_page".into()));
        }
        let page: serde_json::Value = serde_json::from_str(&page_raw)
            .map_err(|e| PlatformError::Other(format!("publish_page 解析失败：{e}")))?;
        let total = page.get("total_count").and_then(|v| v.as_u64()).unwrap_or(0);
        let mut items = Vec::new();
        let publishes = page
            .get("publish_list")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        for publish in &publishes {
            let info_raw = str_field(publish, "publish_info");
            if info_raw.is_empty() {
                continue;
            }
            let Ok(info) = serde_json::from_str::<serde_json::Value>(&info_raw) else {
                continue;
            };
            if let Some(articles) = info.get("appmsgex").and_then(|v| v.as_array()) {
                for entry in articles {
                    items.push(AppmsgItem {
                        aid: str_field(entry, "aid"),
                        title: str_field(entry, "title"),
                        link: str_field(entry, "link").replace("http://", "https://"),
                        digest: str_field(entry, "digest"),
                        create_time: entry.get("create_time").and_then(|v| v.as_i64()).unwrap_or(0),
                        cover: str_field(entry, "cover"),
                    });
                }
            }
        }
        Ok(AppmsgPage {
            items,
            total,
            end: publishes.is_empty(),
        })
    }
}



#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn biz_error_maps_known_codes() {
        assert!(matches!(biz_error(200013, "freq control"), PlatformError::FreqControl(_)));
        assert!(matches!(biz_error(200002, "invalid session"), PlatformError::NotLoggedIn(_)));
        assert!(matches!(biz_error(1, "boom"), PlatformError::Other(_)));
    }

    #[test]
    fn parses_appmsgpublish_shape() {
        // publish_page 是内嵌 JSON 字符串；一次发布带两篇文章
        let publish_info = serde_json::json!({
            "appmsgex": [
                {"aid": "2247484749_1", "title": "批次文章一", "digest": "摘要一",
                 "link": "http://mp.weixin.qq.com/s?__biz=MzA&mid=1&idx=1&sn=x",
                 "create_time": 1766547906, "cover": "https://mmbiz.qpic.cn/a"},
                {"aid": "2247484749_2", "title": "批次文章二", "digest": "",
                 "link": "http://mp.weixin.qq.com/s?__biz=MzA&mid=1&idx=2&sn=y",
                 "create_time": 1766547906, "cover": ""}
            ]
        })
        .to_string();
        let publish_page = serde_json::json!({
            "total_count": 120,
            "publish_list": [{"publish_info": publish_info}]
        })
        .to_string();
        let resp = serde_json::json!({
            "base_resp": {"ret": 0, "err_msg": "ok"},
            "publish_page": publish_page
        })
        .to_string();
        let value: serde_json::Value = serde_json::from_str(&resp).unwrap();
        let page_raw = str_field(&value, "publish_page");
        let page: serde_json::Value = serde_json::from_str(&page_raw).unwrap();
        assert_eq!(page["total_count"].as_u64().unwrap(), 120);
        let publishes = page["publish_list"].as_array().unwrap();
        assert_eq!(publishes.len(), 1);
        let info: serde_json::Value =
            serde_json::from_str(str_field(&publishes[0], "publish_info").as_str()).unwrap();
        let arts = info["appmsgex"].as_array().unwrap();
        assert_eq!(arts.len(), 2);
        assert_eq!(str_field(&arts[0], "title"), "批次文章一");
        assert_eq!(arts[0]["create_time"].as_i64().unwrap(), 1766547906);
    }
}
