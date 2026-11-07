//! Keep the restored custom frame inside our painted client area, not native resize gutters.

#[cfg(target_os = "windows")]
mod windows {
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        Graphics::Gdi::{BLACK_BRUSH, FillRect, GetDC, GetStockObject, ReleaseDC, ScreenToClient},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi},
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                DestroyIcon, GWL_STYLE, GetClientRect, GetWindowLongPtrW, HICON, HTBOTTOM,
                HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCLIENT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT,
                HTTOPRIGHT, ICON_BIG, ICON_SMALL, IMAGE_ICON, IsZoomed, LoadImageW,
                NCCALCSIZE_PARAMS, SM_CXICON, SM_CXPADDEDBORDER, SM_CXSIZEFRAME, SM_CXSMICON,
                SM_CYICON, SM_CYSIZEFRAME, SM_CYSMICON, SWP_FRAMECHANGED, SWP_NOACTIVATE,
                SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SendMessageW, SetWindowPos, WM_ERASEBKGND,
                WM_NCCALCSIZE, WM_NCDESTROY, WM_NCHITTEST, WM_SETICON, WM_SIZE, WS_CAPTION,
                WS_THICKFRAME,
            },
        },
    };

    const SUBCLASS: usize = 0x5253_4652;

    struct WindowIcons([HICON; 2]);

    impl Drop for WindowIcons {
        fn drop(&mut self) {
            for icon in self.0 {
                if !icon.is_null() {
                    unsafe { DestroyIcon(icon) };
                }
            }
        }
    }

    pub fn install(window: &rune::WindowHandle) -> Result<(), String> {
        window.with_raw_handle(|raw| {
            let raw_window_handle::RawWindowHandle::Win32(raw) = raw.as_raw() else {
                return Ok(());
            };
            let hwnd = raw.hwnd.get() as HWND;
            // This callback runs on the owning UI thread. No native pointer is retained.
            unsafe {
                let icons = Box::into_raw(Box::new(load_icons(hwnd)));
                if SetWindowSubclass(hwnd, Some(frame), SUBCLASS, icons as usize) == 0 {
                    drop(Box::from_raw(icons));
                    return Err("Could not install the native title-bar correction".into());
                }
                if SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    0,
                    0,
                    0,
                    0,
                    SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                ) == 0
                {
                    return Err("Could not recalculate the native window frame".into());
                }
                if !clear_backing(hwnd) {
                    return Err(
                        "Could not initialize the transparent Windows backing bitmap".into(),
                    );
                }
                for (kind, icon) in [ICON_SMALL, ICON_BIG].into_iter().zip((*icons).0) {
                    if !icon.is_null() {
                        SendMessageW(hwnd, WM_SETICON, kind as _, icon as _);
                    }
                }
            }
            Ok(())
        })?
    }

    unsafe fn load_icons(hwnd: HWND) -> WindowIcons {
        unsafe {
            let module = GetModuleHandleW(std::ptr::null());
            if module.is_null() {
                return WindowIcons([std::ptr::null_mut(); 2]);
            }
            let dpi = GetDpiForWindow(hwnd);
            WindowIcons([(SM_CXSMICON, SM_CYSMICON), (SM_CXICON, SM_CYICON)].map(
                |(width, height)| {
                    // Each window owns its two DPI-sized handles until WM_NCDESTROY.
                    LoadImageW(
                        module,
                        1usize as *const u16,
                        IMAGE_ICON,
                        GetSystemMetricsForDpi(width, dpi),
                        GetSystemMetricsForDpi(height, dpi),
                        0,
                    ) as HICON
                },
            ))
        }
    }

    unsafe extern "system" fn frame(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _: usize,
        icons: usize,
    ) -> LRESULT {
        // Rune keeps its caption buttons and maximized/fullscreen geometry. Restored
        // resize margins are painted by us so Acrylic cannot turn them into a grey rim.
        unsafe {
            if message == WM_ERASEBKGND {
                let mut client = RECT {
                    left: 0,
                    top: 0,
                    right: 0,
                    bottom: 0,
                };
                // The GPU visual sits over a separate GDI redirection bitmap. Rune's erase
                // handler does not clear its alpha; black GDI pixels initialize it to zero.
                if GetClientRect(hwnd, &mut client) != 0
                    && FillRect(wparam as _, &client, GetStockObject(BLACK_BRUSH) as _) != 0
                {
                    return 1;
                }
            }
            if message == WM_SIZE {
                let result = DefSubclassProc(hwnd, message, wparam, lparam);
                clear_backing(hwnd);
                return result;
            }
            if message == WM_NCCALCSIZE && wparam != 0 {
                let params = lparam as *mut NCCALCSIZE_PARAMS;
                let outer = (*params).rgrc[0];
                let result = DefSubclassProc(hwnd, message, wparam, lparam);
                let caption = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & WS_CAPTION == WS_CAPTION;
                if caption && IsZoomed(hwnd) == 0 {
                    (*params).rgrc[0] = outer;
                }
                return result;
            }
            if message == WM_NCHITTEST {
                let result = DefSubclassProc(hwnd, message, wparam, lparam);
                let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
                if result == HTCLIENT as LRESULT
                    && IsZoomed(hwnd) == 0
                    && style & WS_CAPTION == WS_CAPTION
                    && style & WS_THICKFRAME != 0
                {
                    let mut point = POINT {
                        x: lparam as i16 as i32,
                        y: (lparam >> 16) as i16 as i32,
                    };
                    let mut client = RECT {
                        left: 0,
                        top: 0,
                        right: 0,
                        bottom: 0,
                    };
                    if ScreenToClient(hwnd, &mut point) != 0
                        && GetClientRect(hwnd, &mut client) != 0
                    {
                        let dpi = GetDpiForWindow(hwnd);
                        let padding = GetSystemMetricsForDpi(SM_CXPADDEDBORDER, dpi);
                        return resize_hit(
                            point.x,
                            point.y,
                            client.right,
                            client.bottom,
                            GetSystemMetricsForDpi(SM_CXSIZEFRAME, dpi) + padding,
                            GetSystemMetricsForDpi(SM_CYSIZEFRAME, dpi) + padding,
                        ) as LRESULT;
                    }
                }
                return result;
            }
            if message == WM_NCDESTROY {
                RemoveWindowSubclass(hwnd, Some(frame), SUBCLASS);
                let result = DefSubclassProc(hwnd, message, wparam, lparam);
                drop(Box::from_raw(icons as *mut WindowIcons));
                return result;
            }
            DefSubclassProc(hwnd, message, wparam, lparam)
        }
    }

    unsafe fn clear_backing(hwnd: HWND) -> bool {
        unsafe {
            let dc = GetDC(hwnd);
            if dc.is_null() {
                return false;
            }
            let mut client = RECT {
                left: 0,
                top: 0,
                right: 0,
                bottom: 0,
            };
            let ok = GetClientRect(hwnd, &mut client) != 0
                && FillRect(dc, &client, GetStockObject(BLACK_BRUSH) as _) != 0;
            ReleaseDC(hwnd, dc);
            ok
        }
    }

    fn resize_hit(x: i32, y: i32, width: i32, height: i32, edge_x: i32, edge_y: i32) -> u32 {
        let left = x < edge_x;
        let right = x >= width - edge_x;
        let top = y < edge_y;
        let bottom = y >= height - edge_y;
        match (left, right, top, bottom) {
            (true, _, true, _) => HTTOPLEFT,
            (_, true, true, _) => HTTOPRIGHT,
            (true, _, _, true) => HTBOTTOMLEFT,
            (_, true, _, true) => HTBOTTOMRIGHT,
            (true, _, _, _) => HTLEFT,
            (_, true, _, _) => HTRIGHT,
            (_, _, true, _) => HTTOP,
            (_, _, _, true) => HTBOTTOM,
            _ => HTCLIENT,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn all_resize_edges_and_corners_work_at_different_scales() {
            for edge in [8, 12, 16] {
                for (x, y, hit) in [
                    (0, 0, HTTOPLEFT),
                    (500, 0, HTTOP),
                    (999, 0, HTTOPRIGHT),
                    (0, 300, HTLEFT),
                    (500, 300, HTCLIENT),
                    (999, 300, HTRIGHT),
                    (0, 599, HTBOTTOMLEFT),
                    (500, 599, HTBOTTOM),
                    (999, 599, HTBOTTOMRIGHT),
                ] {
                    assert_eq!(resize_hit(x, y, 1000, 600, edge, edge), hit);
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
pub use windows::install;
