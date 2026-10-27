pub mod install;
pub mod ipc;
pub mod paths;
pub mod secure;
pub mod server;
#[cfg(windows)]
mod windows;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
