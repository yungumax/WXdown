//! WXdown 核心库：公众号会话、来源解析、文章/合集解析、下载引擎。
//!
//! 文章页与合集接口（getalbum）对桌面浏览器免登录开放；
//! 公众平台登录（UP主全量枚举，阶段二）与小程序嗅探（阶段三）
//! 由 src-tauri 层提供，本库只做纯网络与解析。

pub mod album;
pub mod api;
pub mod article;
pub mod client;
pub mod download;
pub mod error;
pub mod login;
pub mod parser;
pub mod platform;

pub use client::WxClient;
pub use error::{Result, WxError};
