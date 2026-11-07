function Read-ExtensionIconMetadata($registry,$installed) {
    $metadata=@{}
    try {
        if(-not(Test-Path -LiteralPath $registry) -or (Get-Item -LiteralPath $registry).Length -gt 8MB) {return $metadata}
        $records=Get-Content -LiteralPath $registry -Raw|ConvertFrom-Json
        foreach($line in $installed -split '\r?\n') {
            try {
            if($line -notmatch '^([A-Za-z0-9][A-Za-z0-9_.-]*\.[A-Za-z0-9][A-Za-z0-9_.-]*)@([^\s]+)$') {continue}
            $id=$Matches[1];$version=$Matches[2]
            $matching=@($records|Where-Object {$_.identifier.id -eq $id -and $_.version -eq $version})
            if($matching.Count -ne 1) {continue}
            $location=$matching[0].location
            if($location.scheme -ne 'file' -or $location.authority -or -not $location.path) {continue}
            $root=([uri]('file://'+$location.path)).LocalPath
            if($root -notmatch '^[A-Za-z]:[\\/]') {continue}
            $root=[IO.Path]::GetFullPath($root).TrimEnd('\')+'\'
            $manifest=Join-Path $root 'package.json'
            if(-not(Test-Path -LiteralPath $manifest) -or (Get-Item -LiteralPath $manifest).Length -gt 1MB) {continue}
            $package=Get-Content -LiteralPath $manifest -Raw|ConvertFrom-Json
            if(($package.publisher+'.'+$package.name) -ne $id -or $package.version -ne $version) {continue}
            $description=if($package.description -is [string]){$package.description}else{$null}
            if($description -match '^%([^%]+)%$') {
                $descriptionKey=$Matches[1];$description=$null
                try {
                    $nls=Get-Item -LiteralPath (Join-Path $root 'package.nls.json') -ErrorAction Stop
                    if(-not $nls.PSIsContainer -and $nls.Length -le 1MB -and -not ($nls.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                        $strings=Get-Content -LiteralPath $nls.FullName -Raw|ConvertFrom-Json
                        if($strings.$descriptionKey -is [string]) {$description=$strings.$descriptionKey}
                    }
                } catch {}
            }
            if($description) {$description=$description.Substring(0,[Math]::Min(4096,$description.Length))}
            $changelog=$null
            foreach($name in @('CHANGELOG.md','changelog.md','Changelog.md')) {
                $path=Join-Path $root $name
                if(Test-Path -LiteralPath $path -PathType Leaf) {$changelog=$path;break}
            }
            $repository=if($package.repository -is [string]){$package.repository}else{[string]$package.repository.url}
            $preRelease=if($null -ne $package.__metadata.isPreReleaseVersion){[bool]$package.__metadata.isPreReleaseVersion}
                elseif($null -ne $matching[0].metadata.isPreReleaseVersion){[bool]$matching[0].metadata.isPreReleaseVersion}else{$null}
            $metadata[$id]=@{description=$description;extension=@{root=$root;changelog=$changelog;repository=$repository;directory=[string]$package.repository.directory;
                pre_release=$preRelease;target_platform=if($package.__metadata.targetPlatform){[string]$package.__metadata.targetPlatform}else{[string]$matching[0].metadata.targetPlatform}}}
            $icon=[string]$package.icon
            if(-not $icon -or [IO.Path]::IsPathRooted($icon)) {continue}
            $target=[IO.Path]::GetFullPath([IO.Path]::Combine($root,$icon))
            if(-not $target.StartsWith($root,[StringComparison]::OrdinalIgnoreCase)) {continue}
            if([IO.Path]::GetExtension($target) -notin @('.png','.ico','.jpg','.jpeg','.webp')) {continue}
            $file=Get-Item -LiteralPath $target -ErrorAction Stop
            if($file.PSIsContainer -or $file.Length -gt 1MB -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)) {continue}
            $metadata[$id].iconPath=$file.FullName
            } catch {}
        }
    } catch {}
    return $metadata
}
