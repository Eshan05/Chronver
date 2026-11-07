#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
//! Opt-in native pixel regression. Creates its own temporary colored backdrop and foreground
//! windows without taking focus; samples only their test area, never captures a screenshot.

#[cfg(target_os = "windows")]
use rune::{App, AppSpec, Color, WindowOptions, el};
#[cfg(target_os = "windows")]
use std::sync::atomic::{AtomicU32, Ordering};
#[cfg(target_os = "windows")]
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
    Graphics::{
        Dwm::DwmFlush,
        Gdi::{
            BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, GetDC, GetPixel,
            InvalidateRect, PAINTSTRUCT, ReleaseDC, UpdateWindow,
        },
    },
    System::LibraryLoader::GetModuleHandleW,
    UI::WindowsAndMessaging::*,
};

#[cfg(target_os = "windows")]
#[path = "../src/native_acrylic.rs"]
mod native_acrylic;
#[cfg(target_os = "windows")]
#[path = "../src/native_frame.rs"]
mod native_frame;

#[cfg(target_os = "windows")]
static MODE: AtomicU32 = AtomicU32::new(0);

#[cfg(target_os = "windows")]
unsafe extern "system" fn background(hwnd: HWND, message: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        if message == WM_PAINT {
            let mut paint: PAINTSTRUCT = std::mem::zeroed();
            let dc = BeginPaint(hwnd, &mut paint);
            let mut client: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut client);
            let mode = MODE.load(Ordering::Relaxed);
            let count = if mode == 2 { client.right / 12 + 1 } else { 1 };
            for i in 0..count {
                let color = match mode {
                    0 => 0x002828dc,
                    1 => 0x00dc2828,
                    _ => {
                        if i % 2 == 0 {
                            0
                        } else {
                            0x00ffffff
                        }
                    }
                };
                let brush = CreateSolidBrush(color);
                let rect = if mode == 2 {
                    RECT {
                        left: i * 12,
                        top: 0,
                        right: (i + 1) * 12,
                        bottom: client.bottom,
                    }
                } else {
                    client
                };
                FillRect(dc, &rect, brush);
                DeleteObject(brush);
            }
            EndPaint(hwnd, &paint);
            return 0;
        }
        DefWindowProcW(hwnd, message, wp, lp)
    }
}

#[cfg(target_os = "windows")]
unsafe fn fixture() -> HWND {
    unsafe {
        let name = windows_sys::w!("ReleaseScoutOwnedBackdropTest");
        let class = WNDCLASSW {
            lpfnWndProc: Some(background),
            hInstance: GetModuleHandleW(std::ptr::null()),
            lpszClassName: name,
            ..std::mem::zeroed()
        };
        assert_ne!(RegisterClassW(&class), 0);
        let hwnd = CreateWindowExW(
            WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            name,
            name,
            WS_POPUP,
            40,
            60,
            720,
            500,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            class.hInstance,
            std::ptr::null(),
        );
        assert!(!hwnd.is_null());
        ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        UpdateWindow(hwnd);
        hwnd
    }
}

