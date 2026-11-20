$ErrorActionPreference = "Stop"

$AppName = "iPixel 32x32 Controller"
$InstallDir = Join-Path $env:LOCALAPPDATA "Programs\iPixel32x32"
$ExeTarget = Join-Path $InstallDir "ipixel_windows.exe"
$UninstallScript = Join-Path $InstallDir "uninstall.ps1"

New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null

$SourceDir = Split-Path -Parent $MyInvocation.MyCommand.Path
Copy-Item (Join-Path $SourceDir "ipixel_windows.exe") $ExeTarget -Force
Copy-Item (Join-Path $SourceDir "uninstall.ps1") $UninstallScript -Force

$WshShell = New-Object -ComObject WScript.Shell

$DesktopShortcut = Join-Path ([Environment]::GetFolderPath("Desktop")) "$AppName.lnk"
$Shortcut = $WshShell.CreateShortcut($DesktopShortcut)
$Shortcut.TargetPath = $ExeTarget
$Shortcut.WorkingDirectory = $InstallDir
$Shortcut.Description = "Controlador Bluetooth para panel LED 32x32"
$Shortcut.Save()

$StartMenuDir = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs"
$StartShortcutPath = Join-Path $StartMenuDir "$AppName.lnk"
$StartShortcut = $WshShell.CreateShortcut($StartShortcutPath)
$StartShortcut.TargetPath = $ExeTarget
$StartShortcut.WorkingDirectory = $InstallDir
$StartShortcut.Description = "Controlador Bluetooth para panel LED 32x32"
$StartShortcut.Save()

$RegPath = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\iPixel32x32"
New-Item -Path $RegPath -Force | Out-Null
Set-ItemProperty -Path $RegPath -Name "DisplayName" -Value $AppName
Set-ItemProperty -Path $RegPath -Name "DisplayVersion" -Value "0.1.0"
Set-ItemProperty -Path $RegPath -Name "Publisher" -Value "iPixel Windows"
Set-ItemProperty -Path $RegPath -Name "InstallLocation" -Value $InstallDir
Set-ItemProperty -Path $RegPath -Name "DisplayIcon" -Value $ExeTarget
Set-ItemProperty -Path $RegPath -Name "UninstallString" -Value "powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$UninstallScript`""
Set-ItemProperty -Path $RegPath -Name "NoModify" -Value 1 -Type DWord
Set-ItemProperty -Path $RegPath -Name "NoRepair" -Value 1 -Type DWord

Add-Type -AssemblyName PresentationFramework
$res = [System.Windows.MessageBox]::Show(
    "$AppName se ha instalado correctamente.`nSe han creado accesos directos en el Escritorio y en el Menú Inicio.`n`n¿Quieres abrir la aplicación ahora?",
    "Instalación completada",
    [System.Windows.MessageBoxButton]::YesNo,
    [System.Windows.MessageBoxImage]::Information
)

if ($res -eq [System.Windows.MessageBoxResult]::Yes) {
    Start-Process -FilePath $ExeTarget -WorkingDirectory $InstallDir
}
