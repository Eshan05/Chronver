function Get-LocalRegistrations {
    $uninstall='SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall'
    $wow='SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
    $roots=@(
        @{hive=[Microsoft.Win32.RegistryHive]::LocalMachine;scope='Machine';arch='X64';path=$uninstall},
        @{hive=[Microsoft.Win32.RegistryHive]::LocalMachine;scope='Machine';arch='X86';path=$wow},
        @{hive=[Microsoft.Win32.RegistryHive]::CurrentUser;scope='User';arch='X64';path=$uninstall},
        @{hive=[Microsoft.Win32.RegistryHive]::CurrentUser;scope='User';arch='X86';path=$wow}
    )
    foreach($r in $roots) {
        $base=$null;$parent=$null
        try {
            $base=[Microsoft.Win32.RegistryKey]::OpenBaseKey($r.hive,[Microsoft.Win32.RegistryView]::Registry64)
            $parent=$base.OpenSubKey($r.path,$false)
            if(-not $parent) { continue }
            foreach($name in $parent.GetSubKeyNames()) {
                $key=$null
                try {
                    $key=$parent.OpenSubKey($name,$false)
                    if(-not $key) { continue }
                    $display=[string]$key.GetValue('DisplayName')
                    if(-not $display.Trim()) { continue }
                    [pscustomobject]@{
                        RegistrationId=('ARP\'+$r.scope+'\'+$r.arch+'\'+$name)
                        DisplayName=$display;DisplayVersion=[string]$key.GetValue('DisplayVersion')
                        Publisher=[string]$key.GetValue('Publisher');InstallLocation=[string]$key.GetValue('InstallLocation')
                        DisplayIcon=[string]$key.GetValue('DisplayIcon');Comments=[string]$key.GetValue('Comments')
                        Homepage=[string]$key.GetValue('URLInfoAbout')
                        SystemComponent=$key.GetValue('SystemComponent');ParentKeyName=[string]$key.GetValue('ParentKeyName')
                        ReleaseType=[string]$key.GetValue('ReleaseType');Scope=$r.scope;Architecture=$r.arch
                        ProductCode=if($key.GetValue('WindowsInstaller') -eq 1 -and $name -match '^\{[0-9A-Fa-f-]{36}\}$'){$name}else{$null}
                    }
                } finally { if($key) { $key.Dispose() } }
            }
        } finally {
            if($parent) { $parent.Dispose() }
            if($base) { $base.Dispose() }
        }
    }
}

function Get-RegisteredIcon($reference) {
    $text=[Environment]::ExpandEnvironmentVariables([string]$reference).Trim()
    # Rune extracts the first native icon only. Do not silently show a different
    # resource when a registration explicitly requests another index.
    $m=[regex]::Match($text,'^(?:"([^"]+)"|(.+?))(?:,\s*(-?\d+))?$')
    if(-not $m.Success -or ($m.Groups[3].Success -and $m.Groups[3].Value -ne '0')) { return $null }
    $path=if($m.Groups[1].Success){$m.Groups[1].Value}else{$m.Groups[2].Value.Trim()}
    if($path -notmatch '^[a-zA-Z]:[\\/]' -or [IO.Path]::GetExtension($path) -notin @('.exe','.dll','.ico','.png','.jpg','.jpeg')) { return $null }
    try {
        $file=Get-Item -LiteralPath $path -ErrorAction Stop
        if($file.PSIsContainer -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)) { return $null }
        if($file.Extension -notin @('.exe','.dll') -and $file.Length -gt 4MB) { return $null }
        return $file.FullName
    } catch { return $null }
}
