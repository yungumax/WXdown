//! 命名模板：变量表 + 渲染器。预览与真实落盘共用这一份，保证所见即所得。

/// 变量表：(标记, 短标签, 栏目, 悬停说明)。顺序即「魔法变量」面板的栏目顺序；
/// 同名栏目必须连续（前端按"换名就换列"渲染）。
pub const VARIABLES: &[(&str, &str, &str, &str)] = &[
    ("title", "标题", "标题与作者", "文章标题（超长自动截断）"),
    ("author", "公众号名", "标题与作者", "发布文章的公众号"),
    ("collection", "合集名", "来源", "来自合集时是合集名；单篇文章时是年月（如 2026-01）"),
    ("bid", "文章标识", "标识", "sn / msgid（文章的唯一标识）"),
    ("uid", "公众号 ID", "标识", "__biz（公众号的 base64 标识）"),
    ("publish_date", "发布日期", "时间", "文章发布那天"),
    ("date", "下载日期", "时间", "任务创建那天"),
    ("year", "发布年", "时间", "如 2026"),
    ("month", "发布月", "时间", "如 01（补零）"),
    ("source_kind", "来源类型", "来源", "单篇文章 / 合集"),
    ("index", "序号", "来源", "批次内由旧到新；单条链接为 1"),
    ("ext", "扩展名", "来源", "正文 / 图片 / 视频各自的后缀"),
];

/// 渲染一条模板。`ext` 为空串时视为模板自己带扩展名；模板没写 {ext} 时自动补。
///
/// 返回相对路径（文件夹模板可以带 `/` 分层）；每个路径段都会做文件名消毒。
/// `dir = true` 表示这是文件夹层级模板：永远不自动补扩展名，空模板返回空串。
pub fn render(template: &str, vars: &std::collections::HashMap<String, String>, dir: bool) -> String {
    if dir && template.trim().is_empty() {
        return String::new();
    }
    let mut segments: Vec<String> = Vec::new();
    let mut used_ext = false;

    for raw in template.split(['/', '\\']) {
        let mut out = String::new();
        let mut rest = raw;
        loop {
            let Some(start) = rest.find('{') else {
                out.push_str(rest);
                break;
            };
            out.push_str(&rest[..start]);
            let Some(end) = rest[start..].find('}') else {
                out.push_str(&rest[start..]);
                break;
            };
            let end = start + end;
            let token = &rest[start + 1..end];
            if token == "ext" {
                used_ext = true;
                out.push_str(vars.get("ext").map(String::as_str).unwrap_or(""));
            } else if let Some(value) = vars.get(token) {
                out.push_str(value);
            } else {
                out.push('{');
                out.push_str(token);
                out.push('}');
            }
            rest = &rest[end + 1..];
        }

        let cleaned = out
            .chars()
            .map(|c| match c {
                '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                other => other,
            })
            .collect::<String>()
            .trim()
            .trim_end_matches('.')
            .trim()
            .to_string();
        if !cleaned.is_empty() && cleaned != "." && cleaned != ".." {
            segments.push(cleaned);
        }
    }

    // 模板没写 {ext}：给最后一段补上扩展名（有 ext 变量可用时；文件夹模板不补）
    if !used_ext && !dir {
        if let Some(ext) = vars.get("ext").filter(|e| !e.is_empty()) {
            match segments.last_mut() {
                Some(last) => last.push('.'),
                None => {
                    segments.push("wxdown".to_string());
                    segments.last_mut().unwrap().push('.');
                }
            }
            segments.last_mut().unwrap().push_str(ext);
        }
    }

    segments.join("/")
}

/// 序号补零：宽度由 index_pad 决定（前端按来源总数算好传进来）。
pub fn padded_index(index: u32, pad: usize) -> String {
    format!("{index:0>width$}", width = pad.max(2))
}

/// 文案摘要：取前 30 个字符，换行折成空格。
pub fn title_of(text: &str) -> String {
    wx_core::login::title_preview(text, 30)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn vars() -> HashMap<String, String> {
        let mut v = HashMap::new();
        v.insert("title".into(), "今日份穿搭分享".into());
        v.insert("author".into(), "示例作者".into());
        v.insert("year".into(), "2025".into());
        v.insert("month".into(), "10".into());
        v.insert("index".into(), padded_index(3, 2));
        v.insert("ext".into(), "jpg".into());
        v
    }

    #[test]
    fn renders_with_ext() {
        assert_eq!(render("{index} {title}.{ext}", &vars(), false), "03 今日份穿搭分享.jpg");
    }

    #[test]
    fn appends_ext_when_missing() {
        assert_eq!(render("{title}", &vars(), false), "今日份穿搭分享.jpg");
    }

    #[test]
    fn folder_template_layers() {
        assert_eq!(render("{author}/{year}-{month}", &vars(), true), "示例作者/2025-10");
    }

    #[test]
    fn sanitizes_bad_chars() {
        let mut v = vars();
        v.insert("title".into(), "a:b*c?".into());
        assert_eq!(render("{title}.{ext}", &v, false), "a_b_c_.jpg");
    }

    #[test]
    fn empty_folder_template_is_empty() {
        assert_eq!(render("", &vars(), true), "");
    }

    #[test]
    fn title_truncates_to_30() {
        let long = "字".repeat(50);
        assert_eq!(title_of(&long).chars().count(), 30);
    }
}
