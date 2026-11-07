$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)
$OutputEncoding = [Console]::OutputEncoding
$env:GIT_TERMINAL_PROMPT = '0'
$env:GIT_OPTIONAL_LOCKS = '0'
$env:GCM_INTERACTIVE = 'Never'
$core = (& scoop prefix scoop | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or -not $core) { throw 'Scoop installation path unavailable' }
$root = Split-Path (Split-Path (Split-Path $core -Parent) -Parent) -Parent
$bucketRoot = Join-Path $root 'buckets'
if (-not (Get-Command git.exe -ErrorAction SilentlyContinue)) {
    @{fresh=$false;action='blocked';warnings=@('Git is unavailable; Scoop metadata was not refreshed.')} | ConvertTo-Json -Compress
    exit 0
}

function Read-Buckets {
    $ErrorActionPreference = 'Continue'
    $fresh = $true
    $blocked = $false
    $count = 0
    $stale = @()
    if (Test-Path -LiteralPath (Join-Path $core '.git')) {
        $coreDirty = (& git.exe -C $core status --porcelain --untracked-files=no | Out-String).Trim()
        if ($LASTEXITCODE -ne 0 -or $coreDirty) { $blocked = $true; $stale += 'Scoop core' }
    }
    foreach ($bucket in @(Get-ChildItem -LiteralPath $bucketRoot -Directory -ErrorAction Stop)) {
        if (-not (Test-Path -LiteralPath (Join-Path $bucket.FullName '.git'))) { continue }
        $count++
        $gitDir = (& git.exe -C $bucket.FullName rev-parse --absolute-git-dir | Out-String).Trim()
        if ($LASTEXITCODE -ne 0) { $blocked = $true; $stale += $bucket.Name; continue }
        if (@('index.lock','FETCH_HEAD.lock','shallow.lock') | Where-Object { Test-Path -LiteralPath (Join-Path $gitDir $_) }) { $blocked = $true; $stale += $bucket.Name; continue }
        $dirty = (& git.exe -C $bucket.FullName status --porcelain --untracked-files=no | Out-String).Trim()
        if ($LASTEXITCODE -ne 0 -or $dirty) { $blocked = $true; $stale += $bucket.Name; continue }
        $head = (& git.exe -C $bucket.FullName rev-parse --verify HEAD | Out-String).Trim()
        if ($LASTEXITCODE -ne 0) { $blocked = $true; $stale += $bucket.Name; continue }
        $upstream = (& git.exe -C $bucket.FullName rev-parse --verify '@{upstream}' 2>$null | Out-String).Trim()
        if ($LASTEXITCODE -ne 0 -or $head -ne $upstream) { $fresh = $false; $stale += $bucket.Name; continue }
        $fetch = (& git.exe -C $bucket.FullName rev-parse --git-path FETCH_HEAD | Out-String).Trim()
        if ($LASTEXITCODE -ne 0 -or -not $fetch) { $fresh = $false; $stale += $bucket.Name; continue }
        if (-not [IO.Path]::IsPathRooted($fetch)) { $fetch = Join-Path $bucket.FullName $fetch }
        if (-not (Test-Path -LiteralPath $fetch)) { $fresh = $false; $stale += $bucket.Name; continue }
        # FETCH_HEAD is tab-delimited and can list several branches; the first is not necessarily ours.
        $fetched = @(Get-Content -LiteralPath $fetch -TotalCount 1024 -ErrorAction Stop | ConvertFrom-Csv -Delimiter "`t" -Header 'Commit','Marker','Description')
        if (-not ($fetched | Where-Object { $_.Commit -eq $upstream -and $_.Marker -in @('', $null, 'not-for-merge') })) { $fresh = $false; $stale += $bucket.Name; continue }
        $age = ([DateTime]::UtcNow - (Get-Item -LiteralPath $fetch).LastWriteTimeUtc).TotalSeconds
        if ($age -lt 0 -or $age -ge 3600) { $fresh = $false; $stale += $bucket.Name }
    }
    @{fresh=($fresh -and -not $blocked -and $count -gt 0);blocked=$blocked;count=$count;stale=@($stale)}
}

$warnings = @()
$before = Read-Buckets
$action = 'recent'
if ($before.blocked) {
    $action = 'blocked'
    $warnings += 'Scoop buckets have local changes, an active Git lock, or unreadable Git metadata; automatic refresh skipped.'
    $warnings += ('Affected sources: ' + ($before.stale -join ', '))
} elseif ($before.count -eq 0) {
    $action = 'local-only'
    $warnings += 'No Git-backed Scoop buckets were found; local manifests are used without a remote freshness claim.'
} elseif (-not $before.fresh) {
    $action = 'updated'
    # No package name or wildcard: this updates Scoop itself and bucket metadata, not installed apps.
    $preference = $ErrorActionPreference
    try {
        # Git stderr is non-terminating in Scoop's normal shell; one dead bucket must not abort all others.
        $ErrorActionPreference = 'Continue'
        $log = @(& scoop update 2>&1 3>&1 4>&1 5>&1 6>&1 | ForEach-Object { $_.ToString() })
        $code = $LASTEXITCODE
    } catch {
        $code = 1
        $log = @($_.ToString())
    } finally { $ErrorActionPreference = $preference }
    if ($code -ne 0) { $warnings += "Scoop metadata refresh exited with code $code." }
    $after = Read-Buckets
    if ($code -ne 0 -or -not $after.fresh) {
        $action = 'failed'
        $warnings += 'Scoop bucket freshness could not be confirmed after refresh; candidates still use local manifests.'
        $warnings += ('Stale or unavailable sources: ' + ($after.stale -join ', '))
        $warnings += (($log | Select-Object -Last 3) -join ' ').Substring(0, [Math]::Min(600, (($log | Select-Object -Last 3) -join ' ').Length))
    }
    $before = $after
}
@{fresh=($before.fresh -and $action -in @('recent','updated'));action=$action;warnings=@($warnings)} | ConvertTo-Json -Depth 4 -Compress
