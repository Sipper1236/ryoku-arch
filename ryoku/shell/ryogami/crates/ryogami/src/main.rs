mod client;
mod config;
mod db;
mod render;
mod server;
mod util;
mod wall;

use tracing_subscriber::{
    EnvFilter, Layer, fmt, layer::SubscriberExt, util::SubscriberInitExt,
};

const VERSION: &str = env!("RYOGAMI_VERSION");

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("ryogami {VERSION}");
        return Ok(());
    }
    // A `wallpaper …` / `depth …` subcommand acts as a client to the running
    // daemon; anything else (no args, or `daemon`) starts the daemon below.
    if let Some(line) = client::command_line(&args) {
        return client::run(&line).await;
    }

    let log_dir = std::env::var("XDG_CACHE_HOME")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var("HOME").ok().map(|h| std::path::PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("ryogami");
    let _ = std::fs::create_dir_all(&log_dir);
    let file_appender = tracing_appender::rolling::never(&log_dir, "ryogami.log");
    let (file_writer, _file_guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = || {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
    };

    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(std::io::stderr).with_filter(env_filter()))
        .with(fmt::layer().with_writer(file_writer).with_ansi(false).with_filter(env_filter()))
        .init();

    Box::leak(Box::new(_file_guard));

    tracing::info!(version = VERSION, log_dir = %log_dir.display(), "ryogami starting; logs at ~/.cache/ryogami/ryogami.log");

    if config::load().unwrap_or_default().features.wallpapers {
        wall::apply::kill_orphan_paper_procs().await;
    } else {
        tracing::info!("wallpapers module disabled; leaving external paper renderers alone");
    }

    server::run().await
}
