//! 登录辅助：公众平台登录（阶段二）走 src-tauri 的 web_login_* 命令，
//! 这里只放调试日志与文本工具。

/// 调试日志：%APPDATA%\wxdown\wx_debug.log（无 APPDATA 则退到临时目录）。
pub fn debug_log(line: &str) {
    use std::io::Write;
    let path = std::env::var("APPDATA")
        .map(|a| std::path::PathBuf::from(a).join("wxdown").join("wx_debug.log"))
        .unwrap_or_else(|_| std::env::temp_dir().join("wxdown_debug.log"));
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{ts}] {line}");
    }
}

/// 文本摘要：取前 max_chars 个字符，换行折成空格。
pub fn title_preview(text: &str, max_chars: usize) -> String {
    let flat: String = text
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let trimmed = flat.trim();
    if trimmed.chars().count() <= max_chars {
        trimmed.to_string()
    } else {
        trimmed.chars().take(max_chars).collect()
    }
}
