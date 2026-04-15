//! System tray icon and menu for the Sidewinder background service.
//!
//! On Windows this uses the `Shell_NotifyIcon` API via `windows-sys`.
//! On other platforms it compiles to a no-op stub so the rest of the app
//! remains buildable.

use tokio::sync::watch;

use crate::{
    config::Config,
    status::{ConnectionStatus, StartupStatus},
};

// ── Public API ────────────────────────────────────────────────────────────────

/// Spawn the system tray (Windows) or log a startup message (other platforms).
///
/// Returns a `watch::Receiver<bool>` that becomes `true` when the user
/// requests quit via the tray menu.
#[cfg_attr(
    not(target_os = "windows"),
    expect(
        clippy::needless_pass_by_value,
        reason = "on non-Windows args are intentionally dropped; on Windows they are moved into the tray thread"
    )
)]
pub fn spawn_tray(
    config_rx: watch::Receiver<Config>,
    status_rx: watch::Receiver<ConnectionStatus>,
    startup: StartupStatus,
) -> watch::Receiver<bool> {
    let (quit_tx, quit_rx) = watch::channel(false);

    #[cfg(target_os = "windows")]
    {
        tokio::task::spawn_blocking(move || {
            windows_tray::run(config_rx, status_rx, quit_tx, startup);
        });
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Non-Windows: just log and keep the quit receiver open forever.
        let _ = config_rx;
        let _ = status_rx;
        let _ = startup;
        let _ = quit_tx;
        tracing::info!("system tray not available on this platform — running headless");
    }

    quit_rx
}

// ── Tooltip text ─────────────────────────────────────────────────────────────

/// Build the tray tooltip string for a given connection + driver + calibration state.
///
/// Pure function — no Win32 FFI. Testable on all platforms.
#[cfg(any(target_os = "windows", test))]
fn status_label(
    conn: ConnectionStatus,
    driver: crate::status::DriverStatus,
    calibration_set: bool,
) -> &'static str {
    use crate::status::DriverStatus;
    match (conn, driver) {
        (ConnectionStatus::Connected, DriverStatus::Present) if !calibration_set => {
            "Sidewinder FFB2 \u{2014} calibration not set (run Calibrate for accuracy)"
        }
        (ConnectionStatus::Connected, DriverStatus::Present) => {
            "Sidewinder FFB2 \u{2014} connected"
        }
        (ConnectionStatus::Connected, DriverStatus::Missing) => {
            "Driver not installed \u{2014} FFB unavailable. Right-click \u{2192} Install Driver."
        }
        (ConnectionStatus::Disconnected, DriverStatus::Present) => {
            "Waiting for joystick\u{2026} (plug in your Sidewinder Force Feedback 2)"
        }
        (ConnectionStatus::Disconnected, DriverStatus::Missing) => {
            "No joystick or driver found. Check connections and driver installation."
        }
    }
}

