$AppName = "iPixel 32x32 Controller"
$InstallDir = Join-Path $env:LOCALAPPDATA "Programs\iPixel32x32"

Get-Process -Name "ipixel_windows" -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 300

$DesktopShortcut = Join-Path ([Environment]::GetFolderPath("Desktop")) "$AppName.lnk"
if (Test-Path $DesktopShortcut) { Remove-Item $DesktopShortcut -Force }

$StartShortcut = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\$AppName.lnk"
if (Test-Path $StartShortcut) { Remove-Item $StartShortcut -Force }

$RegPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\iPixel32x32"
if (Test-Path $RegPath) { Remove-Item $RegPath -Recurse -Force }

if (Test-Path $InstallDir) {
    Start-Process -FilePath "cmd.exe" -ArgumentList "/c timeout /t 1 /nobreak >nul & rmdir /s /q `"$InstallDir`"" -WindowStyle Hidden
}

Add-Type -AssemblyName PresentationFramework
[System.Windows.MessageBox]::Show(
    "$AppName se ha desinstalado correctamente.",
    "Desinstalación",
    [System.Windows.MessageBoxButton]::OK,
    [System.Windows.MessageBoxImage]::Information
) | Out-Null
