$metadata=@{}; $metadataErrors=0
function Read-ReceiptXml($path) {
    if((Get-Item -LiteralPath $path).Length -gt 1MB) { throw 'Chocolatey receipt too large' }
    $settings=[Xml.XmlReaderSettings]::new()
    $settings.DtdProcessing=[Xml.DtdProcessing]::Prohibit; $settings.XmlResolver=$null
    $reader=[Xml.XmlReader]::Create($path,$settings)
    try { $doc=[Xml.XmlDocument]::new(); $doc.XmlResolver=$null; $doc.Load($reader); return $doc }
    finally { $reader.Dispose() }
}
foreach($line in $i) {
    $id=($line -split '\|')[0]
    if($id -notmatch '^[A-Za-z0-9_.-]+$') { continue }
    try {
        $dir=Join-Path $env:ChocolateyInstall ('lib/'+$id)
        $path=Join-Path $dir ($id+'.nuspec')
        if(-not(Test-Path -LiteralPath $path)) { continue }
        $doc=Read-ReceiptXml $path
        $tags=$doc.SelectSingleNode('//*[local-name()="metadata"]/*[local-name()="tags"]')
        $description=$doc.SelectSingleNode('//*[local-name()="metadata"]/*[local-name()="description"]')
        $deps=@($doc.SelectNodes('//*[local-name()="dependency"]'))
        $target=$null
        if($deps.Count -eq 1 -and $id -notlike '*.extension' -and $id -notmatch '^KB[0-9]+$') {
            $files=@(Get-ChildItem -LiteralPath $dir -File)
            $dirs=@(Get-ChildItem -LiteralPath $dir -Directory)
            $unknownDirs=@($dirs|Where-Object {$_.Name -notin @('tools','legal','_build')})
            $tools=Join-Path $dir 'tools'
            if(Test-Path -LiteralPath $tools) {
                $files+=@(Get-ChildItem -LiteralPath $tools -File)
                $unknownDirs+=@(Get-ChildItem -LiteralPath $tools -Directory)
            }
            $payload=@($files|Where-Object {$_.Extension -notin @('.nupkg','.nuspec','.md','.txt','.png','.jpg') -and $_.Name -ne 'update.ps1'})
            if($unknownDirs.Count -eq 0 -and $payload.Count -eq 0) { $target=$deps[0].GetAttribute('id') }
        }
        $text=if($description){$description.InnerText}else{''}
        $metadata[$id]=@{tags=if($tags){$tags.InnerText}else{''};wrapperTarget=$target;
            description=$text.Substring(0,[Math]::Min(4096,$text.Length))}
        foreach($field in @(@('projectUrl','homepage'),@('iconUrl','iconUrl'))) {
            $node=$doc.SelectSingleNode('//*[local-name()="metadata"]/*[local-name()="'+$field[0]+'"]')
            if($node) {$metadata[$id][$field[1]]=$node.InnerText}
        }
        foreach($field in @(@('projectSourceUrl','repository'),@('releaseNotes','releaseNotes'))) {
            $node=$doc.SelectSingleNode('//*[local-name()="metadata"]/*[local-name()="'+$field[0]+'"]')
            if($node) {$metadata[$id][$field[1]]=$node.InnerText}
        }
        $version=($line -split '\|')[1]
        if($version -notmatch '^[A-Za-z0-9_.+-]+$') { continue }
        $registryPath=Join-Path $env:ChocolateyInstall ('.chocolatey/'+$id+'.'+$version+'/.registry')
        if(Test-Path -LiteralPath $registryPath) {
            $receipt=Read-ReceiptXml $registryPath
            $locations=@($receipt.SelectNodes('//*[local-name()="key"]')|Where-Object {$_.InstallLocation})
            $libraryOnly=$locations.Count -gt 0
            foreach($key in $locations) {
                $location=$key.SelectSingleNode('*[local-name()="InstallLocation"]').InnerText
                if(-not(Test-Path -LiteralPath $location -PathType Container)) { $libraryOnly=$false; break }
                $uninstallers=@(foreach($command in $key.SelectNodes('*[local-name()="UninstallString" or local-name()="QuietUninstallString"]')) {
                    $m=[regex]::Match($command.InnerText,'^(?:"([^"]+)"|(\S+))')
                    if($m.Success) {
                        $uninstaller=if($m.Groups[1].Success){$m.Groups[1].Value}else{$m.Groups[2].Value}
                        # Receipt paths can use an 8.3 alias even when directory entries do not.
                        if($uninstaller -match '^[A-Za-z]:[\\/]'){[IO.Path]::GetFullPath($uninstaller)}else{$uninstaller}
                    }
                })
                $queue=[Collections.Generic.Queue[string]]::new();$queue.Enqueue($location)
                $files=[Collections.Generic.List[IO.FileInfo]]::new();$count=0;$complete=$true
                while($queue.Count -gt 0 -and $complete) {
                    foreach($entry in Get-ChildItem -LiteralPath $queue.Dequeue() -Force -ErrorAction Stop) {
                        $count++
                        if($count -gt 500 -or ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint)) { $complete=$false;break }
                        if($entry.PSIsContainer){$queue.Enqueue($entry.FullName)}else{$files.Add($entry)}
                    }
                }
                if(-not $complete) { $libraryOnly=$false; break }
                $dlls=@($files|Where-Object {$_.Extension -eq '.dll'})
                $commands=@($files|Where-Object {$_.Extension -in @('.exe','.com','.cmd','.bat','.ps1') -and [IO.Path]::GetFullPath($_.FullName) -notin $uninstallers})
                if($dlls.Count -eq 0 -or $commands.Count -gt 0) { $libraryOnly=$false; break }
            }
            if($libraryOnly) { $metadata[$id].libraryOnly=$true }
        }
    } catch { $metadataErrors++ }
}
