$metadata=@{}; $metadataErrors=0; $manifests=@{}
$userRoot=if($env:SCOOP){$env:SCOOP}else{Join-Path $env:USERPROFILE 'scoop'}
$globalRoot=if($env:SCOOP_GLOBAL){$env:SCOOP_GLOBAL}else{Join-Path $env:ProgramData 'scoop'}
foreach($app in $installed.apps) {
    $id=$app.Name
    if($id -notmatch '^[A-Za-z0-9][A-Za-z0-9_.-]*$') { continue }
    try {
        $base=if([string]$app.Info -match 'Global install'){$globalRoot}else{$userRoot}
        $dir=Join-Path $base ('apps/'+$id+'/current')
        $path=Join-Path $dir 'scoop-manifest.json'
        if(-not(Test-Path -LiteralPath $path)) { $path=Join-Path $dir 'manifest.json' }
        if(-not(Test-Path -LiteralPath $path)) { continue }
        if((Get-Item -LiteralPath $path).Length -gt 1MB) { throw 'Scoop manifest too large' }
        $manifest=Get-Content -LiteralPath $path -Raw|ConvertFrom-Json
        $installFile=if([IO.Path]::GetFileName($path) -eq 'scoop-manifest.json'){'scoop-install.json'}else{'install.json'}
        foreach($file in @($installFile)) {
            $installPath=Join-Path $dir $file
            if(Test-Path -LiteralPath $installPath) {
                if((Get-Item -LiteralPath $installPath).Length -gt 1MB) { throw 'Scoop installation metadata too large' }
                $install=Get-Content -LiteralPath $installPath -Raw|ConvertFrom-Json
                $arch=$null
                if($install.architecture) { $arch=$manifest.architecture.([string]$install.architecture) }
                foreach($field in @('bin','shortcuts','env_add_path','env_set','url')) {
                    if($null -ne $arch.$field) { $manifest|Add-Member -NotePropertyName $field -NotePropertyValue $arch.$field -Force }
                }
                break
            }
        }
        $commands=$null -ne $manifest.bin -and @($manifest.bin).Count -gt 0
        foreach($relative in @($manifest.env_add_path)) {
            if(-not $relative -or $relative -match '\$') { continue }
            $pathDir=Join-Path $dir ([string]$relative)
            if(Test-Path -LiteralPath $pathDir) {
                $executables=@(Get-ChildItem -LiteralPath $pathDir -File -ErrorAction SilentlyContinue|
                    Where-Object {$_.Extension -in @('.exe','.cmd','.bat','.ps1')}|Select-Object -First 1)
                if($executables.Count -gt 0) { $commands=$true }
            }
        }
        $runtime=$null
        if($manifest.env_set.JAVA_HOME -and (Test-Path -LiteralPath (Join-Path $dir 'bin/java.exe'))) {
            $runtime=-not(Test-Path -LiteralPath (Join-Path $dir 'bin/javac.exe'))
        }
        $completionData=@($manifest.env_set.PSObject.Properties|Where-Object {$_.Name -match '(^|_)COMPLETIONS?(_|$)'}).Count -gt 0
        $urls=@($manifest.url|Where-Object {$_ -is [string]})
        $certificates=$urls.Count -gt 0
        foreach($url in $urls) {
            try {
                $uri=[uri]$url
                $filename=if($uri.Fragment){$uri.Fragment.TrimStart('#','/')}else{$uri.AbsolutePath}
                if([IO.Path]::GetExtension($filename) -notin @('.pem','.crt','.cer')) {$certificates=$false}
            } catch {$certificates=$false}
        }
        $metadata[$id]=@{description=[string]$manifest.description;hasCommands=[bool]$commands;
            hasShortcuts=($null -ne $manifest.shortcuts -and @($manifest.shortcuts).Count -gt 0);
            runtimeOnly=$runtime;supportData=($completionData -or $certificates);root=$dir;isDependency=$false;homepage=[string]$manifest.homepage}
        $manifests[$id]=$manifest
        $metadata[$id].repository=[string]$manifest.checkver.github
        $metadata[$id].releaseNotes=[string]$manifest.notesUrl
        $metadata[$id].downloads=@($manifest.url|Where-Object {$_ -is [string]})
        try {
            $root=[IO.Path]::GetFullPath($dir).TrimEnd('\')+'\'
            foreach($entry in @($manifest.shortcuts)+@($manifest.bin)) {
                $relative=if($entry -is [string]){$entry}else{@($entry)[0]}
                if(-not $relative -or [IO.Path]::IsPathRooted($relative)) {continue}
                $target=[IO.Path]::GetFullPath([IO.Path]::Combine($root,$relative))
                if(-not $target.StartsWith($root,[StringComparison]::OrdinalIgnoreCase) -or [IO.Path]::GetExtension($target) -notin @('.exe','.ico','.png')) {continue}
                $file=Get-Item -LiteralPath $target -ErrorAction SilentlyContinue
                if($file -and -not $file.PSIsContainer -and -not ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                    $metadata[$id].iconPath=$file.FullName;break
                }
            }
        } catch {}
    } catch { $metadataErrors++ }
}
foreach($manifest in $manifests.Values) {
    foreach($dependency in @($manifest.depends)) {
        $id=([string]$dependency -split '/')[-1]
        if($metadata.ContainsKey($id)) { $metadata[$id].isDependency=$true }
    }
}
