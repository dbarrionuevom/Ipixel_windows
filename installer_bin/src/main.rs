#![windows_subsystem = "windows"]

use std::env;
use std::fs;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use windows::core::HSTRING;
use windows::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK, MB_YESNO,
};

const APP_BYTES: &[u8] = include_bytes!("../../target/release/ipixel_windows.exe");
const UNINSTALL_PS1: &str = include_str!("../../installer/uninstall.ps1");
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
const CREATE_NO_WINDOW: u32 = 0x08000000;

fn msg_yes_no(title: &str, text: &str) -> bool {
    let t = HSTRING::from(title);
    let b = HSTRING::from(text);
    unsafe { MessageBoxW(None, &b, &t, MB_YESNO | MB_ICONQUESTION) == IDYES }
}

fn msg_info(title: &str, text: &str) {
    let t = HSTRING::from(title);
    let b = HSTRING::from(text);
    unsafe {
        MessageBoxW(None, &b, &t, MB_OK | MB_ICONINFORMATION);
    }
}

fn msg_error(title: &str, text: &str) {
    let t = HSTRING::from(title);
    let b = HSTRING::from(text);
    unsafe {
        MessageBoxW(None, &b, &t, MB_OK | MB_ICONERROR);
    }
}

fn run_install() -> Result<(PathBuf, PathBuf), String> {
    let local_app_data =
        env::var("LOCALAPPDATA").map_err(|_| "No se encontró LOCALAPPDATA.".to_string())?;
    let install_dir = PathBuf::from(local_app_data)
        .join("Programs")
        .join("iPixel32x32");
    fs::create_dir_all(&install_dir)
        .map_err(|e| format!("No se pudo crear la carpeta de instalación: {e}"))?;

    let exe_path = install_dir.join("ipixel_windows.exe");
    let uninstall_path = install_dir.join("uninstall.ps1");

    fs::write(&exe_path, APP_BYTES)
        .map_err(|e| format!("Error copiando ipixel_windows.exe (¿está abierto?): {e}"))?;
    fs::write(&uninstall_path, UNINSTALL_PS1)
        .map_err(|e| format!("Error escribiendo uninstall.ps1: {e}"))?;

    let ps_script = format!(
        r#"$ErrorActionPreference = 'Stop'
$AppName = 'iPixel 32x32 Controller'
$InstallDir = '{dir}'
$ExeTarget = '{exe}'
$UninstallScript = '{uninst}'
$WshShell = New-Object -ComObject WScript.Shell
$DesktopShortcut = Join-Path ([Environment]::GetFolderPath('Desktop')) "$AppName.lnk"
$S1 = $WshShell.CreateShortcut($DesktopShortcut)
$S1.TargetPath = $ExeTarget
$S1.WorkingDirectory = $InstallDir
$S1.Description = 'Controlador Bluetooth para panel LED 32x32'
$S1.Save()
$StartMenuDir = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
$S2 = $WshShell.CreateShortcut((Join-Path $StartMenuDir "$AppName.lnk"))
$S2.TargetPath = $ExeTarget
$S2.WorkingDirectory = $InstallDir
$S2.Description = 'Controlador Bluetooth para panel LED 32x32'
$S2.Save()
$RegPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\iPixel32x32'
New-Item -Path $RegPath -Force | Out-Null
Set-ItemProperty -Path $RegPath -Name 'DisplayName' -Value $AppName
Set-ItemProperty -Path $RegPath -Name 'DisplayVersion' -Value '{ver}'
Set-ItemProperty -Path $RegPath -Name 'Publisher' -Value 'iPixel Windows'
Set-ItemProperty -Path $RegPath -Name 'InstallLocation' -Value $InstallDir
Set-ItemProperty -Path $RegPath -Name 'DisplayIcon' -Value $ExeTarget
Set-ItemProperty -Path $RegPath -Name 'UninstallString' -Value "powershell.exe -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$UninstallScript`""
Set-ItemProperty -Path $RegPath -Name 'NoModify' -Value 1 -Type DWord
Set-ItemProperty -Path $RegPath -Name 'NoRepair' -Value 1 -Type DWord
"#,
        dir = install_dir.display().to_string().replace('\'', "''"),
        exe = exe_path.display().to_string().replace('\'', "''"),
        uninst = uninstall_path.display().to_string().replace('\'', "''"),
        ver = APP_VERSION,
    );

    let status = Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &ps_script])
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("Error ejecutando PowerShell para accesos directos: {e}"))?;

    if !status.success() {
        return Err("Falló la creación de accesos directos o registro de desinstalación.".to_string());
    }

    Ok((install_dir, exe_path))
}

fn main() {
    let title = format!("iPixel 32x32 Controller v{APP_VERSION} - Instalador");
    let prompt = format!(
        "¿Deseas instalar iPixel 32x32 Controller v{APP_VERSION} en este equipo?\n\nSe instalará en tu carpeta de usuario (sin necesidad de permisos de administrador) con acceso directo en el Escritorio y Menú Inicio."
    );
    if !msg_yes_no(&title, &prompt) {
        return;
    }

    match run_install() {
        Ok((install_dir, exe_path)) => {
            if msg_yes_no(
                "Instalación completada",
                &format!("iPixel 32x32 Controller v{APP_VERSION} se ha instalado correctamente.\n\n¿Quieres abrir la aplicación ahora?"),
            ) {
                if let Err(e) = Command::new(&exe_path).current_dir(&install_dir).spawn() {
                    msg_error("Error al abrir", &format!("No se pudo abrir la aplicación: {e}"));
                }
            } else {
                msg_info(
                    "Listo",
                    "Puedes abrirla cuando quieras desde el acceso directo del Escritorio o Menú Inicio.",
                );
            }
        }
        Err(e) => {
            msg_error("Error de instalación", &e);
        }
    }
}
