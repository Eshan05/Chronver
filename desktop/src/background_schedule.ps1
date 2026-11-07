$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$taskName = 'Chronver Background Watch'
$description = 'Chronver: hourly read-only watch for published releases'
$exe = $env:CHRONVER_WATCH_EXE
$argument = $env:CHRONVER_WATCH_ACTION
$user = [Security.Principal.WindowsIdentity]::GetCurrent().Name
$prior = Get-ScheduledTask -TaskName $taskName -TaskPath '\' -ErrorAction SilentlyContinue
if ($prior -and ($prior.Description -cne $description -or @($prior.Actions).Count -ne 1 -or
                  $prior.Actions[0].Arguments -cne $argument -or $prior.Actions[0].Execute -ne $exe)) {
    throw 'An unrelated scheduled task already uses the Background Watch name.'
}
if ($env:CHRONVER_WATCH_ENABLE -ne '1') {
    if ($prior) { Unregister-ScheduledTask -TaskName $taskName -TaskPath '\' -Confirm:$false }
    return
}
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw 'Chronver executable is missing.' }
$appId = Get-StartApps -Name Chronver | Where-Object {$_.Name -ceq 'Chronver' -and $_.AppID -eq $exe} | Select-Object -First 1
if (-not $appId) { throw 'Background Watch requires the installed Chronver Start menu shortcut.' }
$scheme = 'HKCU:\Software\Classes\chronver-watch'
$command = '"'+$exe+'" "%1"'
$existing = Get-Item -LiteralPath "$scheme\shell\open\command" -ErrorAction SilentlyContinue
if ((Test-Path -LiteralPath $scheme) -and (-not $existing -or $existing.GetValue('') -ne $command)) {
    throw 'The Chronver notification link is already registered to a different app.'
}
if (-not $existing) {
    $null = New-Item -Path $scheme -Force
    $null = Set-Item -LiteralPath $scheme -Value 'URL:Chronver Background Watch'
    $null = New-ItemProperty -LiteralPath $scheme -Name 'URL Protocol' -Value '' -PropertyType String
    $null = New-Item -Path "$scheme\shell\open\command" -Force
    $null = Set-Item -LiteralPath "$scheme\shell\open\command" -Value $command
}
# The stub activator keeps protocol toasts in Notification Center without a COM server.
$appKey = 'HKCU:\Software\Classes\AppUserModelId\Chronver.BackgroundWatch'
$activator = '{647DD219-4794-44B2-833C-38EF945B0F3D}'
$existingApp = Get-Item -LiteralPath $appKey -ErrorAction SilentlyContinue
if ($existingApp -and $existingApp.GetValue('CustomActivator') -ne $activator) {
    throw 'The Chronver notification identity is already registered to a different app.'
}
if (-not $existingApp) {
    $null = New-Item -Path $appKey -Force
}
$null = New-ItemProperty -LiteralPath $appKey -Name DisplayName -Value 'Chronver' -PropertyType String -Force
$null = New-ItemProperty -LiteralPath $appKey -Name CustomActivator -Value $activator -PropertyType String -Force
$action = New-ScheduledTaskAction -Execute $exe -Argument $argument
$trigger = New-ScheduledTaskTrigger -Once -At ((Get-Date).AddHours(1)) -RepetitionInterval (New-TimeSpan -Hours 1)
$principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -StartWhenAvailable -MultipleInstances IgnoreNew `
    -ExecutionTimeLimit (New-TimeSpan -Minutes 10) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
$null = Register-ScheduledTask -TaskName $taskName -TaskPath '\' -Description $description `
    -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force