// ── Windows implementation ────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
#[expect(
    unsafe_code,
    reason = "Win32 system tray requires raw WNDCLASSEX/HWND/NOTIFYICONDATA FFI calls"
)]
mod windows_tray {
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::watch;
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            Shell::{
                NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
                Shell_NotifyIconW,
            },
            WindowsAndMessaging::{
                AppendMenuW, CW_USEDEFAULT, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
                DestroyMenu, DestroyWindow, DispatchMessageW, GetMessageW, MF_STRING, MSG,
                PostMessageW, PostQuitMessage, RegisterClassExW, SetForegroundWindow,
                TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RETURNCMD, TrackPopupMenu, TranslateMessage,
                WM_APP, WM_DESTROY, WM_RBUTTONUP, WNDCLASSEXW, WS_EX_TOOLWINDOW,
                WS_OVERLAPPEDWINDOW,
            },
        },
    };

    use crate::{
        config::Config,
        status::{ConnectionStatus, DriverStatus, StartupStatus},
    };

    const WM_TRAY: u32 = WM_APP + 1;
    const IDM_QUIT: usize = 1001;
    const IDM_STATUS: usize = 1002;
    const IDM_INSTALL_DRIVER: usize = 1003;

    /// Set once in `run()` before the message loop; read in `tray_wnd_proc` to
    /// add the "Install Driver…" menu item when the driver is missing.
    static DRIVER_MISSING: AtomicBool = AtomicBool::new(false);

    /// Custom window message posted when the connection status changes.
    ///
    /// `wParam` carries the new [`ConnectionStatus`] discriminant as a `usize`:
    /// `0` = Connected, `1` = Disconnected.
    ///
    /// Compile-time assertions below keep the encode/decode in sync with the enum.
    const WM_STATUS_CHANGED: u32 = WM_APP + 2;

    // Compile-time contract: wParam encoding must match enum discriminants.
    const _: () = assert!(ConnectionStatus::Connected as usize == 0);
    const _: () = assert!(ConnectionStatus::Disconnected as usize == 1);

    fn status_label(conn: ConnectionStatus, driver: DriverStatus, calibration_set: bool) -> &'static str {
        super::status_label(conn, driver, calibration_set)
    }

    fn fill_tip(tip: &mut [u16; 128], text: &str) {
        tip.fill(0);
        for (i, c) in text.encode_utf16().take(127).enumerate() {
            tip[i] = c;
        }
    }

    /// Run the tray message loop on the calling (blocking) thread.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "config_rx and status_rx ownership transferred here; consumed by the tray lifetime"
    )]
    pub fn run(
        _config_rx: watch::Receiver<Config>,
        mut status_rx: watch::Receiver<ConnectionStatus>,
        quit_tx: watch::Sender<bool>,
        startup: StartupStatus,
    ) {
        // Store driver status for use in tray_wnd_proc (an extern "system" fn
        // that cannot capture closure state directly).
        DRIVER_MISSING.store(startup.driver == DriverStatus::Missing, Ordering::Relaxed);

        // SAFETY: standard WinAPI setup; all calls follow documented usage.
        unsafe {
            let hinstance = GetModuleHandleW(std::ptr::null());

            // Register a minimal hidden window class for message delivery.
            let class_name: Vec<u16> = "SideblinderTray\0".encode_utf16().collect();
            #[expect(
                clippy::cast_possible_truncation,
                reason = "struct size always fits in u32"
            )]
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(tray_wnd_proc),
                hInstance: hinstance,
                lpszClassName: class_name.as_ptr(),
                style: 0,
                cbClsExtra: 0,
                cbWndExtra: 0,
                hIcon: std::ptr::null_mut(),
                hCursor: std::ptr::null_mut(),
                hbrBackground: std::ptr::null_mut(),
                lpszMenuName: std::ptr::null(),
                hIconSm: std::ptr::null_mut(),
            };
            if RegisterClassExW(&raw const wc) == 0 {
                tracing::error!("Failed to register tray window class — tray icon will not appear");
                return;
            }

            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class_name.as_ptr(),
                std::ptr::null(),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                std::ptr::null_mut(), // no parent window
                std::ptr::null_mut(), // no menu
                hinstance,
                std::ptr::null(),
            );
            if hwnd.is_null() {
                tracing::error!("Failed to create tray window — tray icon will not appear");
                return;
            }

            let nid = add_tray_icon(hwnd, &mut status_rx, startup);
            spawn_status_watcher(hwnd, status_rx);
            run_message_loop(hwnd, &nid, startup);

            let _ = quit_tx.send(true);
        }
    }

    /// Add the tray icon with the current connection status as tooltip.
    ///
    /// # Safety
    /// `hwnd` must be a valid window handle created on the calling thread.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "struct size always fits in u32"
    )]
    unsafe fn add_tray_icon(
        hwnd: HWND,
        status_rx: &mut watch::Receiver<ConnectionStatus>,
        startup: StartupStatus,
    ) -> NOTIFYICONDATAW {
        let initial_status = *status_rx.borrow_and_update();
        let mut tip = [0u16; 128];
        fill_tip(
            &mut tip,
            status_label(initial_status, startup.driver, startup.calibration_set),
        );

        // SAFETY: zeroed() is valid for NOTIFYICONDATAW (all-zero is a legal initial state);
        // hwnd is a valid handle supplied by the caller; Shell_NotifyIconW follows documented usage.
        let nid = unsafe {
            NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: 1,
                uFlags: NIF_MESSAGE | NIF_TIP,
                uCallbackMessage: WM_TRAY,
                hIcon: std::ptr::null_mut(),
                szTip: tip,
                ..std::mem::zeroed()
            }
        };
        if unsafe { Shell_NotifyIconW(NIM_ADD, &raw const nid) } == 0 {
            tracing::error!("Failed to add system tray icon — the app will run without a tray icon");
        } else {
            tracing::info!("system tray icon added");
        }
        nid
    }

    /// Spawn a background thread that watches for status changes and posts
    /// `WM_STATUS_CHANGED` to the tray message loop.
    ///
    /// The thread exits when either:
    /// - all `ConnectionStatus` senders are dropped (`status_rx.changed()` returns `Err`), or
    /// - `PostMessageW` returns 0, indicating the target window has been destroyed.
    ///
    /// # Safety
    /// `hwnd` must encode a valid window handle at the time of each `PostMessageW` call.
    /// The thread self-terminates when `PostMessageW` fails, so the caller only needs to
    /// ensure the window remains valid while the bridge is running.
    unsafe fn spawn_status_watcher(hwnd: HWND, mut status_rx: watch::Receiver<ConnectionStatus>) {
        let rt = tokio::runtime::Handle::current();
        // HWND is *mut c_void and not Send.  Transmit as usize (which is Send)
        // and cast back inside the thread.  PostMessageW is thread-safe per
        // Win32 documentation.
        let hwnd_val: usize = hwnd as usize;
        std::thread::spawn(move || {
            loop {
                if rt.block_on(status_rx.changed()).is_err() {
                    break;
                }
                let new_status = *status_rx.borrow_and_update();
                // Encode ConnectionStatus as wParam discriminant.
                // Compile-time assertions ensure 0=Connected, 1=Disconnected.
                let wparam: usize = new_status as usize;
                // SAFETY: PostMessageW is thread-safe; hwnd_val encodes a valid HWND.
                // Exit the loop if PostMessageW returns 0 (window destroyed).
                let ok = unsafe { PostMessageW(hwnd_val as HWND, WM_STATUS_CHANGED, wparam, 0) };
                if ok == 0 {
                    tracing::warn!(
                        "Status watcher: PostMessageW failed — tray tooltip will no longer \
                         reflect connection changes"
                    );
                    break;
                }
            }
        });
    }

    /// Run the Win32 message loop until `WM_QUIT`, updating the tray tooltip on
    /// `WM_STATUS_CHANGED`, then remove the tray icon and destroy the window.
    ///
    /// # Safety
    /// `hwnd` and `nid` must have been created by the calling thread.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "struct size always fits in u32"
    )]
    unsafe fn run_message_loop(hwnd: HWND, nid: &NOTIFYICONDATAW, startup: StartupStatus) {
        // SAFETY: all Win32 calls follow documented usage; hwnd and nid were
        // created on this thread and remain valid for the duration of this fn.
        unsafe {
            let mut msg = MSG {
                hwnd: std::ptr::null_mut(),
                message: 0,
                wParam: 0,
                lParam: 0,
                time: 0,
                pt: windows_sys::Win32::Foundation::POINT { x: 0, y: 0 },
            };

            loop {
                let ret = GetMessageW(&raw mut msg, std::ptr::null_mut(), 0, 0);
                if ret == 0 {
                    break; // WM_QUIT
                }
                if ret == -1 {
                    tracing::error!(
                        "The system tray stopped responding. Restart the app to restore the tray icon."
                    );
                    break;
                }

                if msg.message == WM_STATUS_CHANGED {
                    let new_status = match msg.wParam {
                        0 => ConnectionStatus::Connected,
                        1 => ConnectionStatus::Disconnected,
                        other => {
                            tracing::warn!(
                                "tray: unexpected WM_STATUS_CHANGED wParam {other}; ignoring"
                            );
                            continue;
                        }
                    };
                    tracing::info!("tray: connection status → {new_status:?}");
                    let mut updated_nid = NOTIFYICONDATAW {
                        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                        hWnd: hwnd,
                        uID: 1,
                        uFlags: NIF_TIP,
                        hIcon: std::ptr::null_mut(),
                        ..std::mem::zeroed()
                    };
                    fill_tip(
                        &mut updated_nid.szTip,
                        status_label(new_status, startup.driver, startup.calibration_set),
                    );
                    if Shell_NotifyIconW(NIM_MODIFY, &raw const updated_nid) == 0 {
                        tracing::warn!("Failed to update tray tooltip");
                    }
                    continue;
                }

                TranslateMessage(&raw const msg);
                DispatchMessageW(&raw const msg);
            }

            if Shell_NotifyIconW(NIM_DELETE, nid) == 0 {
                tracing::warn!("Failed to remove tray icon on shutdown — icon may linger");
            }
            DestroyWindow(hwnd);
        }
    }

    unsafe extern "system" fn tray_wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // SAFETY: all Win32 calls here follow documented usage; hwnd, hmenu are
        // valid handles provided by the OS message loop.
        unsafe {
            match msg {
                #[expect(
                    clippy::cast_sign_loss,
                    clippy::cast_possible_truncation,
                    reason = "lparam low word holds a WM_ constant; upper bits are zero on 64-bit Windows for mouse messages"
                )]
                WM_TRAY if lparam as u32 == WM_RBUTTONUP => {
                    // Show context menu on right-click.
                    // TPM_RETURNCMD returns the selected item ID as the return
                    // value of TrackPopupMenu rather than posting WM_COMMAND.
                    // This avoids the issue where WM_COMMAND from a popup menu
                    // is delivered via DispatchMessageW (to the wndproc), not
                    // via GetMessageW (to the message loop), making it
                    // unreachable in the loop's quit-detection branch.
                    let hmenu = CreatePopupMenu();
                    let status: Vec<u16> = "Sidewinder FFB2 — running\0".encode_utf16().collect();
                    let quit: Vec<u16> = "Quit\0".encode_utf16().collect();
                    AppendMenuW(hmenu, MF_STRING, IDM_STATUS, status.as_ptr());
                    // Show "Install Driver..." only when the driver is absent.
                    if DRIVER_MISSING.load(Ordering::Relaxed) {
                        let install: Vec<u16> =
                            "Install Driver\u{2026}\0".encode_utf16().collect();
                        AppendMenuW(hmenu, MF_STRING, IDM_INSTALL_DRIVER, install.as_ptr());
                    }
                    AppendMenuW(hmenu, MF_STRING, IDM_QUIT, quit.as_ptr());

                    let mut pt = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
                    windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos(&raw mut pt);
                    SetForegroundWindow(hwnd);
                    let selected = TrackPopupMenu(
                        hmenu,
                        TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RETURNCMD,
                        pt.x,
                        pt.y,
                        0,
                        hwnd,
                        std::ptr::null(),
                    );
                    DestroyMenu(hmenu);

                    // If the user chose Quit, post WM_QUIT to exit GetMessageW.
                    #[expect(
                        clippy::cast_possible_wrap,
                        reason = "IDM_* constants fit in i32; comparisons are safe"
                    )]
                    if selected == IDM_QUIT as i32 {
                        PostQuitMessage(0);
                    } else if selected == IDM_INSTALL_DRIVER as i32 {
                        // Open the install script directory in File Explorer so the
                        // user can review and run it as administrator. Resolve the
                        // path relative to the executable so it works regardless of
                        // the process CWD (which is unpredictable for tray apps).
                        std::thread::spawn(|| {
                            let scripts_path = std::env::current_exe()
                                .ok()
                                .and_then(|p| p.parent().map(|d| d.join("scripts")))
                                .unwrap_or_else(|| std::path::PathBuf::from("scripts"));
                            if !scripts_path.is_dir() {
                                tracing::warn!(
                                    scripts_dir = %scripts_path.display(),
                                    "Install scripts directory not found — check your installation."
                                );
                                return;
                            }
                            if let Err(e) = std::process::Command::new("explorer.exe")
                                .arg(&scripts_path)
                                .spawn()
                            {
                                tracing::warn!(
                                    internal_error = %e,
                                    scripts_dir = %scripts_path.display(),
                                    "Couldn't open the install scripts directory."
                                );
                            }
                        });
                    }
                    0
                }
                WM_DESTROY => {
                    PostQuitMessage(0);
                    0
                }
                _ => DefWindowProcW(hwnd, msg, wparam, lparam),
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::status::{ConnectionStatus, DriverStatus};

    #[test]
    fn status_label_connected_calibrated() {
        assert_eq!(
            status_label(ConnectionStatus::Connected, DriverStatus::Present, true),
            "Sidewinder FFB2 \u{2014} connected"
        );
    }

    #[test]
    fn status_label_connected_no_calibration() {
        let label = status_label(ConnectionStatus::Connected, DriverStatus::Present, false);
        assert!(
            label.contains("calibration not set"),
            "expected calibration warning, got: {label}"
        );
    }

    #[test]
    fn status_label_connected_driver_missing() {
        let label = status_label(ConnectionStatus::Connected, DriverStatus::Missing, true);
        assert!(
            label.contains("Driver not installed"),
            "expected driver-missing message, got: {label}"
        );
    }

    #[test]
    fn status_label_disconnected_driver_present() {
        let label = status_label(ConnectionStatus::Disconnected, DriverStatus::Present, true);
        assert!(
            label.contains("Waiting for joystick"),
            "expected waiting message, got: {label}"
        );
    }

    #[test]
    fn status_label_disconnected_driver_missing() {
        let label = status_label(ConnectionStatus::Disconnected, DriverStatus::Missing, false);
        assert!(
            label.contains("No joystick or driver"),
            "expected no-device message, got: {label}"
        );
    }
}
