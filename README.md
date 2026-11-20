# iPixel 32x32 Controller (Windows)

Aplicación nativa para Windows escrita en Rust para controlar paneles LED matriciales **iPixel 32×32** por Bluetooth Low Energy (BLE).

## Funcionalidades

- **Conexión BLE directa en Windows (`btleplug`):** Escaneo, conexión, ajuste de MTU, encendido/apagado, brillo automático al 100% y borrado de memoria.
- **GIFs y fotos 32×32:** Carga cualquier `.gif` animado o imagen (`.png`, `.jpg`, `.bmp`, `.webp`), recorta manteniendo la proporción (`resize_to_fill`) a **32×32** y codifica en GIF89a con tabla de colores global.
- **Carga desde URL:** Descarga directa de GIFs/imágenes desde cualquier enlace `http://` o `https://`.
- **Buscador Giphy integrado:** Búsqueda con miniaturas animadas y scroll infinito sin abrir el navegador.
- **Conversor Vídeo MP4 -> GIF:** Convierte vídeos locales (`.mp4`, `.mov`, `.avi`, `.wmv`, `.mkv`, `.webm`) o URLs directas usando Windows Media Foundation nativo (sin `ffmpeg` externo).
- **Listas de reproducción (Canales 1–100):** Cola ordenada para subir múltiples animaciones a la memoria del panel.
- **Instalador Windows:** Incluye el proyecto del instalador autocontenido en `installer_bin/`.

## Compilación

```powershell
cargo test
cargo build --release
```

El ejecutable se genera en `target\release\ipixel_windows.exe`.

Para generar el instalador `iPixel_32x32_Setup.exe`:

```powershell
cargo build --release --manifest-path installer_bin/Cargo.toml
```
