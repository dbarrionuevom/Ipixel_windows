use crate::gif_proc::{
    encode_rgba_frames_as_gif, PreviewFrame, ProcessedGif, ResizeMode, MATRIX_HEIGHT, MATRIX_WIDTH,
};
use image::{DynamicImage, RgbaImage};
use std::path::Path;
use windows::core::HSTRING;
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFMediaType, IMFSample, IMFSourceReader, MFCreateAttributes,
    MFCreateMediaType, MFCreateSourceReaderFromURL, MFShutdown, MFStartup, MFVideoFormat_RGB32,
    MFMediaType_Video, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE,
    MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
    MF_SOURCE_READERF_ENDOFSTREAM, MF_VERSION, MFSTARTUP_FULL,
};
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

pub struct VideoConvertOptions {
    pub fps: u32,
    pub start_sec: f32,
    pub max_duration_sec: f32,
    pub resize_mode: ResizeMode,
}

/// Converts a local video file (`.mp4`, `.mov`, `.avi`, `.wmv`, etc.) or direct video URL
/// into a 32x32 animated GIF using Windows Media Foundation (zero external dependencies).
pub fn convert_video_to_gif(
    source: &str,
    opts: &VideoConvertOptions,
) -> Result<ProcessedGif, String> {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return Err("Indica un archivo de vídeo o una URL.".to_string());
    }

    let name = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed
            .split('?')
            .next()
            .and_then(|s| s.rsplit('/').next())
            .filter(|s| !s.is_empty())
            .unwrap_or("video.mp4")
            .to_string()
    } else {
        Path::new(trimmed)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("video.mp4")
            .to_string()
    };
    let gif_name = format!(
        "{}.gif",
        name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(&name)
    );

    unsafe {
        let co_hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        let com_inited = co_hr.is_ok();

        MFStartup(MF_VERSION, MFSTARTUP_FULL)
            .map_err(|e| format!("Error iniciando Media Foundation: {e}"))?;

        let result = convert_inner(trimmed, &gif_name, opts);

        let _ = MFShutdown();
        if com_inited {
            CoUninitialize();
        }
        result
    }
}

