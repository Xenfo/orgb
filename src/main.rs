#![feature(fs_try_exists)]

use std::{fs, time::Duration};

use clap::{command, Parser};
use directories::ProjectDirs;
use openrgb::OpenRGB;
use tokio::{task, time};
use windows::{
    s,
    Win32::{
        Foundation::HANDLE,
        System::{
            Power::RegisterPowerSettingNotification,
            Services::{RegisterServiceCtrlHandlerExA, SERVICE_CONTROL_POWEREVENT},
            SystemServices::GUID_SESSION_DISPLAY_STATUS,
        },
    },
};

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Starts the event watcher
    #[arg(short, default_value_t = false)]
    event_watcher: bool,
}

unsafe extern "system" fn handler(
    dwcontrol: u32,
    dweventtype: u32,
    lpeventdata: *mut ::core::ffi::c_void,
    lpcontext: *mut ::core::ffi::c_void,
) -> u32 {
    println!(
        "dwcontrol: {}, dweventtype: {}, lpeventdata: {:?}, lpcontext: {:?}",
        dwcontrol, dweventtype, lpeventdata, lpcontext
    );

    0
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let not_idle_file_path = ProjectDirs::from("dev", "xenfo", "orgb")
        .unwrap()
        .config_dir()
        .to_owned()
        .join(".not_idle");

    if args.event_watcher {
        let result = fs::write(&not_idle_file_path, "");
        if result.is_err() {
            fs::create_dir_all(&not_idle_file_path.parent().unwrap()).unwrap();
            fs::write(&not_idle_file_path, "").unwrap();
        }

        unsafe {
            let handle = RegisterServiceCtrlHandlerExA(s!("orgb"), Some(handler), None).unwrap();
            if handle.is_invalid() {
                panic!("RegisterServiceCtrlHandlerExA failed");
            }

            let res = RegisterPowerSettingNotification(
                HANDLE(handle.0),
                &GUID_SESSION_DISPLAY_STATUS,
                SERVICE_CONTROL_POWEREVENT,
            )
            .unwrap();
            if res.is_invalid() {
                panic!("RegisterPowerSettingNotification failed");
            }
        }

        loop {}

        // unsafe {
        //     // struct PowerParams {
        //     //     Callback: *const HANDLE,
        //     // }

        //     // let mut result: *mut *mut void = null_mut();

        //     // let handle = Box::into_raw(Box::new(PowerParams {
        //     //     Callback: null_mut(),
        //     // }));
        //     // let handle_ptr = &*handle as *const HANDLE;

        //     // PowerRegisterSuspendResumeNotification(DEVICE_NOTIFY_CALLBACK.0, &*handle as *const HANDLE, result);

        //     let dummy_window = unsafe {
        //         CreateWindowExW(
        //             0,
        //             CLASS_NAME,
        //             null_mut(),
        //             0,
        //             0,
        //             0,
        //             0,
        //             0,
        //             0,
        //             0,
        //             GetModuleHandleW(null_mut()),
        //             null_mut(),
        //         )
        //     };
        // }
    } else {
        let task = task::spawn(async move {
            let mut interval = time::interval(Duration::from_millis(500));

            loop {
                interval.tick().await;

                let open_rgb = OpenRGB::connect().await.unwrap();

                if fs::try_exists(&not_idle_file_path).unwrap() {
                    open_rgb.load_profile("Blue").await.unwrap();
                } else {
                    open_rgb.load_profile("Black").await.unwrap();
                }
            }
        });

        task.await.unwrap();
    }
}

// use std::{os::windows::io::AsRawHandle, ptr, sync::Arc, thread};
// use tokio::sync::broadcast;
// use windows_sys::{
//     w,
//     Win32::{
//         Foundation::{HANDLE, HWND, LPARAM, LRESULT, WPARAM},
//         System::{LibraryLoader::GetModuleHandleW, Threading::GetThreadId},
//         UI::WindowsAndMessaging::{
//             CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
//             GetWindowLongPtrW, PostQuitMessage, PostThreadMessageW, SetWindowLongPtrW,
//             TranslateMessage, GWLP_USERDATA, GWLP_WNDPROC, PBT_APMRESUMEAUTOMATIC,
//             PBT_APMRESUMESUSPEND, PBT_APMSUSPEND, WM_DESTROY, WM_POWERBROADCAST, WM_USER,
//         },
//     },
// };

// const CLASS_NAME: *const u16 = w!("STATIC");
// const REQUEST_THREAD_SHUTDOWN: u32 = WM_USER + 1;

// /// Handle for closing an associated window.
// /// The window is not destroyed when this is dropped.
// pub struct WindowCloseHandle {
//     thread: Option<std::thread::JoinHandle<()>>,
// }

// impl WindowCloseHandle {
//     /// Close the window and wait for the thread.
//     pub fn close(&mut self) {
//         if let Some(thread) = self.thread.take() {
//             let thread_id = unsafe { GetThreadId(thread.as_raw_handle() as HANDLE) };
//             unsafe { PostThreadMessageW(thread_id, REQUEST_THREAD_SHUTDOWN, 0, 0) };
//             let _ = thread.join();
//         }
//     }
// }

