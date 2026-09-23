use std::path::Path;

use tracing_subscriber::prelude::*;

pub fn init(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    let appender = tracing_appender::rolling::daily(dir, "congmiao.log");
    let _ = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(appender)
        .finish()
        .try_init();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("{info}");
        previous(info);
    }));
}
