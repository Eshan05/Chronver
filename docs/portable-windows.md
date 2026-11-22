# Chronver for Windows x64

Download the `-windows-x64.zip` asset from the
[Releases page](https://github.com/Eshan05/Chronver/releases), extract it into a
folder you plan to keep, and open `Chronver.exe`. This is the desktop app, not
the separate `chronver` command-line tool. Windows may warn about or block the
unsigned build, depending on your security settings.

The ZIP does not install a Start menu shortcut. Chronver saves its data under
`%LOCALAPPDATA%\ReleaseScout`, not in the extracted folder. If you enable Hourly
Watch, keep the executable at the same path so the scheduled check can find it.

The matching source is available from the same release tag. The release also
includes a SHA-256 checksum for the ZIP. To compute the local checksum and
compare it with the published value, run:

```powershell
Get-FileHash -Algorithm SHA256 .\chronver-*-windows-x64.zip
```
