param([int]$ProcessId = 0)
$ErrorActionPreference = 'Stop'

# Read-only native geometry check. Headless snapshots cannot see DWM's ghost caption.
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class ChronverFrameCheck {
    [StructLayout(LayoutKind.Sequential)] struct Rect { public int Left, Top, Right, Bottom; }
    [StructLayout(LayoutKind.Sequential)] struct Point { public int X, Y; }
    [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr hwnd, out Rect r);
    [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr hwnd, ref Point p);
    [DllImport("user32.dll")] static extern bool GetClientRect(IntPtr hwnd, out Rect r);
    [DllImport("user32.dll")] static extern bool IsZoomed(IntPtr hwnd);
    [DllImport("user32.dll")] static extern IntPtr SendMessageW(IntPtr hwnd, uint msg, IntPtr wparam, IntPtr lparam);
    [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr hwnd, int attr, out Rect r, int size);
    public static string Verify(IntPtr hwnd) {
        IntPtr previous = SetThreadDpiAwarenessContext(new IntPtr(-4));
        try { return VerifyPhysical(hwnd); }
        finally { if (previous != IntPtr.Zero) SetThreadDpiAwarenessContext(previous); }
    }
    static string VerifyPhysical(IntPtr hwnd) {
        Rect outer, visible; Point client = new Point();
        if (hwnd == IntPtr.Zero || !GetWindowRect(hwnd, out outer) || !ClientToScreen(hwnd, ref client))
            throw new Exception("Could not inspect the live Chronver window");
        if (DwmGetWindowAttribute(hwnd, 9, out visible, Marshal.SizeOf(typeof(Rect))) < 0)
            throw new Exception("Could not read DWM's visible frame bounds");
        if (visible.Top < outer.Top)
            throw new Exception(String.Format("DWM draws a ghost caption {0}px above the window", outer.Top - visible.Top));
        if (!IsZoomed(hwnd) && client.Y != outer.Top)
            throw new Exception(String.Format("Unexpected native top inset: {0}px", client.Y - outer.Top));
        if (!IsZoomed(hwnd)) {
            Rect size;
            if (!GetClientRect(hwnd, out size)) throw new Exception("Client size query failed");
            if (client.X != outer.Left || size.Right != outer.Right - outer.Left || size.Bottom != outer.Bottom - outer.Top)
                throw new Exception("Unpainted native resize gutters remain around the client area");
            int[,] checks = {
                {0,0,13}, {size.Right/2,0,12}, {size.Right-1,0,14},
                {0,size.Bottom/2,10}, {size.Right-1,size.Bottom/2,11},
                {0,size.Bottom-1,16}, {size.Right/2,size.Bottom-1,15}, {size.Right-1,size.Bottom-1,17}
            };
            for (int i = 0; i < checks.GetLength(0); i++) {
                int x = client.X + checks[i,0], y = client.Y + checks[i,1];
                int packed = ((y & 65535) << 16) | (x & 65535);
                long hit = SendMessageW(hwnd, 0x84, IntPtr.Zero, new IntPtr(packed)).ToInt64();
                if (hit != checks[i,2]) throw new Exception(String.Format("Resize hit test {0} failed: {1}", i, hit));
            }
        }
        return String.Format("PASS: no native gutters; resize edges/corners preserved; client top inset={0}px; DWM visible top={1}; window top={2}",
            client.Y - outer.Top, visible.Top, outer.Top);
    }
}
'@

$apps = if ($ProcessId) {
    @(Get-Process -Id $ProcessId)
} else {
    @(Get-Process -Name chronver, chronver-desktop -ErrorAction SilentlyContinue |
        Where-Object MainWindowHandle -ne 0)
}
if ($apps.Count -ne 1) { throw 'Specify -ProcessId when more than one Chronver is running' }
$apps[0].Refresh()
[ChronverFrameCheck]::Verify($apps[0].MainWindowHandle)
