# Native Backdrops

The shell tries native Mica first, then native Acrylic, then the solid neutral
fallback. Only a successful native call exposes the sidebar and header; the
notes pane remains opaque. An 84%-opaque neutral tint over the material bounds its
brightness independently of Windows' native fallback. Material-shell text also
uses stronger secondary ink. Contrast tests cover white, black and saturated
underlying surfaces in both themes, including selected and hovered rows, with a
4.5:1 minimum for primary and secondary text. This avoids assuming the native tint
matches the app theme.
High contrast, disabled transparency or unavailable
composition disable both effects. No wallpaper screenshots or imitation gradient
are used, and inventory/source behavior is unchanged.

Windows 10 Acrylic uses `SetWindowCompositionAttribute` with `WCA_ACCENT_POLICY`.
This is an undocumented native path, also used by Tauri's window-vibrancy helper.
Our adapter resolves it dynamically and checks its BOOL result instead of treating
API availability as success. Unsupported systems retain the solid background.
Native tints use restrained neutral AABBGGRR values; the renderer, not the accent
API, owns the final readability tint.

Windows 10 v1903+ has a documented-in-window-vibrancy drag/resize slowdown with
Acrylic. During `WM_ENTERSIZEMOVE` the native subclass switches to the lighter
blur-behind state with the same tint, then restores Acrylic on `WM_EXITSIZEMOVE`.
The UI does not become clear glass during this transition. No timers, background
polling or OS-wide settings changes are involved. This path still needs human
evaluation for appearance and drag/resize smoothness on different GPUs.

## Native Pixel Regression

Rune's DirectComposition visual is layered over a separate Windows GDI redirection
bitmap. Its existing erase handler returns without clearing that bitmap, leaving
an opaque white/stale rectangle beneath translucent GPU pixels. `native_frame.rs`
initializes it to zero alpha with black GDI pixels at creation, background erase
and resize. Clearing only the GPU framebuffer or accepting an Acrylic API call
does not fix that separate surface.

The opt-in example creates its own solid red/blue and checkerboard background,
plus a transparent foreground with an opaque black pane. It samples only those
owned test areas; no input automation or desktop screenshots are used. It asserts
that changing the actual background changes foreground pixels, that Acrylic
reduces checkerboard contrast, and that the opaque pane stays black. Resize/move
mode also samples a newly exposed area beyond the original foreground width.
Pixel sampling runs on a worker after each background repaint settles, keeping
both test windows' UI threads responsive instead of perturbing the native effect
with a long, blocking readback callback.

Run from an interactive Windows desktop (the temporary windows close themselves):

```powershell
rtk cargo run -j 1 --locked --example native_backdrop_probe
rtk cargo run -j 1 --locked --example native_backdrop_probe -- --transparent-only
rtk cargo run -j 1 --locked --example native_backdrop_probe -- --resize --move
```

Before the backing-bitmap fix, both colored backgrounds sampled white and the
response assertion failed. With the fix, the native test observes red/blue changes
and a blurred checkerboard, including after the resize/move. The transparent-only
control preserves the unblurred black/white checker instead.

The accent ABI and state/flag choices were checked against the MIT/Apache-2.0
window-vibrancy implementation:
https://github.com/tauri-apps/window-vibrancy/blob/dev/src/windows.rs

The code is an independently scoped native adapter, not a Tauri dependency.
Reference copyright: 2019-2022 Tauri Programme within The Commons Conservancy.
