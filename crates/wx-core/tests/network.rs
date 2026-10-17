//! 真实网络冒烟测试（匿名路径）：
//! 用一篇公开文章与一个公开合集验证「直取页面 → 解析出数据」全链路。
//!
//! 运行：cargo test -p wx-core --test network -- --nocapture
//! 样例链接来自公开渠道（2026-10 实测可用）；若上游删除属预期失败，
//! 换新的公开样例即可。

use wx_core::album::AlbumPage;
use wx_core::parser::SourceTarget;
use wx_core::WxClient;

/// NAS 部署音乐服务器文章（含图片，服务端渲染标准页）
const SAMPLE_ARTICLE: &str = "https://mp.weixin.qq.com/s/is9FoCMlqRbUQcEPfOnt1w";
/// 「日常随笔」合集（33 篇，getalbum 免登录）
const SAMPLE_ALBUM_BIZ: &str = "MzYzOTU1NTUzNw==";
const SAMPLE_ALBUM_ID: &str = "4319761724883959819";

#[tokio::test]
async fn parses_public_article() {
    let client = WxClient::new().expect("构建客户端");
    let data = client
        .article_detail(SAMPLE_ARTICLE)
        .await
        .expect("抓取并解析文章");
    assert!(!data.title.is_empty(), "标题不能为空");
    assert!(!data.account.is_empty(), "公众号名不能为空");
    assert!(!data.text.is_empty(), "正文不能为空");
    assert!(!data.images.is_empty(), "这篇样例应含图片");
    assert!(
        data.images.iter().all(|img| img.url.contains("mmbiz.qpic.cn")),
        "图片都应在 mmbiz 域"
    );
    println!(
        "文章解析 OK：{}（{}）正文 {} 字、{} 图、{} 视频",
        data.title,
        data.account,
        data.text.chars().count(),
        data.images.len(),
        data.videos.len()
    );
}

#[tokio::test]
async fn fetches_album_first_page() {
    let client = WxClient::new().expect("构建客户端");
    let page: AlbumPage = client
        .album_page(SAMPLE_ALBUM_BIZ, SAMPLE_ALBUM_ID, 5, None)
        .await
        .expect("取合集第一页");
    assert_eq!(page.info.nickname, "金渐成");
    assert_eq!(page.info.article_count, 33);
    assert_eq!(page.items.len(), 5);
    assert!(page.continue_flag, "33 篇的合集第一页后应有下一页");
    assert!(page.items.iter().all(|i| i.url.starts_with("https://mp.weixin.qq.com/s")));
    println!(
        "合集解析 OK：{}《{}》{} 篇，本页 {} 条",
        page.info.nickname,
        page.info.title,
        page.info.article_count,
        page.items.len()
    );
}

#[tokio::test]
async fn album_pagination_advances() {
    let client = WxClient::new().expect("构建客户端");
    let page1 = client
        .album_page(SAMPLE_ALBUM_BIZ, SAMPLE_ALBUM_ID, 5, None)
        .await
        .expect("第一页");
    let last = page1.items.last().expect("第一页有条目");
    let page2 = client
        .album_page(SAMPLE_ALBUM_BIZ, SAMPLE_ALBUM_ID, 5, Some((&last.msgid, &last.itemidx)))
        .await
        .expect("第二页");
    assert_eq!(page2.items.len(), 5);
    assert_ne!(
        page2.items[0].msgid, last.msgid,
        "第二页首条不能与第一页末条相同"
    );
    println!("翻页 OK：page1 末 msgid={} → page2 首 msgid={}", last.msgid, page2.items[0].msgid);
}

#[tokio::test]
async fn article_exposes_embedded_album() {
    let client = WxClient::new().expect("构建客户端");
    // 日常随笔合集里的一条动态(实测页面内嵌 album 链接)
    let data = client
        .article_detail("https://mp.weixin.qq.com/s?__biz=MzYzOTU1NTUzNw==&mid=2247484749&idx=1&sn=f553dd20b542569d15fd137fc392866b&chksm=f0d187f3c7a60ee530cc3eff7aa5327246b3430044669874748cc2baea69abbb05977fde7e40")
        .await
        .expect("抓取文章");
    assert_eq!(data.album_id, "4319761724883959819", "页面内嵌的合集 ID");
    assert!(!data.album_biz.is_empty());
    // 用抠出的合集身份真的拉一页
    let page = client
        .album_page(&data.album_biz, &data.album_id, 5, None)
        .await
        .expect("按内嵌合集身份取合集");
    assert_eq!(page.info.article_count, 33);
    println!("文章→合集 OK：{}《{}》", page.info.nickname, page.info.title);
}

#[test]
fn parser_accepts_share_forms() {
    assert!(matches!(
        wx_core::parser::parse_input("https://mp.weixin.qq.com/s/abcDEF123_-"),
        Ok(SourceTarget::Article { .. })
    ));
    assert!(matches!(
        wx_core::parser::parse_input("https://mp.weixin.qq.com/s?__biz=MzA&mid=1&idx=1&sn=x"),
        Ok(SourceTarget::Article { .. })
    ));
    assert!(matches!(
        wx_core::parser::parse_input("https://mp.weixin.qq.com/mp/appmsgalbum?__biz=MzA&album_id=123"),
        Ok(SourceTarget::Album { .. })
    ));
}
