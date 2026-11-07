$starts=@(Get-StartApps -ErrorAction SilentlyContinue)
$inbox=$null
try {
    $inbox=@(Get-ChildItem -LiteralPath 'HKLM:/SOFTWARE/Microsoft/Windows/CurrentVersion/Appx/AppxAllUserStore/InboxApplications' -ErrorAction Stop|Select-Object -ExpandProperty PSChildName)
} catch {}
$records=@(foreach($p in Get-AppxPackage -ErrorAction Stop|Where-Object {-not $_.IsFramework -and -not $_.IsResourcePackage}) {
    $family=$p.PackageFamilyName+'!'
    $names=@($starts|Where-Object {$_.AppID.StartsWith($family)}|Select-Object -ExpandProperty Name -Unique)
    $visible=$null; $alias=$null; $cliAlias=$null; $preinstalled=$null; $isInbox=$null;$friendly=$null;$icon=$null
    if($null -ne $inbox) {
        $isInbox=@($inbox|Where-Object {$_.StartsWith($p.Name+'_') -and $_.EndsWith('_'+$p.PublisherId)}).Count -gt 0
    }
    try {
        $manifest=Get-AppxPackageManifest -Package $p.PackageFullName -ErrorAction Stop
        try {
        $display=$manifest.SelectSingleNode('//*[local-name()="Properties"]/*[local-name()="DisplayName"]')
        if($display) { $friendly=Resolve-AppxName $p $display.InnerText }
        if(-not $friendly) {
            $display=$manifest.SelectSingleNode('//*[local-name()="VisualElements"]')
            if($display) { $friendly=Resolve-AppxName $p $display.GetAttribute('DisplayName') }
        }
        $icon=Get-AppxIcon $p $manifest
        if(-not $friendly -or -not $icon) {
            $exe=Get-AppxExecutable $p $manifest
            if($exe) {
                if(-not $icon) { $icon=$exe }
                if(-not $friendly) {
                    $friendly=[Diagnostics.FileVersionInfo]::GetVersionInfo($exe).FileDescription
                }
            }
        }
        } catch {}
        $apps=@($manifest.SelectNodes('//*[local-name()="Applications"]/*[local-name()="Application"]'))
        $v=@($apps|Where-Object {$el=$_.SelectSingleNode('*[local-name()="VisualElements"]'); $el -and $el.GetAttribute('AppListEntry') -ne 'none'})
        $visible=$v.Count -gt 0
        $aliases=@($manifest.SelectNodes('//*[local-name()="ExecutionAlias"]'))
        $alias=$aliases.Count -gt 0
        $cliAlias=$false;$unknownAlias=$false
        foreach($entry in $aliases) {
            $extension=$entry.ParentNode.ParentNode
            $executable=$extension.GetAttribute('Executable')
            if(-not $executable) {
                $application=$extension
                while($application -and $application.LocalName -ne 'Application') { $application=$application.ParentNode }
                if($application) { $executable=$application.GetAttribute('Executable') }
            }
            $console=$null
            if($executable) {
                $root=[IO.Path]::GetFullPath($p.InstallLocation).TrimEnd('\')+'\'
                $target=[IO.Path]::GetFullPath([IO.Path]::Combine($root,$executable))
                if($target.StartsWith($root,[StringComparison]::OrdinalIgnoreCase)) { $console=Test-ConsoleExecutable $target }
            }
            if($console -eq $true) { $cliAlias=$true }
            if($null -eq $console) { $unknownAlias=$true }
        }
        if(-not $cliAlias -and $unknownAlias) { $cliAlias=$null }
        $preinstalled=@($manifest.SelectNodes('//*[local-name()="Extension" and @Category="windows.preInstalledConfigTask"]')).Count -gt 0
    } catch {}
    if($names.Count -gt 0) { $visible=$true }
    if($names.Count -gt 0) { $friendly=$names -join ', ' }
    [pscustomobject]@{Name=$p.Name;PackageFamilyName=$p.PackageFamilyName;Version=$p.Version.ToString();
        InstallLocation=$p.InstallLocation;NonRemovable=$p.NonRemovable;SignatureKind=$p.SignatureKind.ToString();
        Publisher=$p.Publisher;IsDevelopmentMode=$p.IsDevelopmentMode;IsInbox=$isInbox;HasPreinstalledTask=$preinstalled;
        HasVisibleApp=$visible;HasExecutionAlias=$alias;HasCliAlias=$cliAlias;FriendlyName=$friendly;IconPath=$icon}
})
ConvertTo-Json -Depth 5 -Compress -InputObject $records
