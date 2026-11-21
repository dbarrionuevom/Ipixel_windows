use crate::gif_proc::{prepare_gif_from_bytes, ProcessedGif, ResizeMode};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
struct SavedPlaylist {
    clear_before_upload: bool,
    items: Vec<SavedPlaylistItem>,
}

#[derive(Serialize, Deserialize)]
struct SavedPlaylistItem {
    name: String,
    cache_file: String,
    original_size: (u32, u32),
    was_reencoded: bool,
}

fn app_data_dir() -> Option<PathBuf> {
    env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("iPixel32x32"))
}

pub fn save_playlist(items: &[ProcessedGif], clear_before_upload: bool) -> Result<(), String> {
    let base = app_data_dir().ok_or_else(|| "No se encontró LOCALAPPDATA.".to_string())?;
    let cache_dir = base.join("playlist_cache");
    fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("No se pudo crear carpeta de caché: {e}"))?;

    // Clear old cached GIF files so deleted items don't accumulate on disk.
    if let Ok(entries) = fs::read_dir(&cache_dir) {
        for entry in entries.flatten() {
            let _ = fs::remove_file(entry.path());
        }
    }

    let mut saved_items = Vec::with_capacity(items.len());
    for (idx, gif) in items.iter().enumerate() {
        let cache_file = format!("item_{:03}.gif", idx + 1);
        let cache_path = cache_dir.join(&cache_file);
        fs::write(&cache_path, &gif.gif_bytes)
            .map_err(|e| format!("Error guardando {} en caché: {e}", gif.name))?;

        saved_items.push(SavedPlaylistItem {
            name: gif.name.clone(),
            cache_file,
            original_size: gif.original_size,
            was_reencoded: gif.was_reencoded,
        });
    }

    let manifest = SavedPlaylist {
        clear_before_upload,
        items: saved_items,
    };
    let json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("Error serializando playlist: {e}"))?;
    fs::write(base.join("playlist.json"), json)
        .map_err(|e| format!("Error escribiendo playlist.json: {e}"))?;
    Ok(())
}

pub fn load_playlist() -> (bool, Vec<ProcessedGif>) {
    let Some(base) = app_data_dir() else {
        return (true, Vec::new());
    };
    let manifest_path = base.join("playlist.json");
    let Ok(json) = fs::read_to_string(manifest_path) else {
        return (true, Vec::new());
    };
    let Ok(saved) = serde_json::from_str::<SavedPlaylist>(&json) else {
        return (true, Vec::new());
    };

    let cache_dir = base.join("playlist_cache");
    let mut loaded = Vec::with_capacity(saved.items.len());
    for item in saved.items {
        let path = cache_dir.join(&item.cache_file);
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        if let Ok(mut gif) =
            prepare_gif_from_bytes(item.name, &bytes, true, ResizeMode::PixelArtNearest, false)
        {
            gif.original_size = item.original_size;
            gif.was_reencoded = item.was_reencoded;
            loaded.push(gif);
        }
    }

    (saved.clear_before_upload, loaded)
}
