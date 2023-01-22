use std::{fs, path::PathBuf, time::Duration};

use clap::{command, Parser};
use directories::ProjectDirs;
use openrgb::OpenRGB;
use tokio::{task, time};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Sets the state to idle
    #[arg(short, long, default_value_t = false)]
    sleep: bool,

    /// Sets the state to not idle
    #[arg(short, long, default_value_t = false)]
    wake_up: bool,
}

#[derive(PartialEq)]
enum State {
    Idle,
    NotIdle,
}

impl ToString for State {
    fn to_string(&self) -> String {
        match self {
            State::Idle => "idle".to_string(),
            State::NotIdle => "not_idle".to_string(),
        }
    }
}

struct Config;

impl Config {
    fn path() -> PathBuf {
        ProjectDirs::from("dev", "xenfo", "orgb")
            .unwrap()
            .config_dir()
            .to_owned()
            .join("state")
    }

    fn read() -> State {
        let path = Self::path();

        let config = fs::read_to_string(&path);
        if config.is_err() {
            fs::create_dir_all(&path.parent().unwrap()).unwrap();
            fs::write(&path, State::NotIdle.to_string()).unwrap();
        }

        match config.unwrap().as_str() {
            "idle" => State::Idle,
            "not_idle" => State::NotIdle,
            _ => State::NotIdle,
        }
    }

    fn write(state: State) {
        let path = Self::path();

        let result = fs::write(&path, state.to_string());
        if result.is_err() {
            fs::create_dir_all(&path.parent().unwrap()).unwrap();
            fs::write(&path, state.to_string()).unwrap();
        }
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    if args.sleep {
        Config::write(State::Idle);
    } else if args.wake_up {
        Config::write(State::NotIdle);
    } else {
        let task = task::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(1));
            let open_rgb = OpenRGB::connect().await.unwrap();

            loop {
                interval.tick().await;

                if Config::read() == State::Idle {
                    open_rgb.load_profile("Black").await.unwrap();
                } else {
                    open_rgb.load_profile("Blue").await.unwrap();
                }
            }
        });

        task.await.unwrap();
    }
}
