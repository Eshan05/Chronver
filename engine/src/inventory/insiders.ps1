# Shared reader for VS Code-family editors. The default preserves standalone Insiders fixtures.
if(-not $editorCommand) {
    $cli=(Get-Command code-insiders.cmd -ErrorAction Stop).Source
    $editorDataName='Code - Insiders';$editorExtensions='.vscode-insiders'
} else {
    $cli=$null
    foreach($found in @(Get-Command $editorCommand -CommandType Application -All -ErrorAction SilentlyContinue)) {
        $candidate=$found.Source
        if([IO.Path]::GetExtension($candidate) -eq '.cmd' -and (Get-Item -LiteralPath $candidate).Length -lt 64KB) {
            $head=Get-Content -LiteralPath $candidate -TotalCount 1
            if($head -match '^@rem ([A-Za-z]:\\.+\.cmd)$') {
                if(-not(Test-Path -LiteralPath $Matches[1])) {continue}
                $candidate=$Matches[1]
            }
        }
        $cli=$candidate;break
    }
    if(-not $cli) {
        $installed=$false
        $programRoots=@($env:ProgramFiles,${env:ProgramFiles(x86)},$env:ProgramW6432)
        if($env:LOCALAPPDATA) {$programRoots+=(Join-Path $env:LOCALAPPDATA 'Programs')}
        foreach($root in $programRoots | Where-Object {$_} | Select-Object -Unique) {
            $folder=Join-Path $root $editorFolder
            if(-not(Test-Path -LiteralPath (Join-Path $folder $editorExe) -PathType Leaf)) {continue}
            $installed=$true
            $candidate=Join-Path (Join-Path $folder 'bin') $editorCommand
            if(Test-Path -LiteralPath $candidate -PathType Leaf) {$cli=$candidate;break}
        }
        if(-not $cli) {
            # Profile data survives uninstall; it is not installation evidence.
            if(-not $installed) {
                $installed=$null -ne (Get-LocalRegistrations | Where-Object {
                    $_.Publisher -eq $editorPublisher -and
                    ($_.DisplayName -replace ' \((User|System)\)$','') -in $editorDisplayNames
                } | Select-Object -First 1)
            }
            if($installed) {@{editorCliUnavailable=$true}|ConvertTo-Json -Compress}
            else {@{profiles=@()}|ConvertTo-Json -Compress}
            return
        }
    }
}
$appRoot=Split-Path (Split-Path $cli -Parent) -Parent
$productPath=$null
$directory=Split-Path $cli -Parent
for($depth=0;$depth -lt 5 -and $directory;$depth++) {
    $candidates=@((Join-Path $directory 'resources/app/product.json'),(Join-Path $directory 'product.json'))
    if($depth -le 1) {
        $candidates+=@(Get-ChildItem -LiteralPath $directory -Directory|Sort-Object LastWriteTime -Descending|
            Select-Object -First 50|ForEach-Object {Join-Path $_.FullName 'resources/app/product.json'})
    }
    $productPath=$candidates|Where-Object {Test-Path -LiteralPath $_ -PathType Leaf}|Select-Object -First 1
    if($productPath) {break}
    $directory=Split-Path $directory -Parent
}
if(-not $productPath -or (Get-Item -LiteralPath $productPath).Length -gt 1MB) {throw 'Editor product metadata unavailable'}
$product=Get-Content -LiteralPath $productPath -Raw|ConvertFrom-Json
if($editorApplication) {
    if($product.applicationName -notin @($editorApplication)) {throw 'Resolved CLI belongs to a different editor'}
} elseif($product.quality -ne 'insider') {throw 'The resolved CLI is not VS Code Insiders'}
if($editorExtensions -eq '.vscode-insiders' -and $product.quality -ne 'insider') {throw 'The resolved CLI is not VS Code Insiders'}
$exe=$null
$directory=Split-Path $productPath -Parent
for($depth=0;$depth -lt 6 -and $directory;$depth++) {
    foreach($name in @($product.nameShort,$product.applicationName)|Where-Object {$_}) {
        $candidate=Join-Path $directory ($name+'.exe')
        if(Test-Path -LiteralPath $candidate -PathType Leaf) {$exe=$candidate;break}
    }
    if($exe) {$appRoot=$directory;break}
    $directory=Split-Path $directory -Parent
}
if($product.dataFolderName) {$editorExtensions=$product.dataFolderName}
if($product.nameShort) {$editorDataName=$product.nameShort}
$portable=if($env:VSCODE_PORTABLE -and $editorExtensions -in @('.vscode','.vscode-insiders')){$env:VSCODE_PORTABLE}else{Join-Path $appRoot 'data'}
$userData=if(Test-Path -LiteralPath $portable){Join-Path $portable 'user-data'}else{Join-Path $env:APPDATA $editorDataName}
$overrideName=switch($editorExtensions) {
    '.vscode-insiders' {'RELEASESCOUT_INSIDERS_USER_DATA'}
    '.vscode' {'RELEASESCOUT_VSCODE_USER_DATA'}
    '.cursor' {'RELEASESCOUT_CURSOR_USER_DATA'}
    default {throw 'Unsupported editor data directory'}
}
$override=[Environment]::GetEnvironmentVariable($overrideName)
if($override) { $userData=$override }
$userData=[IO.Path]::GetFullPath($userData)
$userDir=Join-Path $userData 'User'
if(-not(Test-Path -LiteralPath $userDir -PathType Container)) {
    @{profiles=@()}|ConvertTo-Json -Depth 8 -Compress
    return
}
$statePath=Join-Path $userDir 'globalStorage/storage.json'
$registered=@()
if(Test-Path -LiteralPath $statePath) {
    if((Get-Item -LiteralPath $statePath).Length -gt 8MB) { throw 'Editor profile registry too large' }
    $state=Get-Content -LiteralPath $statePath -Raw|ConvertFrom-Json
    if($null -ne $state.userDataProfiles) { $registered=@($state.userDataProfiles) }
}
if($registered.Count -gt 100) { throw 'Editor profile budget exceeded' }
function Quote-NativeArgument([string]$value) {
    $escaped=[regex]::Replace($value,'(\\*)"','$1$1\"')
    $escaped=[regex]::Replace($escaped,'(\\+)$','$1$1')
    return '"'+$escaped+'"'
}
function Read-InsidersExtensions($arguments) {
    $entry=Join-Path (Split-Path $productPath -Parent) 'out/cli.js'
    if($exe -and (Test-Path -LiteralPath $entry)) {
        # Avoid cmd.exe interpreting profile names or custom paths as commands.
        $info=[Diagnostics.ProcessStartInfo]::new();$info.FileName=$exe
        $info.Arguments=(@($entry)+$arguments|ForEach-Object {Quote-NativeArgument ([string]$_)}) -join ' '
        $info.UseShellExecute=$false;$info.CreateNoWindow=$true
        $info.EnvironmentVariables['ELECTRON_RUN_AS_NODE']='1'
        $info.EnvironmentVariables.Remove('VSCODE_DEV')
        $info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
        $info.StandardOutputEncoding=[Text.UTF8Encoding]::new($false)
        $info.StandardErrorEncoding=[Text.UTF8Encoding]::new($false)
        $process=[Diagnostics.Process]::Start($info)
        try {
            $out=$process.StandardOutput.ReadToEndAsync();$err=$process.StandardError.ReadToEndAsync()
            $process.WaitForExit()
            if($process.ExitCode -ne 0){throw ('Editor CLI failed: '+$err.Result)}
            return $out.Result
        } finally {$process.Dispose()}
    }
    # Wrapped command shims may conceal the native bootstrap. Fail closed for
    # shell metacharacters rather than interpreting profile data through cmd.
    foreach($argument in $arguments) {
        if([string]$argument -match '["&|<>^%!()\r\n]'){throw 'This wrapped editor CLI cannot safely accept the profile path/name'}
    }
    $body=& $cli @arguments
    if($LASTEXITCODE -ne 0){throw 'Editor extension listing failed'}
    return ($body|Out-String)
}
$profiles=@(@{id='__default__profile__';name='Default';root=$userDir;isDefault=$true;icon=$null})
foreach($profile in $registered) {
    if(-not $profile.name -or -not $profile.location) { throw 'Invalid registered editor profile' }
    $location=$profile.location
    if($location -is [string]) {
        $root=[IO.Path]::GetFullPath((Join-Path (Join-Path $userDir 'profiles') $location))
    } elseif($location.scheme -eq 'file' -and $location.path -and -not $location.authority) {
        $root=([uri]('file://'+$location.path)).LocalPath
    } else { throw 'Unsupported editor profile location' }
    $icon=if($profile.icon -is [string] -and $profile.icon -cmatch '^[a-z0-9-]{1,96}$'){$profile.icon}else{$null}
    $profiles+=@{id=$root;name=[string]$profile.name;root=$root;isDefault=$false;icon=$icon}
}
if($editorUpdatesOnly) {
    if(-not $exe) {throw 'Editor executable metadata unavailable'}
    $stream=[IO.File]::Open($exe,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
    $reader=[IO.BinaryReader]::new($stream)
    try {
        if($stream.Length -lt 64 -or $reader.ReadUInt16() -ne 0x5a4d) {throw 'Editor executable is not a Windows PE'}
        $stream.Position=0x3c
        $pe=$reader.ReadUInt32()
        if($pe -lt 64 -or $pe -gt 1MB -or $stream.Length -lt $pe+6) {throw 'Editor PE header unavailable'}
        $stream.Position=$pe
        if($reader.ReadUInt32() -ne 0x4550) {throw 'Invalid editor PE header'}
        $platform=switch($reader.ReadUInt16()) {
            0x8664 {'win32-x64'}
            0xaa64 {'win32-arm64'}
            0x14c {'win32-ia32'}
            default {throw 'Unsupported editor platform'}
        }
    } finally {$reader.Dispose()}
    $defaultRegistry=if(Test-Path -LiteralPath $portable) {Join-Path $portable 'extensions/extensions.json'}
        else {Join-Path (Join-Path $env:USERPROFILE $editorExtensions) 'extensions/extensions.json'}
    $registries=@(foreach($profile in $profiles) {
        $registry=if($profile.isDefault){$defaultRegistry}else{Join-Path $profile.root 'extensions.json'}
        @{id=$profile.id;path=$registry}
    })
    $version=if($editorExtensions -eq '.cursor') {$product.vscodeVersion} else {$product.version}
    @{version=$version;platform=$platform;date=$product.date;gallery=$product.extensionsGallery.serviceUrl;registries=$registries}|ConvertTo-Json -Depth 5 -Compress
    return
}
$results=@(foreach($profile in $profiles) {
    $arguments=@('--list-extensions','--show-versions',('--user-data-dir='+$userData))
    if(-not $profile.isDefault) { $arguments+=@('--profile='+$profile.name) }
    # Existing names come from the profile registry, never guessed or created.
    $body=Read-InsidersExtensions $arguments
    $metadata=@{}
    if(Test-Path 'Function:\Read-ExtensionIconMetadata') {
        $registry=if($profile.isDefault) {
            if(Test-Path -LiteralPath $portable) {Join-Path $portable 'extensions/extensions.json'}
            else {Join-Path (Join-Path $env:USERPROFILE $editorExtensions) 'extensions/extensions.json'}
        } else {Join-Path $profile.root 'extensions.json'}
        $metadata=Read-ExtensionIconMetadata $registry $body
        # Inherited profile membership can refer to the default extension registry.
        if(-not $profile.isDefault) {
            $defaultRegistry=if(Test-Path -LiteralPath $portable){Join-Path $portable 'extensions/extensions.json'}else{Join-Path (Join-Path $env:USERPROFILE $editorExtensions) 'extensions/extensions.json'}
            $fallback=Read-ExtensionIconMetadata $defaultRegistry $body
            foreach($id in $fallback.Keys) {if(-not $metadata.ContainsKey($id)) {$metadata[$id]=$fallback[$id]}}
        }
    }
    @{name=$profile.name;id=$profile.id;root=$profile.root;icon=$profile.icon;installed=$body;metadata=$metadata}
})
@{profiles=$results}|ConvertTo-Json -Depth 8 -Compress
