# -*- coding: utf-8 -*-
"""账号枚举翻页改批次偏移 + 限频节奏"""
import io
import sys

sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")

# state.rs: BatchCache 加 cursor
p = "src-tauri/src/state.rs"
s = open(p, encoding="utf-8").read()
s = s.replace(
    """    /// 来源总数（合集 article_count / 公众号 app_msg_cnt；用于判断是否翻完）
    pub total: usize,""",
    """    /// 来源总数（合集 article_count / 公众号发布批次数；展示用）
    pub total: usize,
    /// 公众号枚举的下一页 begin 偏移（按发布批次计，每页 5 批；合集不用）
    pub cursor: u32,""",
)
open(p, "w", encoding="utf-8").write(s)

p = "src-tauri/src/commands.rs"
s = open(p, encoding="utf-8").read()

# 1) 三处现有 BatchCache 构造补 cursor: 0
s = s.replace(
    """            BatchCache {
                uid: biz.to_string(),
                album_id: album_id.to_string(),
                kind: "album".into(),
                total,""",
    """            BatchCache {
                uid: biz.to_string(),
                album_id: album_id.to_string(),
                kind: "album".into(),
                total,
                cursor: 0,""",
)
s = s.replace(
    """                            BatchCache {
                                uid: probe.uid.clone(),
                                album_id: probe.bid.clone(),
                                kind: "album".into(),
                                total: probe.total,""",
    """                            BatchCache {
                                uid: probe.uid.clone(),
                                album_id: probe.bid.clone(),
                                kind: "album".into(),
                                total: probe.total,
                                cursor: 0,""",
)
s = s.replace(
    """            BatchCache {
                uid: fakeid.clone(),
                album_id: String::new(),
                kind: "account".into(),
                total,
                items: items.clone(),
                exhausted: items.len() >= total,""",
    """            BatchCache {
                uid: fakeid.clone(),
                album_id: String::new(),
                kind: "account".into(),
                total,
                cursor: 5,
                items: items.clone(),
                exhausted: page.end,""",
)

# 2) account_probe 的 total/exhausted/note 语义修正
s = s.replace(
    """    let total = if page.total > 0 {
        page.total as usize
    } else {
        items.len()
    };
    // 同一份缓存记两个键:名称键(继续解析/重命名按输入找) + fakeid 键
    for key in [format!("account-query:{name}"), format!("account:{fakeid}")] {""",
    """    let total = page.total as usize;
    // 同一份缓存记两个键:名称键(继续解析/重命名按输入找) + fakeid 键
    for key in [format!("account-query:{name}"), format!("account:{fakeid}")] {""",
)
s = s.replace(
    """        note: format!(
            "公众号全部文章：共 {} 篇；点「解析全部」逐页加载（平台限频，几百篇要几分钟）",
            total
        ),
        total,
        loaded: items.len(),
        exhausted: items.len() >= total,""",
    """        note: format!(
            "公众号全部文章：{} 次发布已出 {} 篇；点「解析全部」逐页加载到底（平台限频，几百篇要几分钟）",
            total,
            items.len()
        ),
        total,
        loaded: items.len(),
        exhausted: page.end,""",
)

# 3) probe_more 的 account 分支:cursor 偏移 + 页间节奏 + end 判定
old_start = s.index('    if cache.kind == "account" {')
old_end = s.index('    if cache.album_id.is_empty() {')
new_block = '''    if cache.kind == "account" {
        // 公众号全量：appmsgpublish 按发布批次翻页(cursor 为下一页 begin,每页 5 批,
        // 一批可含多篇文章);页间固定间隔 + 限频退避,翻完的标志是 publish_list 为空
        let token = state.mp_token.lock().unwrap().clone();
        if token.is_empty() {
            return Err("登录已失效，请重新登录后再试".into());
        }
        let fakeid = cache.uid.clone();
        let account = cache
            .items
            .last()
            .map(|i| i.author.clone())
            .unwrap_or_default();
        let want = want.max(5) as usize;
        let mut new_items: Vec<PostEntry> = Vec::new();
        let mut note = String::new();
        let mut total = cache.total;
        let mut cursor = cache.cursor;
        let mut exhausted = false;
        while new_items.len() < want {
            let page =
                match with_freq_retry(|| client.appmsg_page(&token, &fakeid, cursor, 5), 4).await {
                    Ok(page) => page,
                    Err(e) => {
                        note = format!("{e}；已加载的可先下载，等一两分钟再点「继续解析」补齐");
                        break;
                    }
                };
            if page.total > 0 {
                total = page.total as usize;
            }
            if page.end {
                exhausted = true;
                break;
            }
            for item in &page.items {
                if cache.items.iter().any(|e| e.bid == item.aid)
                    || new_items.iter().any(|e| e.bid == item.aid)
                {
                    continue;
                }
                new_items.push(appmsg_item_to_entry(item, &account));
            }
            cursor += 5;
            // 页间节奏：猛拉必触发 freq control（实测）
            tokio::time::sleep(Duration::from_millis(2600)).await;
        }
        let loaded = cache.items.len() + new_items.len();
        cache.total = total;
        cache.cursor = cursor;
        cache.items.extend(new_items.clone());
        cache.exhausted = exhausted;
        state.cache_batch(key, cache);
        return Ok(ProbeMore {
            items: new_items,
            loaded,
            total,
            exhausted,
            note: if !note.is_empty() {
                note
            } else if exhausted {
                "已到最后一篇".into()
            } else {
                String::new()
            },
        });
    }
'''
s = s[:old_start] + new_block + s[old_end:]

# 4) with_freq_retry 退避加长(平台限频粘性强,实测 45 秒不够)
s = s.replace(
    "    let mut delay = Duration::from_millis(1500);",
    "    let mut delay = Duration::from_secs(5);",
)
open(p, "w", encoding="utf-8").write(s)
print("account paging ok")
