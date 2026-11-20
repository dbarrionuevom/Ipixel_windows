use gif::{Encoder, Frame, Repeat};
use image::codecs::gif::GifDecoder;
use image::imageops::FilterType;
use image::{AnimationDecoder, DynamicImage, GenericImageView, ImageReader};
use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;
use std::time::Duration;

pub const MATRIX_WIDTH: u16 = 32;
pub const MATRIX_HEIGHT: u16 = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeMode {
    /// Crisp edges, ideal for pixel art and 32x32 sprites.
    PixelArtNearest,
    /// Smooth downscaling, ideal for photos and video GIFs.
    SmoothLanczos,
}

impl ResizeMode {
    pub fn label(self) -> &'static str {
        match self {
            ResizeMode::PixelArtNearest => "Pixel Art (Nearest)",
            ResizeMode::SmoothLanczos => "Suave (Lanczos3)",
        }
    }

    pub fn to_filter(self) -> FilterType {
        match self {
            ResizeMode::PixelArtNearest => FilterType::Nearest,
            ResizeMode::SmoothLanczos => FilterType::Lanczos3,
        }
    }
}

#[derive(Clone)]
pub struct PreviewFrame {
    pub rgba: Vec<u8>,
    pub delay_ms: u32,
}

#[derive(Clone)]
pub struct ProcessedGif {
    pub name: String,
    pub gif_bytes: Vec<u8>,
    pub preview_frames: Vec<PreviewFrame>,
    pub original_size: (u32, u32),
    pub was_reencoded: bool,
}

/// Downloads an image/GIF from an HTTP/HTTPS URL (up to 25 MB) and returns `(filename, bytes, is_gif_hint)`.
pub fn download_bytes_from_url(url: &str) -> Result<(String, Vec<u8>, bool), String> {
    let trimmed = url.trim();
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Err("La URL debe empezar por http:// o https://".to_string());
    }

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(15))
        .build();

    let resp = agent
        .get(trimmed)
        .set("User-Agent", "iPixel-Windows/0.1")
        .call()
        .map_err(|e| format!("Error descargando URL: {e}"))?;

    let content_type = resp.content_type().to_string();
    let is_gif_hint = content_type.eq_ignore_ascii_case("image/gif")
        || trimmed
            .split('?')
            .next()
            .unwrap_or(trimmed)
            .to_ascii_lowercase()
            .ends_with(".gif");

    let name = trimmed
        .split('?')
        .next()
        .and_then(|s| s.rsplit('/').next())
        .filter(|s| !s.is_empty())
        .unwrap_or("web_animacion.gif")
        .to_string();

    const MAX_BYTES: usize = 25 * 1024 * 1024;
    let mut bytes = Vec::new();
    resp.into_reader()
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Error leyendo respuesta HTTP: {e}"))?;

    if bytes.len() > MAX_BYTES {
        return Err("El archivo de la URL supera el límite de 25 MB.".to_string());
    }

    Ok((name, bytes, is_gif_hint))
}

/// Loads a GIF or static image from disk and prepares it for 32x32 LED display.
pub fn load_and_prepare_gif(
    path: &Path,
    mode: ResizeMode,
    force_reencode: bool,
) -> Result<ProcessedGif, String> {
    let raw_bytes = fs::read(path).map_err(|e| format!("No se pudo leer el archivo: {e}"))?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("animacion.gif")
        .to_string();

    let is_gif_hint = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("gif"))
        .unwrap_or(false);

    prepare_gif_from_bytes(name, &raw_bytes, is_gif_hint, mode, force_reencode)
}

