use std::{process, time::Duration};

use openrgb::{
    data::{Controller, Mode},
    OpenRGB, OpenRGBError,
};
use tokio::{net::TcpStream, time};
use tracing::{debug, error, trace};

const ATTEMPTS: i32 = 100;
const CONTROLLER_COUNT: u32 = 4;

pub struct OpenRGBClient {
    client: Option<OpenRGB<TcpStream>>,
}

impl OpenRGBClient {
    pub fn new() -> Self {
        Self { client: None }
    }

    pub async fn connect(&mut self) {
        let mut interval = time::interval(Duration::from_secs(3));

        for idx in 0..ATTEMPTS {
            let result = OpenRGB::connect().await;
            let Ok(open_rgb) = result else {
                trace!("Unable to connect to OpenRGB SDK server, {} retries left", ATTEMPTS - idx);

                if idx == ATTEMPTS - 1 {
                    error!("Unable to connect to OpenRGB SDK server: {:#?}", unsafe { result.unwrap_err_unchecked() });
                    process::exit(1)
                }

                interval.tick().await;
                continue;
            };

            debug!("Connected to OpenRGB SDK server");

            open_rgb.set_name("ORGB").await.unwrap_or_else(|err| {
                error!("Unable to set OpenRGB SDK client name: {:#?}", err);
                process::exit(1)
            });

            self.client = Some(open_rgb);
            return;
        }

        self.client = None;
    }

    pub async fn ensure_controllers(&mut self) {
        for idx in 0..ATTEMPTS {
            let count = self.client.as_ref().unwrap().get_controller_count().await;

            if let Ok(count) = count {
                if count == CONTROLLER_COUNT {
                    debug!("Controller count: {CONTROLLER_COUNT}");
                    return;
                }
            }

            trace!(
                "Invalid controller count, {} retries left",
                ATTEMPTS - idx
            );
            if idx == ATTEMPTS - 1 {
                error!("Invalid controller count: {:#?}", count.err());
                process::exit(1)
            }

            if let Err(error) = count {
                self.handle_error(error).await;
            }

            time::sleep(Duration::from_secs(3)).await;
            continue;
        }
    }

    pub async fn get_controller(&mut self, controller_id: u32) -> Option<Controller> {
        for idx in 0..ATTEMPTS {
            let controller = self
                .client
                .as_ref()
                .unwrap()
                .get_controller(controller_id)
                .await;

            match controller {
                Ok(ctrl) => return Some(ctrl),
                Err(error) => {
                    trace!(
                        "Unable to get controller {controller_id}, {} retries left",
                        ATTEMPTS - idx
                    );
                    if idx == ATTEMPTS - 1 {
                        error!("Unable to get controller {controller_id}: {:#?}", error);
                        process::exit(1)
                    }

                    self.handle_error(error).await;
                    time::sleep(Duration::from_secs(3)).await;
                    continue;
                }
            }
        }

        None
    }

    pub async fn update_mode(&mut self, controller_id: u32, mode_id: i32, mode: Mode) {
        for idx in 0..ATTEMPTS {
            let update = self
                .client
                .as_ref()
                .unwrap()
                .update_mode(
                    controller_id,
                    mode_id,
                    Mode {
                        name: mode.name.clone(),
                        colors: mode.colors.clone(),
                        ..mode
                    },
                )
                .await;

            match update {
                Err(error) => {
                    trace!(
                        "Unable to set mode for controller {controller_id} to \"Direct\" mode, {} retries left",
                        ATTEMPTS - idx
                    );
                    if idx == ATTEMPTS - 1 {
                        error!(
                            "Unable to set mode for controller {controller_id} to \"Direct\" mode: {:#?}",
                            error
                        );
                        process::exit(1)
                    }

                    self.handle_error(error).await;
                    time::sleep(Duration::from_secs(3)).await;
                    continue;
                }
                _ => return,
            }
        }
    }

    pub async fn set_direct(&mut self) {
        for id in 0..CONTROLLER_COUNT {
            let controller = self.get_controller(id).await.unwrap();
            trace!("Controller {id}: {:#?}", controller);

            let found_mode = controller
                .modes
                .into_iter()
                .enumerate()
                .find(|(_, mode)| mode.name == "Direct");
            let Some((index, mode)) = found_mode else {
                error!("Unable to find \"Direct\" mode for controller {id} ({})", controller.name);
                process::exit(1);
            };
            debug!(
                "Found \"Direct\" mode for controller {id} ({}) at index {index}",
                controller.name
            );

            self.update_mode(id, index as i32, mode).await;
        }
    }

    pub async fn load_profile(&mut self, profile: &str) {
        for idx in 0..ATTEMPTS {
            let result = self.client.as_ref().unwrap().load_profile(profile).await;

            match result {
                Ok(_) => return,
                Err(error) => {
                    trace!("Unable to load profile \"{profile}\", {} retries left", ATTEMPTS - idx);
                    if idx == ATTEMPTS - 1 {
                        error!("Unable to load profile \"{profile}\": {:#?}", error);
                    }

                    self.handle_error(error).await;
                    time::sleep(Duration::from_secs(3)).await;
                    continue;
                }
            }
        }
    }

    pub async fn handle_error(&mut self, error: OpenRGBError) {
        if let OpenRGBError::CommunicationError { source } = error {
            if source.kind() == std::io::ErrorKind::ConnectionReset {
                self.connect().await;
            }
        }
    }
}
