//! WXdown 本地 MITM 代理：
//! - 小程序/视频号媒体嗅探（URL 规则匹配，直链可下载）
//! - 公众号文章列表捕获（收割微信客户端的 profile_ext 会话凭据，
//!   再由上层用凭据重放枚举全部文章——2026-07 平台枚举接口被封后的替代路线）
//!
//! 证书：首次生成 WXdown 根 CA（数据目录 proxy-ca.key/.crt），装机需要用户
//! 同意装进系统受信任根（UAC），系统代理只写当前用户注册表、退出还原。

pub mod ca;
pub mod proxy;
pub mod rules;

pub use proxy::{run_proxy, SniffEvent, SniffHandler, StopHandle};