/// Decodes raw image/GIF bytes, resizes to 32x32 if needed (or when `force_reencode` is true),
/// and produces both the raw GIF89a bytes for the LED panel and RGBA frames for UI preview.
pub fn prepare_gif_from_bytes(
    name: String,
    raw_bytes: &[u8],
    is_gif_hint: bool,
    mode: ResizeMode,
    force_reencode: bool,
) -> Result<ProcessedGif, String> {
    let is_gif = is_gif_hint || raw_bytes.starts_with(b"GIF8");

    if is_gif {
        let decoder = GifDecoder::new(Cursor::new(raw_bytes))
            .map_err(|e| format!("GIF inválido: {e}"))?;
        let frames = decoder
            .into_frames()
            .collect_frames()
            .map_err(|e| format!("Error decodificando frames del GIF: {e}"))?;

        if frames.is_empty() {
            return Err("El GIF no contiene fotogramas.".to_string());
        }

        let (orig_w, orig_h) = frames[0].buffer().dimensions();
        let already_32 = orig_w == u32::from(MATRIX_WIDTH) && orig_h == u32::from(MATRIX_HEIGHT);

        let mut preview_frames = Vec::with_capacity(frames.len());
        let mut resized_rgba_frames: Vec<(Vec<u8>, u16)> = Vec::with_capacity(frames.len());

        for frame in frames {
            let (num, den) = frame.delay().numer_denom_ms();
            let delay_ms = if den == 0 { 100 } else { (num / den).max(20) };
            let delay_cs = ((delay_ms + 5) / 10).max(2) as u16;

            let img = DynamicImage::ImageRgba8(frame.into_buffer());
            let resized = if already_32 {
                img.to_rgba8()
            } else {
                img.resize_to_fill(
                    u32::from(MATRIX_WIDTH),
                    u32::from(MATRIX_HEIGHT),
                    mode.to_filter(),
                )
                .to_rgba8()
            };

            let raw_rgba = resized.into_raw();
            preview_frames.push(PreviewFrame {
                rgba: raw_rgba.clone(),
                delay_ms,
            });
            resized_rgba_frames.push((raw_rgba, delay_cs));
        }

        if already_32 && !force_reencode {
            return Ok(ProcessedGif {
                name,
                gif_bytes: raw_bytes.to_vec(),
                preview_frames,
                original_size: (orig_w, orig_h),
                was_reencoded: false,
            });
        }

        let encoded = encode_rgba_frames_as_gif(&resized_rgba_frames)?;
        return Ok(ProcessedGif {
            name,
            gif_bytes: encoded,
            preview_frames,
            original_size: (orig_w, orig_h),
            was_reencoded: true,
        });
    }

    // Static image fallback (PNG/JPG/BMP/WebP -> 1-frame 32x32 GIF)
    let img = ImageReader::new(Cursor::new(raw_bytes))
        .with_guessed_format()
        .map_err(|e| format!("Formato de imagen no reconocido: {e}"))?
        .decode()
        .map_err(|e| format!("Error decodificando imagen: {e}"))?;

    let (orig_w, orig_h) = img.dimensions();
    let resized = img
        .resize_to_fill(
            u32::from(MATRIX_WIDTH),
            u32::from(MATRIX_HEIGHT),
            mode.to_filter(),
        )
        .to_rgba8();

    let raw_rgba = resized.into_raw();
    let encoded = encode_rgba_frames_as_gif(&[(raw_rgba.clone(), 100)])?;

    Ok(ProcessedGif {
        name,
        gif_bytes: encoded,
        preview_frames: vec![PreviewFrame {
            rgba: raw_rgba,
            delay_ms: 1000,
        }],
        original_size: (orig_w, orig_h),
        was_reencoded: true,
    })
}

pub fn encode_rgba_frames_as_gif(frames: &[(Vec<u8>, u16)]) -> Result<Vec<u8>, String> {
    let total_h = MATRIX_HEIGHT
        .checked_mul(frames.len() as u16)
        .ok_or_else(|| "Demasiados fotogramas para codificar el GIF.".to_string())?;

    let mut combined = Vec::with_capacity(frames.len() * usize::from(MATRIX_WIDTH) * usize::from(MATRIX_HEIGHT) * 4);
    for (rgba, _) in frames {
        let mut buf = rgba.clone();
        // Flatten alpha onto black background so transparent pixels render cleanly on LEDs.
        for px in buf.chunks_exact_mut(4) {
            let a = u16::from(px[3]);
            if a < 255 {
                px[0] = ((u16::from(px[0]) * a) / 255) as u8;
                px[1] = ((u16::from(px[1]) * a) / 255) as u8;
                px[2] = ((u16::from(px[2]) * a) / 255) as u8;
                px[3] = 255;
            }
        }
        combined.extend_from_slice(&buf);
    }

    let quantized = Frame::from_rgba_speed(MATRIX_WIDTH, total_h, &mut combined, 10);
    let global_palette = quantized.palette.as_deref().unwrap_or(&[]);
    let frame_pixels = usize::from(MATRIX_WIDTH) * usize::from(MATRIX_HEIGHT);

    let mut out = Vec::new();
    {
        let mut encoder = Encoder::new(&mut out, MATRIX_WIDTH, MATRIX_HEIGHT, global_palette)
            .map_err(|e| format!("Error inicializando codificador GIF: {e}"))?;
        encoder
            .set_repeat(Repeat::Infinite)
            .map_err(|e| format!("Error configurando repetición GIF: {e}"))?;

        for (idx, (_, delay_cs)) in frames.iter().enumerate() {
            let start = idx * frame_pixels;
            let end = start + frame_pixels;
            let mut frame = Frame::from_indexed_pixels(
                MATRIX_WIDTH,
                MATRIX_HEIGHT,
                &quantized.buffer[start..end],
                None,
            );
            frame.delay = *delay_cs;
            encoder
                .write_frame(&frame)
                .map_err(|e| format!("Error escribiendo frame GIF: {e}"))?;
        }
    }
    Ok(out)
}