#[cfg(target_os = "windows")]
unsafe fn sample(x: i32, y: i32) -> [i32; 3] {
    unsafe {
        let dc = GetDC(std::ptr::null_mut());
        let color = GetPixel(dc, x, y);
        ReleaseDC(std::ptr::null_mut(), dc);
        assert_ne!(color, u32::MAX);
        [
            (color & 255) as _,
            ((color >> 8) & 255) as _,
            ((color >> 16) & 255) as _,
        ]
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("The native backdrop pixel probe requires an interactive Windows desktop.");
}

#[cfg(target_os = "windows")]
fn main() {
    std::panic::set_hook(Box::new(|info| {
        eprintln!("{info}");
        std::process::exit(1);
    }));
    let bg = unsafe { fixture() } as usize;
    let app = App::new(AppSpec::new(
        "Chronver isolated backdrop test",
        (480., 360.),
        || (),
        |_, _| {
            el().w_full()
                .h_full()
                .fill(Color::TRANSPARENT)
                .child(el().absolute(320., 0.).w(160.).h_full().fill(Color::BLACK))
        },
    ))
    .titlebar(rune::Titlebar::new(48.))
    .open_inactive()
    .window_options(WindowOptions {
        transparent: true,
        always_on_top: true,
        position: Some((100, 120)),
        min_size: Some((0., 0.)),
        ..Default::default()
    });
    let handle = app.handle();
    app.on_window(|window| {
        native_frame::install(window).unwrap();
        window
            .with_raw_handle(|raw| unsafe {
                let raw_window_handle::RawWindowHandle::Win32(raw) = raw.as_raw() else {
                    return;
                };
                let hwnd = raw.hwnd.get() as HWND;
                if !std::env::args().any(|a| a == "--transparent-only") {
                    assert!(native_acrylic::apply(hwnd, true));
                }
            })
            .unwrap();
    })
    .on_ready(move |window| {
        let window = window.clone();
        let handle = handle.clone();
        std::thread::spawn(move || {
            use std::{sync::mpsc::sync_channel, time::Duration};
            std::thread::sleep(Duration::from_millis(600));
            let (send, receive) = sync_channel(1);
            let for_origin = window.clone();
            handle.update(move |_, _| {
                let origin = for_origin
                    .with_raw_handle(|raw| unsafe {
                        let raw_window_handle::RawWindowHandle::Win32(raw) = raw.as_raw() else {
                            panic!("Expected a Windows handle");
                        };
                        let hwnd = raw.hwnd.get() as HWND;
                        let moved = std::env::args().any(|a| a == "--move");
                        let sized = std::env::args().any(|a| a == "--resize");
                        if moved || sized {
                            assert_ne!(
                                SetWindowPos(
                                    hwnd,
                                    std::ptr::null_mut(),
                                    140,
                                    150,
                                    600,
                                    400,
                                    SWP_NOZORDER
                                        | SWP_NOACTIVATE
                                        | if moved { 0 } else { SWP_NOMOVE }
                                        | if sized { 0 } else { SWP_NOSIZE }
                                ),
                                0
                            );
                        }
                        let mut origin = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
                        assert_ne!(
                            windows_sys::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut origin),
                            0
                        );
                        (origin.x, origin.y)
                    })
                    .unwrap();
                send.send(origin).unwrap();
            });
            let (x, y) = receive.recv_timeout(Duration::from_secs(3)).unwrap();
            std::thread::sleep(Duration::from_millis(600));
            let mut values = Vec::new();
            for mode in [0, 1, 2] {
                let (send, receive) = sync_channel(1);
                handle.update(move |_, _| unsafe {
                    MODE.store(mode, Ordering::Relaxed);
                    InvalidateRect(bg as HWND, std::ptr::null(), 0);
                    UpdateWindow(bg as HWND);
                    send.send(()).unwrap();
                });
                receive.recv_timeout(Duration::from_secs(3)).unwrap();
                // Keep both UI windows responsive while the compositor settles and pixels
                // are read. Blocking their UI thread can change the native material itself.
                std::thread::sleep(Duration::from_millis(400));
                unsafe {
                    DwmFlush();
                    let control = sample(50, 70);
                    let expected = match mode {
                        0 => [220, 40, 40],
                        1 => [40, 40, 220],
                        _ => [0, 0, 0],
                    };
                    assert_eq!(control, expected, "Owned test background is obscured");
                    let body = sample(x + 400, y + 180);
                    assert_eq!(body, [0; 3], "Opaque test pane is not visible");
                    let sample_x = if std::env::args().any(|a| a == "--resize") {
                        520
                    } else {
                        80
                    };
                    let pixel = sample(x + sample_x, y + 180);
                    eprintln!("Owned fixture {mode}: transparent={pixel:?}; opaque={body:?}");
                    if mode < 2 {
                        values.push(pixel);
                    } else {
                        let row: Vec<_> = (0..12)
                            .map(|i| sample(x + 60 + i * 6, y + 180)[0])
                            .collect();
                        eprintln!("Checker samples: {row:?}");
                        let span = row.iter().max().unwrap() - row.iter().min().unwrap();
                        if std::env::args().any(|a| a == "--transparent-only") {
                            assert_eq!(span, 255, "Transparent control must preserve the checker");
                        } else {
                            assert!(span < 40, "Acrylic must blur the checker, not just tint it");
                        }
                    }
                }
            }
            assert!(
                (values[0][0] - values[1][0]).abs() > 50
                    && (values[0][2] - values[1][2]).abs() > 50,
                "Background colors must change the probe pixels"
            );
            eprintln!("PASS: real background response, checker behavior, and opaque pane");
            let exit = handle.clone();
            handle.update(move |_, _| unsafe {
                window
                    .with_raw_handle(|raw| {
                        if let raw_window_handle::RawWindowHandle::Win32(raw) = raw.as_raw() {
                            native_acrylic::clear(raw.hwnd.get() as _);
                        }
                    })
                    .unwrap();
                DestroyWindow(bg as HWND);
                exit.quit();
            });
        });
    })
    .run();
}
