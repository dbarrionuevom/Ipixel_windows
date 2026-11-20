use image::codecs::gif::GifDecoder;
use image::AnimationDecoder;
use serde::Deserialize;
use std::io::{Cursor, Read};
use std::time::Duration;

const GIPHY_WEB_KEY: &str = "Gc7131jiJuvI7IdN0HZ1D7nh0ow5BU6g";

#[derive(Clone)]
pub struct GiphyThumbFrame {
    pub rgba: Vec<u8>,
    pub delay_ms: u32,
}

#[derive(Clone)]
pub struct GiphyItem {
    pub id: String,
    pub title: String,
    pub gif_url: String,
    pub thumb_size: [usize; 2],
    pub thumb_frames: Vec<GiphyThumbFrame>,
}

pub struct GiphySearchPage {
    pub items: Vec<GiphyItem>,
    pub next_offset: usize,
    pub has_more: bool,
}

#[derive(Deserialize)]
struct GiphyApiResponse {
    data: Vec<GiphyApiGif>,
    pagination: Option<GiphyPagination>,
}

#[derive(Deserialize)]
struct GiphyApiGif {
    id: String,
    #[serde(default)]
    title: String,
}

#[derive(Deserialize)]
struct GiphyPagination {
    total_count: usize,
    count: usize,
    offset: usize,
}

fn url_encode_query(q: &str) -> String {
    let mut out = String::new();
    for b in q.trim().as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => out.push(*b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

fn fetch_thumb(agent: &ureq::Agent, id: &str) -> Option<([usize; 2], Vec<GiphyThumbFrame>)> {
    let url = format!("https://media.giphy.com/media/{id}/100w.gif");
    let resp = agent
        .get(&url)
        .set("User-Agent", "Mozilla/5.0")
        .call()
        .ok()?;
    let mut bytes = Vec::new();
    resp.into_reader()
        .take(2 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .ok()?;

    if let Ok(decoder) = GifDecoder::new(Cursor::new(&bytes)) {
        if let Ok(frames) = decoder.into_frames().collect_frames() {
            if !frames.is_empty() {
                let (w, h) = frames[0].buffer().dimensions();
                let thumb_frames: Vec<GiphyThumbFrame> = frames
                    .into_iter()
                    .take(36)
                    .map(|f| {
                        let (num, den) = f.delay().numer_denom_ms();
                        let delay_ms = if den == 0 { 80 } else { (num / den).max(30) };
                        GiphyThumbFrame {
                            rgba: f.into_buffer().into_raw(),
                            delay_ms,
                        }
                    })
                    .collect();
                return Some(([w as usize, h as usize], thumb_frames));
            }
        }
    }

    let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    Some((
        [w as usize, h as usize],
        vec![GiphyThumbFrame {
            rgba: img.into_raw(),
            delay_ms: 100,
        }],
    ))
}

/// Searches Giphy with pagination (`offset`, `limit`) and downloads animated thumbnails concurrently.
pub fn search_giphy(
    query: &str,
    offset: usize,
    limit: usize,
) -> Result<GiphySearchPage, String> {
    let encoded = url_encode_query(query);
    if encoded.is_empty() {
        return Err("Escribe algo para buscar en Giphy.".to_string());
    }

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(12))
        .build();

    let api_url = format!(
        "https://api.giphy.com/v1/gifs/search?api_key={GIPHY_WEB_KEY}&q={encoded}&limit={limit}&offset={offset}&rating=g"
    );

    let resp = agent
        .get(&api_url)
        .set("User-Agent", "Mozilla/5.0")
        .call()
        .map_err(|e| format!("Error consultando Giphy: {e}"))?;

    let mut body = String::new();
    resp.into_reader()
        .take(2 * 1024 * 1024)
        .read_to_string(&mut body)
        .map_err(|e| format!("Error leyendo respuesta de Giphy: {e}"))?;

    let parsed: GiphyApiResponse =
        serde_json::from_str(&body).map_err(|e| format!("JSON de Giphy inválido: {e}"))?;

    if parsed.data.is_empty() {
        return Ok(GiphySearchPage {
            items: Vec::new(),
            next_offset: offset,
            has_more: false,
        });
    }

    let (next_offset, has_more) = if let Some(p) = parsed.pagination {
        let next = p.offset + p.count;
        (next, next < p.total_count && p.count > 0)
    } else {
        let next = offset + parsed.data.len();
        (next, parsed.data.len() >= limit)
    };

    let items: Vec<Option<GiphyItem>> = std::thread::scope(|s| {
        let handles: Vec<_> = parsed
            .data
            .into_iter()
            .map(|entry| {
                let agent_ref = &agent;
                s.spawn(move || {
                    let (thumb_size, thumb_frames) = fetch_thumb(agent_ref, &entry.id)?;
                    Some(GiphyItem {
                        gif_url: format!("https://media.giphy.com/media/{}/200w.gif", entry.id),
                        id: entry.id,
                        title: if entry.title.is_empty() {
                            "Giphy GIF".to_string()
                        } else {
                            entry.title
                        },
                        thumb_size,
                        thumb_frames,
                    })
                })
            })
            .collect();

        handles
            .into_iter()
            .map(|h| h.join().ok().flatten())
            .collect()
    });

    Ok(GiphySearchPage {
        items: items.into_iter().flatten().collect(),
        next_offset,
        has_more,
    })
}
