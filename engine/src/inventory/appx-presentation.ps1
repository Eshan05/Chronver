try { Add-Type -ErrorAction Stop -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class ReleaseScoutResources {
    [DllImport("shlwapi.dll", CharSet=CharSet.Unicode)]
    static extern int SHLoadIndirectString(string source, StringBuilder output, uint length, IntPtr reserved);
    public static string Resolve(string source) {
        var output=new StringBuilder(2048);
        return SHLoadIndirectString(source,output,(uint)output.Capacity,IntPtr.Zero)==0 ? output.ToString() : null;
    }
}
'@
} catch {}

function Resolve-AppxName($package,$reference) {
    $text=([string]$reference).Trim()
    if(-not $text) { return $null }
    if(-not $text.StartsWith('ms-resource:',[StringComparison]::OrdinalIgnoreCase)) { return $text }
    if(-not ('ReleaseScoutResources' -as [type])) { return $null }
    $suffix=$text.Substring(12)
    $refs=if($suffix.StartsWith('//')) {@($text)} elseif($suffix.StartsWith('/')) {
        @('ms-resource://'+$package.Name+$suffix)
    } else { @(('ms-resource://'+$package.Name+'/Resources/'+$suffix),('ms-resource://'+$package.Name+'/'+$suffix)) }
    foreach($ref in $refs) {
        $resolved=[ReleaseScoutResources]::Resolve('@{'+$package.PackageFullName+'?'+$ref+'}')
        if($resolved -and -not $resolved.StartsWith('ms-resource:',[StringComparison]::OrdinalIgnoreCase)) { return $resolved }
    }
    return $null
}

function Get-AppxIcon($package,$manifest) {
    $visual=$manifest.SelectSingleNode('//*[local-name()="VisualElements"]')
    $relative=if($visual){$visual.GetAttribute('Square44x44Logo')}else{$null}
    if(-not $relative) {
        $logo=$manifest.SelectSingleNode('//*[local-name()="Properties"]/*[local-name()="Logo"]')
        if($logo) { $relative=$logo.InnerText }
    }
    if(-not $relative) { return $null }
    if([IO.Path]::IsPathRooted($relative) -or $relative.StartsWith('ms-resource:')) { return $null }
    $root=[IO.Path]::GetFullPath($package.InstallLocation).TrimEnd('\')+'\'
    $path=[IO.Path]::GetFullPath([IO.Path]::Combine($root,$relative))
    if(-not $path.StartsWith($root,[StringComparison]::OrdinalIgnoreCase)) { return $null }
    $folder=[IO.Path]::GetDirectoryName($path);$stem=[IO.Path]::GetFileNameWithoutExtension($path);$ext=[IO.Path]::GetExtension($path)
    # Exact metadata, then bounded Windows asset qualifiers, not a recursive search.
    foreach($suffix in @('','.targetsize-48_altform-unplated','.targetsize-48','.scale-100','.scale-125','.scale-150','.scale-200')) {
        $icon=Get-RegisteredIcon ([IO.Path]::Combine($folder,$stem+$suffix+$ext))
        if($icon) { return $icon }
    }
    return $null
}

function Get-AppxExecutable($package,$manifest) {
    $app=$manifest.SelectSingleNode('//*[local-name()="Applications"]/*[local-name()="Application"]')
    if(-not $app) { return $null }
    $relative=$app.GetAttribute('Executable')
    if(-not $relative -or [IO.Path]::IsPathRooted($relative)) { return $null }
    $root=[IO.Path]::GetFullPath($package.InstallLocation).TrimEnd('\')+'\'
    $path=[IO.Path]::GetFullPath([IO.Path]::Combine($root,$relative))
    if($path.StartsWith($root,[StringComparison]::OrdinalIgnoreCase) -and [IO.Path]::GetExtension($path) -eq '.exe') {
        return Get-RegisteredIcon $path
    }
    return $null
}
