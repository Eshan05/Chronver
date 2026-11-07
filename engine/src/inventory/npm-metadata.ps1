$metadata=@{}; $metadataErrors=0
try {
    # Inspect installed package manifests, never execute their lifecycle scripts.
    $groups=ConvertFrom-Json -InputObject $installed
    foreach($group in $groups) {
        foreach($property in $group.dependencies.PSObject.Properties) {
            $id=$property.Name
            if($id -notmatch '^(?:@[A-Za-z0-9_-][A-Za-z0-9_.-]*/)?[A-Za-z0-9_-][A-Za-z0-9_.-]*$') { continue }
            $packageDir=if($property.Value.path -is [string]){$property.Value.path}else{Join-Path $root $id}
            $path=[IO.Path]::Combine($packageDir,'package.json')
            try {
                if(-not(Test-Path -LiteralPath $path)) { continue }
                if((Get-Item -LiteralPath $path).Length -gt 1MB) { throw 'Package manifest too large' }
                $package=Get-Content -LiteralPath $path -Raw|ConvertFrom-Json
                if($package.name -ne $id) { continue }
                if($property.Value.version -and $package.version -ne $property.Value.version) { continue }
                $hasBin=($package.bin -is [string] -and $package.bin.Length -gt 0) -or
                    ($null -ne $package.bin -and @($package.bin.PSObject.Properties).Count -gt 0) -or
                    ($package.directories.bin -is [string] -and $package.directories.bin.Length -gt 0)
                $metadata[$id]=@{hasCommands=[bool]$hasBin;description=[string]$package.description;homepage=[string]$package.homepage}
                $metadata[$id].repository=if($package.repository -is [string]){$package.repository}else{[string]$package.repository.url}
                $metadata[$id].repositoryDirectory=[string]$package.repository.directory
                $metadata[$id].releaseNotes=[string]$package.releaseNotes
                $metadata[$id].packageRoot=$packageDir
            } catch { $metadataErrors++ }
        }
    }
} catch { $metadataErrors++ }
