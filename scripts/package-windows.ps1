#requires -Version 7.0
param(
    [Parameter(Mandatory)]
    [ValidatePattern('^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$')]
    [string]$Version,
    [Parameter(Mandatory)]
    [string]$Executable,
    [Parameter(Mandatory)]
    [string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$files = [ordered]@{
    'Chronver.exe' = (Get-Item -LiteralPath $Executable).FullName
    'LICENSE' = Join-Path $root 'LICENSE'
    'Readme.md' = Join-Path $root 'docs/portable-windows.md'
    'notices/Geist-OFL.txt' = Join-Path $root 'desktop/assets/fonts/OFL.txt'
    'notices/Codicons-LICENSE' = Join-Path $root 'desktop/assets/codicons/LICENSE'
    'notices/Codicons-LICENSE-CODE' = Join-Path $root 'desktop/assets/codicons/LICENSE-CODE'
    'notices/Provider-icons.md' = Join-Path $root 'desktop/assets/providers/README.md'
    'notices/AI-provider-icons.md' = Join-Path $root 'desktop/assets/ai-providers/README.md'
    'notices/AI-provider-icons-LICENSE-MIT' = Join-Path $root 'desktop/assets/ai-providers/LICENSE-MIT'
}
foreach ($file in $files.Values) {
    if (!(Test-Path -LiteralPath $file -PathType Leaf)) {
        throw "Missing package input: $file"
    }
}

$directory = New-Item -ItemType Directory -Path $OutputDirectory -Force
$name = "chronver-v$Version-windows-x64.zip"
$path = Join-Path $directory.FullName $name
$archive = [IO.Compression.ZipFile]::Open($path, [IO.Compression.ZipArchiveMode]::Create)
try {
    foreach ($file in $files.GetEnumerator()) {
        $null = [IO.Compression.ZipFileExtensions]::CreateEntryFromFile(
            $archive, $file.Value, $file.Key, [IO.Compression.CompressionLevel]::Optimal
        )
    }
} finally {
    $archive.Dispose()
}

$hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
"$hash  $name`n" | Set-Content -LiteralPath "$path.sha256" -Encoding ascii -NoNewline
Write-Output $path
