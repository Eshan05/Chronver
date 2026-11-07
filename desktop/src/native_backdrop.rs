use crate::model::State;

#[derive(Default)]
pub struct Backdrop {
    pub active: bool,
    #[cfg(target_os = "windows")]
    window: Option<rune::WindowHandle>,
    #[cfg(target_os = "windows")]
    applied_theme: Option<bool>,
}

pub fn configure(app: rune::App<State>) -> rune::App<State> {
    #[cfg(target_os = "windows")]
    {
        let handle = app.handle();
        app.window_options(rune::WindowOptions {
            transparent: true,
            ..Default::default()
        })
        .on_window(move |window| {
            if let Err(error) = crate::native_frame::install(window) {
                eprintln!("{}: {error}", releasescout_engine::APP_NAME);
                handle.update(|s, _| s.backdrop = Backdrop::default());
                return;
            }
            let window = window.clone();
            handle.update(move |s, _| {
                s.backdrop.window = Some(window);
                s.backdrop.applied_theme = None;
                s.backdrop.sync(s.dark);
            });
        })
        .on_focus(|s, _, _| {
            s.backdrop.applied_theme = None;
        })
    }
    #[cfg(not(target_os = "windows"))]
    app
}

impl Backdrop {
    pub fn sync(&mut self, dark: bool) -> bool {
        #[cfg(target_os = "windows")]
        {
            if self.applied_theme == Some(dark) {
                return false;
            }
            let Some(window) = &self.window else {
                return false;
            };
            let active = window
                .with_raw_handle(|raw| {
                    let raw_window_handle::RawWindowHandle::Win32(raw) = raw.as_raw() else {
                        return false;
                    };
                    // The handle is borrowed only during this callback on Rune's UI thread.
                    unsafe { windows::apply(raw.hwnd.get() as _, dark) }
                })
                .unwrap_or(false);
            self.applied_theme = Some(dark);
            let changed = self.active != active;
            self.active = active;
            changed
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = dark;
            false
        }
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use std::mem::size_of;
    use windows_sys::Win32::{
        Foundation::HWND,
        Graphics::Dwm::{
            DWMSBT_MAINWINDOW, DWMSBT_NONE, DWMWA_SYSTEMBACKDROP_TYPE,
            DWMWA_USE_IMMERSIVE_DARK_MODE, DwmExtendFrameIntoClientArea, DwmIsCompositionEnabled,
            DwmSetWindowAttribute,
        },
        System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW},
        UI::{
            Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
            Controls::MARGINS,
            WindowsAndMessaging::{SPI_GETHIGHCONTRAST, SystemParametersInfoW},
        },
    };
    use windows_sys::w;

    fn allowed() -> bool {
        let mut composition = 0;
        let mut contrast = HIGHCONTRASTW {
            cbSize: size_of::<HIGHCONTRASTW>() as _,
            dwFlags: 0,
            lpszDefaultScheme: std::ptr::null_mut(),
        };
        let mut transparency = 1u32;
        let mut bytes = size_of::<u32>() as u32;
        // All buffers have the documented size; RegGetValueW needs no owned key handle.
        unsafe {
            if DwmIsCompositionEnabled(&mut composition) < 0 || composition == 0 {
                return false;
            }
            if SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                contrast.cbSize,
                (&raw mut contrast).cast(),
                0,
            ) == 0
                || contrast.dwFlags & HCF_HIGHCONTRASTON != 0
            {
                return false;
            }
            let status = RegGetValueW(
                HKEY_CURRENT_USER,
                w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
                w!("EnableTransparency"),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&raw mut transparency).cast(),
                &mut bytes,
            );
            status != 0 || transparency != 0
        }
    }

    /// Call only with a live HWND borrowed on its owning thread.
    pub unsafe fn apply(hwnd: HWND, dark: bool) -> bool {
        let dark = i32::from(dark);
        // Uncover the shell only after an accepted material; rejected Mica is reset before
        // trying legacy Acrylic. Preference changes clear either effect before falling back.
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE as _,
                (&raw const dark).cast(),
                4,
            );
            if allowed() {
                let material = DWMSBT_MAINWINDOW;
                let margins = MARGINS {
                    cxLeftWidth: -1,
                    cxRightWidth: -1,
                    cyTopHeight: -1,
                    cyBottomHeight: -1,
                };
                if DwmSetWindowAttribute(
                    hwnd,
                    DWMWA_SYSTEMBACKDROP_TYPE as _,
                    (&raw const material).cast(),
                    4,
                ) >= 0
                    && DwmExtendFrameIntoClientArea(hwnd, &margins) >= 0
                {
                    crate::native_acrylic::clear(hwnd);
                    return true;
                }
                reset_mica(hwnd);
                if crate::native_acrylic::apply(hwnd, dark != 0) {
                    return true;
                }
            }
            crate::native_acrylic::clear(hwnd);
            reset_mica(hwnd);
        }
        false
    }

    unsafe fn reset_mica(hwnd: HWND) {
        unsafe {
            let material = DWMSBT_NONE;
            let margins = MARGINS {
                cxLeftWidth: 0,
                cxRightWidth: 0,
                cyTopHeight: 0,
                cyBottomHeight: 0,
            };
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_SYSTEMBACKDROP_TYPE as _,
                (&raw const material).cast(),
                4,
            );
            DwmExtendFrameIntoClientArea(hwnd, &margins);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_headless_or_unavailable_window_keeps_the_solid_fallback() {
        let mut backdrop = Backdrop::default();
        for dark in [true, false, true] {
            assert!(!backdrop.sync(dark));
            assert!(!backdrop.active);
        }
    }
}
