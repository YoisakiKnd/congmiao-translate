//! 本机守护进程。桌面端和浏览器宿主都通过它访问翻译引擎。

#![forbid(unsafe_code)]

mod install;
mod jobs;
mod log;
mod server;

pub use install::{firefox_host_manifest, host_manifest, install_host, InstallRequest};
pub use log::init as init_log;
pub use server::{serve, serve_with};
