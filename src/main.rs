use clap::{command, Parser};
use openrgb::OpenRGB;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Runs the sleep functions
    #[arg(short, long, default_value_t = false)]
    sleep: bool,

    /// Runs the wake-up functions
    #[arg(short, long, default_value_t = false)]
    wake_up: bool,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    let open_rgb = OpenRGB::connect().await.unwrap();

    if args.sleep {
        open_rgb.load_profile("Black").await.unwrap();
    } else if args.wake_up {
        open_rgb.load_profile("Blue").await.unwrap();
    }
}
