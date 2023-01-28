#![feature(fs_try_exists)]

use std::{process, ptr};

use openrgb::OpenRGB;
use tokio::sync::broadcast::{self, Receiver};
use tracing::{debug, error, info, instrument};
use windows::{
    core::PCSTR,
    s,
    Win32::{
        Foundation::{HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        System::{
            Power::{
                RegisterPowerSettingNotification, RegisterSuspendResumeNotification,
                DEVICE_NOTIFY_WINDOW_HANDLE, POWERBROADCAST_SETTING,
            },
            SystemServices::GUID_CONSOLE_DISPLAY_STATE,
        },
        UI::WindowsAndMessaging::{
            CreateWindowExA, DefWindowProcA, DispatchMessageA, GetMessageA, GetWindowLongPtrA,
            PostQuitMessage, SetWindowLongPtrA, TranslateMessage, GWLP_USERDATA, GWLP_WNDPROC,
            HMENU, PBT_APMRESUMESUSPEND, PBT_APMSUSPEND, PBT_POWERSETTINGCHANGE, WINDOW_EX_STYLE,
            WINDOW_STYLE, WM_DESTROY, WM_POWERBROADCAST,
        },
    },
};

#[derive(Clone, Debug)]
enum PowerEvent {
    Wake,
    Sleep,
}

#[tokio::main]
#[instrument]
async fn main() {
    tracing_subscriber::fmt::init();

    info!("Starting");

    let mut manager = PowerEventManager::new();
    let window = manager.window.clone();

    tokio::spawn(async move {
        let open_rgb = OpenRGB::connect().await.unwrap_or_else(|err| {
            error!("Unable to connect to OpenRGB SDK server: {:#?}", err);
            process::exit(1)
        });

        loop {
            let event = manager.next_event().await;
            debug!("Power event was received: {:#?}", event);

            let controller_count = open_rgb.get_controller_count().await.unwrap();
            for id in 0..controller_count {
                let controller = open_rgb.get_controller(id).await.unwrap();

                let found_mode = controller
                    .modes
                    .into_iter()
                    .enumerate()
                    .find(|(_, mode)| mode.name == "Direct");
                let Some((index, mode)) = found_mode else {
                    error!("Unable to find \"Direct\" mode for controller {} ({})", id, controller.name);
                    continue;
                };

                open_rgb
                    .update_mode(id, index as i32, mode)
                    .await
                    .unwrap_or_else(|err| {
                        error!(
                            "Unable to set controller {} ({}) to \"Direct\" mode: {:#?}",
                            id, controller.name, err
                        );
                    });
            }

            match event {
                PowerEvent::Wake => {
                    if let Err(err) = open_rgb.load_profile("Blue").await {
                        error!("Unable to load profile \"Blue\": {:#?}", err)
                    };
                }
                PowerEvent::Sleep => {
                    if let Err(err) = open_rgb.load_profile("Black").await {
                        error!("Unable to load profile \"Black\": {:#?}", err)
                    };
                }
            };
        }
    });

    PowerEventManager::listen(window);

    info!("Exiting");
}

unsafe extern "system" fn window_procedure<F>(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT
where
    F: Fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
{
    if message == WM_DESTROY {
        PostQuitMessage(0);
        return LRESULT(0);
    }

    let callback = GetWindowLongPtrA(window, GWLP_USERDATA);
    if callback != 0 {
        let typed_callback = &mut *(callback as *mut F);
        return typed_callback(window, message, wparam, lparam);
    }

    DefWindowProcA(window, message, wparam, lparam)
}

struct PowerEventManager {
    window: HWND,
    rx: Receiver<PowerEvent>,
}

impl PowerEventManager {
    fn new() -> Self {
        let (tx, rx) = broadcast::channel(16);

        let window = unsafe {
            CreateWindowExA(
                WINDOW_EX_STYLE(0),
                s!("STATIC"),
                PCSTR(ptr::null()),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                HWND(0),
                HMENU(0),
                HINSTANCE(0),
                None,
            )
        };
        if window.0 == 0 {
            error!("Unable to create the window");
            process::exit(1)
        }

        let callback =
            move |window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM| -> LRESULT {
                if message == WM_POWERBROADCAST {
                    let event = match wparam.0 as u32 {
                        PBT_APMSUSPEND => Some(PowerEvent::Sleep),
                        PBT_APMRESUMESUSPEND => Some(PowerEvent::Wake),
                        PBT_POWERSETTINGCHANGE => {
                            let settings = unsafe { &*(lparam.0 as *const POWERBROADCAST_SETTING) };

                            if settings.PowerSetting == GUID_CONSOLE_DISPLAY_STATE {
                                match settings.Data[0] {
                                    0 => Some(PowerEvent::Sleep),
                                    1 => Some(PowerEvent::Wake),
                                    _ => None,
                                }
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };
                    let Some(event) = event else {
                    return LRESULT(0)
                };

                    tx.send(event).unwrap_or_else(|err| {
                        error!("Unable to send the power event: {:#?}", err);
                        0
                    });

                    return LRESULT(0);
                }

                unsafe { DefWindowProcA(window, message, wparam, lparam) }
            };

        let manager = Self { rx, window };

        unsafe { manager.set_ptrs(callback) };

        let unregister_sleep_wake_notification = unsafe {
            RegisterSuspendResumeNotification(HANDLE(window.0), DEVICE_NOTIFY_WINDOW_HANDLE.0)
                .unwrap_or_else(|err| {
                    error!(
                        "Unable to register for suspend/resume notifications: {:#?}",
                        err
                    );
                    process::exit(1)
                })
        };
        if unregister_sleep_wake_notification.is_invalid() {
            error!("Unable to register for suspend/resume notifications");
            process::exit(1)
        }

        let unregister_power_setting_notification = unsafe {
            RegisterPowerSettingNotification(
                HANDLE(window.0),
                &GUID_CONSOLE_DISPLAY_STATE,
                DEVICE_NOTIFY_WINDOW_HANDLE.0,
            )
            .unwrap_or_else(|err| {
                error!(
                    "Unable to register for power setting notifications: {:#?}",
                    err
                );
                process::exit(1)
            })
        };
        if unregister_power_setting_notification.is_invalid() {
            error!("Unable to register for power setting notifications");
            process::exit(1)
        }

        manager
    }

    fn listen(window: HWND) {
        let mut msg = unsafe { std::mem::zeroed() };
        loop {
            let status = unsafe { GetMessageA(&mut msg, window, 0, 0) };
            if status.0 < 0 {
                continue;
            }
            if status.0 == 0 {
                break;
            }

            unsafe {
                TranslateMessage(&mut msg);
                DispatchMessageA(&mut msg);
            }
        }
    }

    async fn next_event(&mut self) -> PowerEvent {
        self.rx.recv().await.unwrap_or_else(|err| {
            error!("Unable to receive the power event: {:#?}", err);
            process::exit(1)
        })
    }

    unsafe fn set_ptrs<F>(&self, callback: F)
    where
        F: Fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
    {
        SetWindowLongPtrA(
            self.window,
            GWLP_USERDATA,
            Box::into_raw(Box::new(callback)) as isize,
        );
        SetWindowLongPtrA(self.window, GWLP_WNDPROC, window_procedure::<F> as isize);
    }
}
