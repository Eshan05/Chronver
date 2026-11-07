$metadata=@{}; $metadataErrors=0
function Same-InstalledVersion($a,$b) {
    if($a -eq $b) { return $true }
    $va=$null;$vb=$null
    if(-not[version]::TryParse([string]$a,[ref]$va) -or -not[version]::TryParse([string]$b,[ref]$vb)) { return $false }
    return $va.Major -eq $vb.Major -and $va.Minor -eq $vb.Minor -and
        [Math]::Max(0,$va.Build) -eq [Math]::Max(0,$vb.Build) -and [Math]::Max(0,$va.Revision) -eq [Math]::Max(0,$vb.Revision)
}
function Registered-Executable($command) {
    $text=[Environment]::ExpandEnvironmentVariables([string]$command).Trim()
    $m=[regex]::Match($text,'^(?:"([^"]+\.exe)"|(.+?\.exe)(?=\s|,|$))',[Text.RegularExpressions.RegexOptions]::IgnoreCase)
    if($m.Success) { if($m.Groups[1].Success){return $m.Groups[1].Value}else{return $m.Groups[2].Value} }
    return $null
}
function Startup-OnlyMetadata {
    $starts=[Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $shortcuts=[Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    $runPaths=@('HKCU:/Software/Microsoft/Windows/CurrentVersion/Run','HKLM:/Software/Microsoft/Windows/CurrentVersion/Run',
        'HKLM:/Software/WOW6432Node/Microsoft/Windows/CurrentVersion/Run')
    foreach($path in $runPaths) {
        if(-not(Test-Path -LiteralPath $path)) { continue }
        $properties=(Get-ItemProperty -LiteralPath $path).PSObject.Properties
        foreach($property in $properties|Where-Object {$_.Name -notmatch '^PS'}) {
            $exe=Registered-Executable $property.Value
            if($exe){[void]$starts.Add($exe)}
        }
    }
    $shell=New-Object -ComObject WScript.Shell
    $count=0
    foreach($folder in @('StartMenu','CommonStartMenu','DesktopDirectory','CommonDesktopDirectory')) {
        $path=[Environment]::GetFolderPath($folder)
        if(-not $path -or -not(Test-Path -LiteralPath $path)) { continue }
        foreach($file in Get-ChildItem -LiteralPath $path -Filter '*.lnk' -File -Recurse -ErrorAction Stop) {
            $count++;if($count -gt 2000) { throw 'Shortcut classification budget exceeded' }
            # CreateShortcut reads existing links; Save is never called.
            $target=$shell.CreateShortcut($file.FullName).TargetPath
            if($target){[void]$shortcuts.Add($target)}
        }
    }
    return @{starts=$starts;shortcuts=$shortcuts}
}
$startup=$null
try {$startup=Startup-OnlyMetadata}catch{$metadataErrors++}
try {
    $lines=@(& winget.exe list --disable-interactivity)
    if($LASTEXITCODE -ne 0) { throw 'WinGet display metadata unavailable' }
    $arp=@(Get-LocalRegistrations)
    foreach($source in $i.Sources) {
        foreach($package in $source.Packages) {
            $id=[string]$package.PackageIdentifier
            $matches=@($lines|Where-Object {$_ -match ('\s'+[regex]::Escape($id)+'\s')})
            if($matches.Count -ne 1) { continue }
            $m=[regex]::Match($matches[0], ('^(.*?)\s+'+[regex]::Escape($id)+'\s+(.+)$'))
            $name=$m.Groups[1].Value.Trim()
            if(-not $m.Success -or $name.Contains([string][char]0x2026) -or $name.EndsWith('...')) { continue }
            $key=([string]$source.SourceDetails.Name)+'/'+$id
            $item=@{name=$name}
            # A truncated, ambiguous or version-mismatched ARP join is not evidence.
            $receipts=@($arp|Where-Object {$_.DisplayName -eq $name -and (Same-InstalledVersion $_.DisplayVersion $package.Version)})
            if($receipts.Count -eq 1) {
                $r=$receipts[0]
                $item.registrationId=$r.RegistrationId
                $item.homepage=$r.Homepage
                $item.iconPath=Get-RegisteredIcon $r.DisplayIcon
                $item.systemComponent=if($null -ne $r.SystemComponent){[bool]$r.SystemComponent}else{$null}
                $item.parentPackage=[string]$r.ParentKeyName
                $item.releaseType=[string]$r.ReleaseType
                $item.publisher=[string]$r.Publisher
                $item.description=[string]$r.Comments
                $exe=Registered-Executable $r.DisplayIcon
                if($startup -and $exe -and $startup.starts.Contains($exe) -and -not $startup.shortcuts.Contains($exe)) {
                    # GUI aliases are not console tools; unknown headers fail open.
                    if((Test-ConsoleExecutable $exe) -eq $false) { $item.backgroundOnly=$true }
                }
            }
            $metadata[$key]=$item
        }
    }
} catch { $metadataErrors++ }
