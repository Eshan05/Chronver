//! Windows 10 accent backdrop, using the native path used by window-vibrancy.
//! See docs/native-backdrops.md for provenance and the Win10 move/resize fallback.

use std::{ffi::c_void, mem::size_of, sync::OnceLock};
use windows_sys::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
    UI::{
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE, WM_NCDESTROY},
    },
};

#[repr(C)]
struct Policy {
    state: u32,
    flags: u32,
    color: u32,
    animation: u32,
}

#[repr(C)]
struct Attribute {
    attribute: u32,
    data: *mut c_void,
    size: usize,
}

type SetAttribute = unsafe extern "system" fn(HWND, *mut Attribute) -> i32;
const SUBCLASS: usize = 0x5253_4143;
const DISABLED: u32 = 0;
const BLUR: u32 = 3;
const ACRYLIC: u32 = 4;

fn tint(dark: bool) -> u32 {
    // Native accent colors are AABBGGRR, not RGBA. The UI applies the readability tint.
    if dark { 0x40_18_18_18 } else { 0x40_fa_fa_fa }
}

/// Call on the owning UI thread with a live, borrowed window handle.
pub unsafe fn apply(hwnd: HWND, dark: bool) -> bool {
    let color = tint(dark);
    unsafe {
        if !set(hwnd, ACRYLIC, color) {
            return false;
        }
        // Ref-data is just the packed tint, not an owned pointer. Updating the subclass
        // updates its color without allocating or retaining the HWND.
        if SetWindowSubclass(hwnd, Some(movement), SUBCLASS, color as usize) == 0 {
            set(hwnd, DISABLED, 0);
            return false;
        }
    }
    true
}

pub unsafe fn clear(hwnd: HWND) {
    unsafe {
        RemoveWindowSubclass(hwnd, Some(movement), SUBCLASS);
        set(hwnd, DISABLED, 0);
    }
}

unsafe fn set(hwnd: HWND, state: u32, color: u32) -> bool {
    static SET: OnceLock<Option<SetAttribute>> = OnceLock::new();
    let Some(set) = SET.get_or_init(|| unsafe {
        let module = GetModuleHandleW(windows_sys::w!("user32.dll"));
        GetProcAddress(module, c"SetWindowCompositionAttribute".as_ptr().cast())
            .map(|f| std::mem::transmute::<unsafe extern "system" fn() -> isize, SetAttribute>(f))
    }) else {
        return false;
    };
    let mut policy = Policy {
        state,
        flags: if state == BLUR { 2 } else { 0 },
        color,
        animation: 0,
    };
    let mut attribute = Attribute {
        attribute: 19, // WCA_ACCENT_POLICY
        data: (&raw mut policy).cast(),
        size: size_of::<Policy>(),
    };
    // Both buffers live through the synchronous call. A rejected request keeps the solid UI.
    unsafe { set(hwnd, &mut attribute) != 0 }
}

unsafe extern "system" fn movement(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    color: usize,
) -> LRESULT {
    unsafe {
        match message {
            WM_ENTERSIZEMOVE => {
                set(hwnd, BLUR, color as u32);
            }
            WM_EXITSIZEMOVE => {
                set(hwnd, ACRYLIC, color as u32);
            }
            WM_NCDESTROY => {
                RemoveWindowSubclass(hwnd, Some(movement), SUBCLASS);
            }
            _ => {}
        }
        DefSubclassProc(hwnd, message, wparam, lparam)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_layouts_and_tints_match_the_windows_accent_contract() {
        assert_eq!(size_of::<Policy>(), 16);
        assert_eq!(
            size_of::<Attribute>(),
            if size_of::<usize>() == 8 { 24 } else { 12 }
        );
        for dark in [false, true] {
            let color = tint(dark);
            assert_eq!(color >> 24, 64);
            assert_eq!(color & 255, (color >> 8) & 255);
            assert_eq!(color & 255, (color >> 16) & 255);
        }
    }
}