// /// Creates a dummy window whose messages are handled by `wnd_proc`.
// pub fn create_hidden_window<F: (Fn(HWND, u32, WPARAM, LPARAM) -> LRESULT) + Send + 'static>(
//     wnd_proc: F,
// ) -> WindowCloseHandle {
//     let join_handle = thread::spawn(move || {
//         let dummy_window = unsafe {
//             CreateWindowExW(
//                 0,
//                 CLASS_NAME,
//                 ptr::null_mut(),
//                 0,
//                 0,
//                 0,
//                 0,
//                 0,
//                 0,
//                 0,
//                 GetModuleHandleW(ptr::null_mut()),
//                 ptr::null_mut(),
//             )
//         };

//         // Move callback information to the heap.
//         // This enables us to reach the callback through a "thin pointer".
//         let raw_callback = Box::into_raw(Box::new(wnd_proc));

//         unsafe {
//             SetWindowLongPtrW(dummy_window, GWLP_USERDATA, raw_callback as isize);
//             SetWindowLongPtrW(dummy_window, GWLP_WNDPROC, window_procedure::<F> as isize);
//         }

//         let mut msg = unsafe { std::mem::zeroed() };

//         loop {
//             let status = unsafe { GetMessageW(&mut msg, 0, 0, 0) };

//             if status < 0 {
//                 continue;
//             }
//             if status == 0 {
//                 break;
//             }

//             if msg.hwnd == 0 {
//                 if msg.message == REQUEST_THREAD_SHUTDOWN {
//                     unsafe { DestroyWindow(dummy_window) };
//                 }
//             } else {
//                 unsafe {
//                     TranslateMessage(&mut msg);
//                     DispatchMessageW(&mut msg);
//                 }
//             }
//         }

//         // Free callback.
//         let _ = unsafe { Box::from_raw(raw_callback) };
//     });

//     WindowCloseHandle {
//         thread: Some(join_handle),
//     }
// }

// unsafe extern "system" fn window_procedure<F>(
//     window: HWND,
//     message: u32,
//     wparam: WPARAM,
//     lparam: LPARAM,
// ) -> LRESULT
// where
//     F: Fn(HWND, u32, WPARAM, LPARAM) -> LRESULT,
// {
//     if message == WM_DESTROY {
//         PostQuitMessage(0);
//         return 0;
//     }
//     let raw_callback = GetWindowLongPtrW(window, GWLP_USERDATA);
//     if raw_callback != 0 {
//         let typed_callback = &mut *(raw_callback as *mut F);
//         return typed_callback(window, message, wparam, lparam);
//     }
//     DefWindowProcW(window, message, wparam, lparam)
// }

// /// Power management events
// #[non_exhaustive]
// #[derive(Debug, Clone, Copy, PartialEq)]
// pub enum PowerManagementEvent {
//     /// The system is resuming from sleep or hibernation
//     /// irrespective of user activity.
//     ResumeAutomatic,
//     /// The system is resuming from sleep or hibernation
//     /// due to user activity.
//     ResumeSuspend,
//     /// The computer is about to enter a suspended state.
//     Suspend,
// }

// impl PowerManagementEvent {
//     fn try_from_winevent(wparam: usize) -> Option<Self> {
//         use PowerManagementEvent::*;
//         match wparam as u32 {
//             PBT_APMRESUMEAUTOMATIC => Some(ResumeAutomatic),
//             PBT_APMRESUMESUSPEND => Some(ResumeSuspend),
//             PBT_APMSUSPEND => Some(Suspend),
//             _ => None,
//         }
//     }
// }

// /// Provides power management events to listeners
// pub struct PowerManagementListener {
//     _window: Arc<WindowScopedHandle>,
//     rx: broadcast::Receiver<PowerManagementEvent>,
// }

// impl PowerManagementListener {
//     /// Creates a new listener. This is expensive compared to cloning an existing instance.
//     pub fn new() -> Self {
//         let (tx, rx) = tokio::sync::broadcast::channel(16);

//         let power_broadcast_callback = move |window, message, wparam, lparam| {
//             if message == WM_POWERBROADCAST {
//                 if let Some(event) = PowerManagementEvent::try_from_winevent(wparam) {
//                     if tx.send(event).is_err() {
//                         println!("Stopping power management event monitor");
//                         unsafe { PostQuitMessage(0) };
//                         return 0;
//                     }
//                 }
//             }
//             unsafe { DefWindowProcW(window, message, wparam, lparam) }
//         };

//         let window = create_hidden_window(power_broadcast_callback);

//         Self {
//             _window: Arc::new(WindowScopedHandle(window)),
//             rx,
//         }
//     }

//     /// Returns the next power event.
//     pub async fn next(&mut self) -> Option<PowerManagementEvent> {
//         loop {
//             match self.rx.recv().await {
//                 Ok(event) => break Some(event),
//                 Err(broadcast::error::RecvError::Closed) => {
//                     println!("Sender was unexpectedly dropped");
//                     break None;
//                 }
//                 Err(broadcast::error::RecvError::Lagged(num_skipped)) => {
//                     println!("Skipped {num_skipped} power broadcast events");
//                 }
//             }
//         }
//     }
// }

// impl Clone for PowerManagementListener {
//     fn clone(&self) -> Self {
//         Self {
//             _window: self._window.clone(),
//             rx: self.rx.resubscribe(),
//         }
//     }
// }

// struct WindowScopedHandle(WindowCloseHandle);

// impl Drop for WindowScopedHandle {
//     fn drop(&mut self) {
//         self.0.close();
//     }
// }