unsafe fn convert_inner(
    source: &str,
    gif_name: &str,
    opts: &VideoConvertOptions,
) -> Result<ProcessedGif, String> {
    let mut attrs: Option<IMFAttributes> = None;
    MFCreateAttributes(&mut attrs, 1).map_err(|e| format!("MFCreateAttributes: {e}"))?;
    let attrs = attrs.ok_or_else(|| "No se pudieron crear atributos MF.".to_string())?;
    attrs
        .SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)
        .map_err(|e| format!("SetUINT32 video processing: {e}"))?;

    let url_h = HSTRING::from(source);
    let reader: IMFSourceReader = MFCreateSourceReaderFromURL(&url_h, Some(&attrs))
        .map_err(|e| format!("No se pudo abrir el vídeo '{source}': {e}"))?;

    let stream_idx = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

    let out_type: IMFMediaType =
        MFCreateMediaType().map_err(|e| format!("MFCreateMediaType: {e}"))?;
    out_type
        .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
        .map_err(|e| format!("SetGUID major: {e}"))?;
    out_type
        .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)
        .map_err(|e| format!("SetGUID RGB32: {e}"))?;
    reader
        .SetCurrentMediaType(stream_idx, None, &out_type)
        .map_err(|e| format!("No se pudo configurar salida RGB32: {e}"))?;

    let actual_type = reader
        .GetCurrentMediaType(stream_idx)
        .map_err(|e| format!("GetCurrentMediaType: {e}"))?;
    let packed_size = actual_type
        .GetUINT64(&MF_MT_FRAME_SIZE)
        .map_err(|e| format!("GetUINT64 frame size: {e}"))?;
    let width = (packed_size >> 32) as u32;
    let height = (packed_size & 0xFFFF_FFFF) as u32;
    if width == 0 || height == 0 {
        return Err("Resolución de vídeo inválida (0x0).".to_string());
    }

    let stride_raw = actual_type
        .GetUINT32(&MF_MT_DEFAULT_STRIDE)
        .map(|v| v as i32)
        .unwrap_or((width * 4) as i32);
    let abs_stride = stride_raw.unsigned_abs() as usize;
    let bottom_up = stride_raw < 0;

    let fps = opts.fps.clamp(2, 20);
    let interval_100ns = (10_000_000i64) / i64::from(fps);
    let start_100ns = (opts.start_sec.max(0.0) * 10_000_000.0) as i64;
    let end_100ns = start_100ns + (opts.max_duration_sec.clamp(0.5, 15.0) * 10_000_000.0) as i64;
    let delay_ms = (1000 / fps).max(30);
    let delay_cs = ((delay_ms + 5) / 10).max(3) as u16;

    let mut next_target_100ns = start_100ns;
    let mut preview_frames = Vec::new();
    let mut encoded_frames = Vec::new();
    let max_frames = (fps as f32 * opts.max_duration_sec.clamp(0.5, 15.0)).ceil() as usize + 1;

    loop {
        if encoded_frames.len() >= max_frames {
            break;
        }
        let mut actual_stream = 0u32;
        let mut flags = 0u32;
        let mut timestamp = 0i64;
        let mut sample: Option<IMFSample> = None;

        reader
            .ReadSample(
                stream_idx,
                0,
                Some(&mut actual_stream),
                Some(&mut flags),
                Some(&mut timestamp),
                Some(&mut sample),
            )
            .map_err(|e| format!("Error leyendo frame de vídeo: {e}"))?;

        if (flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32) != 0 {
            break;
        }
        if timestamp > end_100ns {
            break;
        }
        let Some(sample) = sample else {
            continue;
        };
        if timestamp < next_target_100ns {
            continue;
        }
        next_target_100ns = timestamp + interval_100ns;

        let buffer = sample
            .ConvertToContiguousBuffer()
            .map_err(|e| format!("ConvertToContiguousBuffer: {e}"))?;
        let mut ptr: *mut u8 = std::ptr::null_mut();
        let mut max_len = 0u32;
        let mut cur_len = 0u32;
        buffer
            .Lock(&mut ptr, Some(&mut max_len), Some(&mut cur_len))
            .map_err(|e| format!("Buffer Lock: {e}"))?;

        let raw_slice = std::slice::from_raw_parts(ptr, cur_len as usize);
        let rgba_full = bgra_to_rgba(raw_slice, width, height, abs_stride, bottom_up);
        let _ = buffer.Unlock();

        let Some(rgba_img) = RgbaImage::from_raw(width, height, rgba_full) else {
            continue;
        };
        let resized = DynamicImage::ImageRgba8(rgba_img)
            .resize_to_fill(
                u32::from(MATRIX_WIDTH),
                u32::from(MATRIX_HEIGHT),
                opts.resize_mode.to_filter(),
            )
            .to_rgba8();
        let raw_32 = resized.into_raw();

        preview_frames.push(PreviewFrame {
            rgba: raw_32.clone(),
            delay_ms,
        });
        encoded_frames.push((raw_32, delay_cs));
    }

    if encoded_frames.is_empty() {
        return Err("No se pudieron extraer fotogramas del vídeo.".to_string());
    }

    let gif_bytes = encode_rgba_frames_as_gif(&encoded_frames)?;
    Ok(ProcessedGif {
        name: gif_name.to_string(),
        gif_bytes,
        preview_frames,
        original_size: (width, height),
        was_reencoded: true,
    })
}

fn bgra_to_rgba(
    src: &[u8],
    width: u32,
    height: u32,
    stride: usize,
    bottom_up: bool,
) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        let src_y = if bottom_up { h - 1 - y } else { y };
        let row_start = src_y * stride;
        if row_start + w * 4 > src.len() {
            break;
        }
        let src_row = &src[row_start..row_start + w * 4];
        let dst_row = &mut out[y * w * 4..(y + 1) * w * 4];
        for (s, d) in src_row.chunks_exact(4).zip(dst_row.chunks_exact_mut(4)) {
            d[0] = s[2]; // R
            d[1] = s[1]; // G
            d[2] = s[0]; // B
            d[3] = 255;  // A
        }
    }
    out
}
