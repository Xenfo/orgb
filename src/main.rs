#[cfg(not(target_os = "windows"))]
compile_error!("compilation is only allowed for Windows targets");

use directories::ProjectDirs;
use tracing::{debug, info, instrument, metadata::LevelFilter};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, subscribe::CollectExt, EnvFilter};

use crate::{client::OpenRGBClient, manager::{PowerEventManager, PowerEvent}};

mod client;
mod manager;

#[tokio::main]
#[instrument]
async fn main() {
    let _guard = init_tracing();

    info!("Starting");

    let mut open_rgb = OpenRGBClient::new();

    open_rgb.connect().await;
    open_rgb.ensure_controllers().await;

    open_rgb.set_direct().await;
    open_rgb.load_profile("Blue").await;

    let mut manager = PowerEventManager::new();
    let window = manager.window;

    tokio::spawn(async move {
        loop {
            let event = manager.next_event().await;
            debug!("Power event was received: {:#?}", event);

            open_rgb.set_direct().await;

            match event {
                PowerEvent::Wake => open_rgb.load_profile("Blue").await,
                PowerEvent::Sleep => open_rgb.load_profile("Black").await,
            };
        }
    });

    PowerEventManager::listen(window);

    info!("Exiting");
}

fn init_tracing() -> WorkerGuard {
    let config_path = ProjectDirs::from("dev", "Xenfo", "orgb").unwrap();

    let file_appender =
        tracing_appender::rolling::daily(config_path.data_dir().join("logs"), "orgb.log");
    let (file_writer, guard) = tracing_appender::non_blocking(file_appender);

    #[cfg(debug_assertions)]
    let log_level = LevelFilter::TRACE;
    #[cfg(not(debug_assertions))]
    let log_level = LevelFilter::DEBUG;

    let collector = tracing_subscriber::registry()
        .with(
            EnvFilter::builder()
                .with_default_directive(log_level.into())
                .from_env_lossy(),
        )
        .with(fmt::Subscriber::new().with_writer(std::io::stdout))
        .with(
            fmt::Subscriber::new()
                .with_writer(file_writer)
                .with_ansi(false),
        );
    tracing::collect::set_global_default(collector).unwrap();

    guard
}

