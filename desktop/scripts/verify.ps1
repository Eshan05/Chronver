$ErrorActionPreference = 'Stop'
Push-Location (Split-Path $PSScriptRoot -Parent)
try {
    foreach ($command in @(
        @('fmt', '--all', '--check'),
        @('test', '-j', '1', '--locked'),
        @('clippy', '-j', '1', '--all-targets', '--locked', '--', '-D', 'warnings'),
        @('build', '-j', '1', '--locked')
    )) {
        & cargo @command
        if ($LASTEXITCODE -ne 0) { throw "cargo $command failed" }
    }
    $metadata = & cargo metadata --locked --no-deps --format-version 1
    if ($LASTEXITCODE -ne 0) { throw 'Could not locate the Cargo workspace target directory' }
    $binary = Join-Path (($metadata | ConvertFrom-Json).target_directory) 'debug/chronver-desktop.exe'
    [void](New-Item -ItemType Directory -Force -Path 'artifacts')
    $scenes = @(
        @{ name = 'mica-surfaces'; scene = 'mica-surfaces'; size = '1024x640' },
        @{ name = 'mica-surfaces-light'; scene = 'mica-surfaces-light'; size = '1024x640' },
        @{ name = 'reader-software'; scene = 'reader-software'; size = '1024x640' },
        @{ name = 'reader-software-light'; scene = 'reader-software-light'; size = '1280x800' },
        @{ name = 'reader-extension'; scene = 'reader-extension'; size = '1024x640' },
        @{ name = 'reader-extension-light'; scene = 'reader-extension-light'; size = '1280x800' },
        @{ name = 'reader-packages'; scene = 'reader-packages'; size = '1024x640' },
        @{ name = 'reader-packages-light'; scene = 'reader-packages-light'; size = '1280x800' },
        @{ name = 'command'; scene = 'command'; size = '1024x640' },
        @{ name = 'command-light'; scene = 'command-light'; size = '1280x800' },
        @{ name = 'command-query'; scene = 'command-query'; size = '1024x640' },
        @{ name = 'custom-icon'; scene = 'custom-icon'; size = '1024x640' },
        @{ name = 'sidebar-status'; scene = 'sidebar-status'; size = '1024x640' },
        @{ name = 'sidebar-status-light'; scene = 'sidebar-status-light'; size = '1024x640' },
        @{ name = 'provider-badges'; scene = 'provider-badges'; size = '1280x900' },
        @{ name = 'provider-badges-light'; scene = 'provider-badges-light'; size = '1024x640' },
        @{ name = 'watchlist'; scene = 'watchlist'; size = '1024x640' },
        @{ name = 'watchlist-desktop'; scene = 'watchlist'; size = '1280x800' },
        @{ name = 'packages-empty'; scene = 'packages-empty'; size = '1024x640' },
        @{ name = 'packages'; scene = 'packages'; size = '1024x640' },
        @{ name = 'packages-desktop'; scene = 'packages'; size = '1280x800' },
        @{ name = 'packages-observations'; scene = 'packages-observations'; size = '1024x640' },
        @{ name = 'source-discovery'; scene = 'source-discovery'; size = '1024x640' },
        @{ name = 'source-discovered'; scene = 'source-discovered'; size = '1024x640' },
        @{ name = 'extension-notes'; scene = 'extension-notes'; size = '1024x640' },
        @{ name = 'extension-notes-desktop'; scene = 'extension-notes'; size = '1280x800' },
        @{ name = 'local-apps'; scene = 'local-apps'; size = '1024x640' },
        @{ name = 'extensions'; scene = 'extensions'; size = '1024x640' },
        @{ name = 'extensions-desktop'; scene = 'extensions'; size = '1280x800' },
        @{ name = 'extensions-empty'; scene = 'extensions-empty'; size = '1024x640' },
        @{ name = 'portable-filtering'; scene = 'portable-filtering'; size = '1024x640' },
        @{ name = 'portable-filtering-desktop'; scene = 'portable-filtering'; size = '1280x800' },
        @{ name = 'portable-support'; scene = 'portable-support'; size = '1024x640' },
        @{ name = 'profile-observations'; scene = 'profile-observations'; size = '1024x640' },
        @{ name = 'reconciled'; scene = 'reconciled'; size = '1024x640' },
        @{ name = 'reconciled-desktop'; scene = 'reconciled'; size = '1280x800' },
        @{ name = 'reconciled-observations'; scene = 'reconciled-observations'; size = '1024x640' },
        @{ name = 'automatic-filtering'; scene = 'automatic-filtering'; size = '1024x640' },
        @{ name = 'automatic-components'; scene = 'automatic-components'; size = '1024x640' },
        @{ name = 'grouped'; scene = 'grouped'; size = '1024x640' },
        @{ name = 'grouped-desktop'; scene = 'grouped'; size = '1280x800' },
        @{ name = 'components-visible'; scene = 'components-visible'; size = '1024x640' },
        @{ name = 'observations-open'; scene = 'observations-open'; size = '1024x640' },
        @{ name = 'detected'; scene = 'detected'; size = '1024x640' },
        @{ name = 'inventory'; scene = 'inventory'; size = '1024x640' },
        @{ name = 'inventory-results'; scene = 'inventory-results'; size = '1024x640' },
        @{ name = 'inventory-loading'; scene = 'inventory-loading'; size = '1280x800' },
        @{ name = 'inventory-error'; scene = 'inventory-error'; size = '1024x640' },
        @{ name = 'engine-partial'; scene = 'engine-partial'; size = '1024x640' },
        @{ name = 'engine-range'; scene = 'engine-range'; size = '1024x640' },
        @{ name = 'engine-components'; scene = 'engine-components'; size = '1024x640' },
        @{ name = 'engine-loading'; scene = 'engine-loading'; size = '1280x800' },
        @{ name = 'engine-error'; scene = 'engine-error'; size = '1024x640' },
        @{ name = 'desktop'; size = '1280x800' },
        @{ name = 'dark'; scene = 'dark'; size = '1280x800' },
        @{ name = 'light'; scene = 'light'; size = '1280x800' },
        @{ name = 'options'; scene = 'options'; size = '1024x640' },
        @{ name = 'collapsed'; scene = 'collapsed'; size = '1024x640' },
        @{ name = 'minimum'; scene = 'notes'; size = '1024x640' },
        @{ name = 'notes'; scene = 'notes'; size = '1280x800' },
        @{ name = 'unconfirmed'; scene = 'unconfirmed'; size = '1024x640' },
        @{ name = 'long-reference'; scene = 'long-reference'; size = '1024x640' },
        @{ name = 'app'; scene = 'app'; size = '1280x800' },
        @{ name = 'removed-app'; scene = 'removed-app'; size = '1024x640' },
        @{ name = 'stress'; scene = 'stress'; size = '1280x800' },
        @{ name = 'source'; scene = 'source'; size = '1024x640' },
        @{ name = 'empty'; scene = 'empty'; size = '1024x640' }
    )
    Add-Type -AssemblyName System.Drawing
    foreach ($scene in $scenes) {
        $args = @('--snap', "artifacts/$($scene.name).png", '--tree', "artifacts/$($scene.name).json", '--size', $scene.size, '--scale', '1')
        if ($scene.scene) { $args += @('--scene', $scene.scene) }
        # PowerShell does not synchronously wait for a GUI-subsystem executable.
        $process = Start-Process -FilePath $binary -ArgumentList $args -Wait -PassThru -WindowStyle Hidden
        if ($process.ExitCode -ne 0) { throw "Rendering $($scene.name) failed: $($process.ExitCode)" }
        $bitmap = [Drawing.Bitmap]::new((Join-Path (Get-Location) "artifacts/$($scene.name).png"))
        try {
            $colors = [Collections.Generic.HashSet[int]]::new()
            for ($x = 0; $x -lt $bitmap.Width; $x += 7) {
                for ($y = 0; $y -lt $bitmap.Height; $y += 7) {
                    [void]$colors.Add($bitmap.GetPixel($x, $y).ToArgb())
                }
            }
            if ($colors.Count -lt 12) { throw "Suspiciously blank render: $($scene.name)" }
            if ($scene.name -like 'mica-surfaces*') {
                foreach ($point in @(@(10, 500), @(600, 5))) {
                    $alpha = $bitmap.GetPixel($point[0], $point[1]).A
                    if ($alpha -lt 205 -or $alpha -gt 225) { throw "Backdrop shell must retain a readable, translucent tint: $alpha" }
                }
                if ($bitmap.GetPixel(600, 600).A -ne 255) { throw 'The notes surface must stay opaque' }
            }
            Write-Host "$($scene.name): $($bitmap.Width)x$($bitmap.Height), $($colors.Count) sampled colors"
        } finally { $bitmap.Dispose() }
    }
} finally { Pop-Location }
