//! 错误类型：界面拿到的是人话，日志里保留细节。

#[derive(Debug, thiserror::Error)]
pub enum WxError {
    #[error("网络请求失败：{0}")]
    Http(#[from] reqwest::Error),

    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),

    #[error("接口返回异常：{0}")]
    Api(String),

    #[error("响应解析失败：{0}")]
    Decode(String),

    #[error("不支持或无法识别的链接：{0}")]
    InvalidInput(String),

    #[error("需要登录：{0}")]
    NeedLogin(String),

    #[error("下载不完整：应为 {expected} 字节，实得 {actual} 字节")]
    IncompleteDownload { expected: u64, actual: u64 },

    #[error("{0}")]
    Unavailable(String),
}

pub type Result<T> = std::result::Result<T, WxError>;

pub fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars).collect();
        out.push_str("...");
        out
    }
}
