$ErrorActionPreference = 'Stop'
$appId = 'Chronver.BackgroundWatch'
if (-not (Test-Path -LiteralPath "HKCU:\Software\Classes\AppUserModelId\$appId")) {
    throw 'Turn Hourly off and back on to restore the Chronver notification identity.'
}
[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType=WindowsRuntime] > $null
[Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType=WindowsRuntime] > $null
$notifier = [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier($appId)
$title = [Security.SecurityElement]::Escape($env:CHRONVER_WATCH_TITLE)
$detail = [Security.SecurityElement]::Escape($env:CHRONVER_WATCH_DETAIL)
$link = [Security.SecurityElement]::Escape($env:CHRONVER_WATCH_LINK)
$xml = [Windows.Data.Xml.Dom.XmlDocument]::new()
$xml.LoadXml("<toast activationType='protocol' launch='$link'><visual><binding template='ToastGeneric'><text>$title</text><text>$detail</text></binding></visual></toast>")
$toast = [Windows.UI.Notifications.ToastNotification, Windows.UI.Notifications, ContentType=WindowsRuntime]::new($xml)
# The setting may not exist until the first notification is submitted.
$notifier.Show($toast)
if ($null -ne $notifier.Setting -and $notifier.Setting.ToString() -ne 'Enabled') {
    throw 'Windows notifications are disabled for Chronver. Updates are still saved in Background Watch.'
}
