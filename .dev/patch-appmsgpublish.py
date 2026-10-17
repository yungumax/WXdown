# -*- coding: utf-8 -*-
"""platform.rs 切到 appmsgpublish 接口(旧 appmsg 已被微信下线)"""
import io
import sys

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")

p = "crates/wx-core/src/platform.rs"
s = open(p, encoding="utf-8").read()

# 1) AppmsgPage 加 end 标志
s = s.replace(
    """/// 一页 appmsg 结果。
#[derive(Debug, Clone, Default)]
pub struct AppmsgPage {
    pub items: Vec<AppmsgItem>,
    /// 该公众号文章总数
    pub total: u64,
}""",
    """/// 一页 appmsgpublish 结果。
#[derive(Debug, Clone, Default)]
pub struct AppmsgPage {
    /// 本页文章（一次发布可含多篇，已摊平）
    pub items: Vec<AppmsgItem>,
    /// 总发布批次数（不等于文章数：一次发布可含多篇；展示用）
    pub total: u64,
    /// true = publish_list 为空，已翻完
    pub end: bool,
}""",
)

# 2) 接口实现替换
old_impl_start = s.index("    /// 枚举公众号文章一页")
old_impl_end = s.index("fn now_ms()")
new_impl = '''    /// 枚举公众号文章一页（appmsgpublish，按"发布批次"翻页：begin 偏移、
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

'''
s = s[:old_impl_start] + new_impl + s[old_impl_end:]

# 3) 删 now_ms(不再使用)
s = s.replace(
    """fn now_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}""",
    "",
)

# 4) searchbiz 的 random 参数去掉,Referer 改根路径(与 exporter 对齐)
s = s.replace(
    """        let url = format!(
            "https://mp.weixin.qq.com/cgi-bin/searchbiz?action=search_biz&token={}&lang=zh_CN&f=json&ajax=1&random={:.0}&query={}&begin=0&count=5",
            crate::album::urlencode(token),
            now_ms(),
            crate::album::urlencode(query),
        );
        let value = self
            .fetch_json(&url, "https://mp.weixin.qq.com/cgi-bin/home?t=home/index")""",
    """        let url = format!(
            "https://mp.weixin.qq.com/cgi-bin/searchbiz?action=search_biz&token={}&lang=zh_CN&f=json&ajax=1&query={}&begin=0&count=5",
            crate::album::urlencode(token),
            crate::album::urlencode(query),
        );
        let value = self
            .fetch_json(&url, "https://mp.weixin.qq.com/")""",
)

# 5) 测试换成 publish_page 形态
test_start = s.index("    #[test]\n    fn parses_appmsg_shape")
test_end = s.index("    #[test]\n    fn biz_error_maps_known_codes")
new_test = '''    #[test]
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

'''
s = s[:test_start] + new_test + s[test_end:]
open(p, "w", encoding="utf-8").write(s)
print("platform.rs ok")
