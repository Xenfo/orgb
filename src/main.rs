// TODO: Support shutdown events

// #[cfg(not(target_os = "windows"))]
// compile_error!("compilation is only allowed for Windows targets");

use std::process;

use directories::ProjectDirs;
use tokio::sync::broadcast::{self, Receiver};
use tracing::{debug, error, info, instrument, metadata::LevelFilter};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, subscribe::CollectExt, EnvFilter};
use windows::{
    s,
    Win32::{
        Foundation::{HANDLE, HMODULE, HWND, LPARAM, LRESULT, WPARAM},
        System::{
            Power::{
                RegisterPowerSettingNotification, RegisterSuspendResumeNotification,
                POWERBROADCAST_SETTING,
            },
            SystemServices::GUID_CONSOLE_DISPLAY_STATE,
        },
        UI::WindowsAndMessaging::{
            CreateWindowExA, DefWindowProcA, DispatchMessageA, GetMessageA, GetWindowLongPtrA,
            PostQuitMessage, SetWindowLongPtrA, TranslateMessage, DEVICE_NOTIFY_WINDOW_HANDLE,
            GWLP_USERDATA, GWLP_WNDPROC, HMENU, PBT_APMRESUMESUSPEND, PBT_APMSUSPEND,
            PBT_POWERSETTINGCHANGE, WINDOW_EX_STYLE, WINDOW_STYLE, WM_DESTROY, WM_POWERBROADCAST,
        },
    },
};

use crate::client::OpenRGBClient;

mod client;

#[derive(Clone, Debug)]
enum PowerEvent {
    Wake,
    Sleep,
}

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
                PowerEvent::Wake => {
                    open_rgb.load_profile("Blue").await;
                }
                PowerEvent::Sleep => {
                    open_rgb.load_profile("Black").await;
                }
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
                s!("ORGB"),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                HWND(0),
                HMENU(0),
                HMODULE(0),
                None,
            )
        };
        if window.0 == 0 {
            error!("Unable to create the window");
            process::exit(1)
        }

        let callback =
            move |window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM| -> LRESULT {
                let event = match message {
                    WM_POWERBROADCAST => match wparam.0 as u32 {
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
                    },
                    _ => None,
                };

                if let Some(event) = event {
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
            RegisterSuspendResumeNotification(HANDLE(window.0), DEVICE_NOTIFY_WINDOW_HANDLE)
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

            #[allow(clippy::unnecessary_mut_passed)]
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
        #[allow(clippy::fn_to_numeric_cast)]
        SetWindowLongPtrA(self.window, GWLP_WNDPROC, window_procedure::<F> as isize);
    }
}
