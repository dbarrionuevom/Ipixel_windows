use crate::ble::{BleCommand, BleEvent, BleHandle, DiscoveredDevice};
use crate::gif_proc::{
    download_bytes_from_url, load_and_prepare_gif, prepare_gif_from_bytes, ProcessedGif,
    ResizeMode, MATRIX_HEIGHT, MATRIX_WIDTH,
};
use crate::giphy::{search_giphy, GiphyItem, GiphySearchPage};
use crate::protocol::{DIRECT_PLAY_SLOT, MAX_PLAYLIST_SLOTS};
use crate::storage::{load_playlist, save_playlist};
use crate::video_proc::{convert_video_to_gif, VideoConvertOptions};
use eframe::egui::{
    self, Align, Color32, ColorImage, CornerRadius, Frame, Layout, Margin, Pos2, Rect, RichText,
    Stroke, TextureHandle, TextureOptions, Vec2,
};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::Instant;

/// Hallmark-inspired design tokens: dark LED-studio palette with one warm signal accent.
mod theme {
    use eframe::egui::Color32;

    pub const BG_CANVAS: Color32 = Color32::from_rgb(15, 17, 21);
    pub const BG_SURFACE: Color32 = Color32::from_rgb(23, 26, 33);
    pub const BG_ELEVATED: Color32 = Color32::from_rgb(31, 35, 45);
    pub const BG_HOVER: Color32 = Color32::from_rgb(41, 46, 59);
    pub const BORDER_SUBTLE: Color32 = Color32::from_rgb(44, 50, 64);

    pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(243, 244, 246);
    pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(156, 163, 175);
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(107, 114, 128);

    pub const ACCENT: Color32 = Color32::from_rgb(255, 90, 54);
    pub const ACCENT_TEXT: Color32 = Color32::from_rgb(255, 255, 255);
    pub const SUCCESS: Color32 = Color32::from_rgb(52, 211, 153);
    pub const DANGER: Color32 = Color32::from_rgb(248, 113, 113);
    pub const MATRIX_BEZEL: Color32 = Color32::from_rgb(10, 11, 14);
}

fn apply_hallmark_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(12.0, 6.0);
    style.spacing.window_margin = Margin::same(16);
    style.spacing.interact_size.y = 28.0;

    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = theme::BG_CANVAS;
    visuals.window_fill = theme::BG_SURFACE;
    visuals.extreme_bg_color = theme::MATRIX_BEZEL;
    visuals.faint_bg_color = theme::BG_ELEVATED;

    visuals.widgets.noninteractive.bg_fill = theme::BG_SURFACE;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, theme::BORDER_SUBTLE);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, theme::TEXT_SECONDARY);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(8);

    visuals.widgets.inactive.bg_fill = theme::BG_ELEVATED;
    visuals.widgets.inactive.weak_bg_fill = theme::BG_ELEVATED;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, theme::BORDER_SUBTLE);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, theme::TEXT_PRIMARY);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(7);

    visuals.widgets.hovered.bg_fill = theme::BG_HOVER;
    visuals.widgets.hovered.weak_bg_fill = theme::BG_HOVER;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, theme::TEXT_MUTED);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, theme::TEXT_PRIMARY);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(7);

    visuals.widgets.active.bg_fill = theme::ACCENT;
    visuals.widgets.active.weak_bg_fill = theme::ACCENT;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0, theme::ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0, theme::ACCENT_TEXT);
    visuals.widgets.active.corner_radius = CornerRadius::same(7);

    visuals.selection.bg_fill = theme::ACCENT.linear_multiply(0.35);
    visuals.selection.stroke = Stroke::new(1.0, theme::ACCENT);

    style.visuals = visuals;
    ctx.set_style(style);
}

fn primary_button(label: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(label).color(theme::ACCENT_TEXT).strong())
        .fill(theme::ACCENT)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(7))
}

fn surface_frame() -> Frame {
    Frame::NONE
        .fill(theme::BG_SURFACE)
        .stroke(Stroke::new(1.0, theme::BORDER_SUBTLE))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::same(16))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UrlDownloadAction {
    PreviewOnly,
    PreviewAndSaveToDevice,
    AddToPlaylist,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeftTab {
    Preview,
    Giphy,
    Video,
}

struct GiphyCard {
    item: GiphyItem,
    textures: Vec<TextureHandle>,
    delays_ms: Vec<u32>,
    frame_idx: usize,
    last_switch: Instant,
}

#[derive(Clone)]
enum GifSource {
    File(PathBuf),
    Url {
        name: String,
        raw_bytes: Vec<u8>,
        is_gif: bool,
    },
    Prepared(ProcessedGif),
}

struct ActivePreview {
    gif: ProcessedGif,
    source: GifSource,
    textures: Vec<TextureHandle>,
    frame_idx: usize,
    last_frame_switch: Instant,
}

struct PlaylistEntry {
    gif: ProcessedGif,
}

pub struct IPixelApp {
    ble: BleHandle,
    event_rx: Receiver<BleEvent>,

    // BLE state
    scanning: bool,
    devices: Vec<DiscoveredDevice>,
    selected_device_id: Option<String>,
    connected_name: Option<String>,
    mtu_chunk: usize,
    brightness: u8,
    busy: bool,
    progress: Option<(f32, String)>,

    // Single GIF editor/preview
    left_tab: LeftTab,
    resize_mode: ResizeMode,
    force_reencode: bool,
    show_led_grid: bool,
    target_slot: u8,
    preview: Option<ActivePreview>,
    url_input: String,
    downloading_url: bool,
    url_rx: Option<Receiver<(UrlDownloadAction, Result<(String, Vec<u8>, bool), String>)>>,

    // Integrated Giphy browser
    giphy_query: String,
    giphy_active_query: String,
    giphy_offset: usize,
    giphy_has_more: bool,
    giphy_searching: bool,
    giphy_rx: Option<Receiver<(bool, Result<GiphySearchPage, String>)>>,
    giphy_results: Vec<GiphyCard>,

    // Video to GIF converter
    video_source: String,
    video_fps: u32,
    video_start_sec: f32,
    video_duration_sec: f32,
    video_resize_mode: ResizeMode,
    video_converting: bool,
    video_rx: Option<Receiver<Result<ProcessedGif, String>>>,

    // Playlist
    playlist: Vec<PlaylistEntry>,
    clear_before_upload: bool,

    // Logs
    show_logs: bool,
    logs: Vec<String>,
    status_banner: Option<(bool, String)>,
}

impl IPixelApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_hallmark_theme(&cc.egui_ctx);
        let (tx, rx) = mpsc::channel();
        let ble = BleHandle::spawn(tx, cc.egui_ctx.clone());
        let (clear_before_upload, saved_gifs) = load_playlist();
        let playlist: Vec<PlaylistEntry> = saved_gifs
            .into_iter()
            .map(|gif| PlaylistEntry { gif })
            .collect();
        let mut logs = vec!["Listo. Pulsa 'Escanear' para buscar tu panel 32x32.".to_string()];
        if !playlist.is_empty() {
            logs.push(format!(
                "Lista de reproducción restaurada ({} elementos).",
                playlist.len()
            ));
        }

        Self {
            ble,
            event_rx: rx,
            scanning: false,
            devices: Vec::new(),
            selected_device_id: None,
            connected_name: None,
            mtu_chunk: 180,
            brightness: 100,
            busy: false,
            progress: None,
            left_tab: LeftTab::Preview,
            resize_mode: ResizeMode::PixelArtNearest,
            force_reencode: false,
            show_led_grid: true,
            target_slot: 1,
            preview: None,
            url_input: String::new(),
            downloading_url: false,
            url_rx: None,
            giphy_query: "pixel art".to_string(),
            giphy_active_query: String::new(),
            giphy_offset: 0,
            giphy_has_more: false,
            giphy_searching: false,
            giphy_rx: None,
            giphy_results: Vec::new(),
            video_source: String::new(),
            video_fps: 10,
            video_start_sec: 0.0,
            video_duration_sec: 5.0,
            video_resize_mode: ResizeMode::SmoothLanczos,
            video_converting: false,
            video_rx: None,
            playlist,
            clear_before_upload,
            show_logs: false,
            logs,
            status_banner: None,
        }
    }

    fn persist_playlist(&mut self) {
        let gifs: Vec<ProcessedGif> = self.playlist.iter().map(|e| e.gif.clone()).collect();
        if let Err(e) = save_playlist(&gifs, self.clear_before_upload) {
            self.logs.push(format!("Aviso guardando lista: {e}"));
        }
    }

    fn poll_ble_events(&mut self) {
        while let Ok(ev) = self.event_rx.try_recv() {
            match ev {
                BleEvent::Log(msg) => {
                    self.logs.push(msg);
                    if self.logs.len() > 200 {
                        self.logs.remove(0);
                    }
                }
                BleEvent::ScanFinished(devs) => {
                    self.scanning = false;
                    if let Some(first) = devs.first() {
                        if self.selected_device_id.is_none() {
                            self.selected_device_id = Some(first.id.clone());
                        }
                    }
                    self.devices = devs;
                }
                BleEvent::Connected(name) => {
                    self.connected_name = Some(name.clone());
                    self.busy = false;
                    self.status_banner = Some((true, format!("Conectado a {name}")));
                }
                BleEvent::Disconnected => {
                    self.connected_name = None;
                    self.busy = false;
                    self.progress = None;
                }
                BleEvent::Progress(frac, text) => {
                    self.progress = Some((frac.clamp(0.0, 1.0), text));
                }
                BleEvent::TransferSuccess(msg) => {
                    self.busy = false;
                    self.progress = None;
                    self.status_banner = Some((true, msg.clone()));
                    self.logs.push(msg);
                }
                BleEvent::Error(err) => {
                    self.busy = false;
                    self.progress = None;
                    self.status_banner = Some((false, err.clone()));
                    self.logs.push(format!("ERROR: {err}"));
                }
            }
        }
    }

    fn poll_url_download(&mut self, ctx: &egui::Context) {
        let mut finished = None;
        if let Some(rx) = &self.url_rx {
            if let Ok(res) = rx.try_recv() {
                finished = Some(res);
            }
        }
        if let Some((action, res)) = finished {
            self.downloading_url = false;
            self.url_rx = None;
            match res {
                Ok((name, raw_bytes, is_gif)) => {
                    self.logs
                        .push(format!("GIF descargado: {name} ({} bytes)", raw_bytes.len()));
                    match action {
                        UrlDownloadAction::AddToPlaylist => {
                            if self.playlist.len() >= MAX_PLAYLIST_SLOTS {
                                self.status_banner = Some((
                                    false,
                                    format!("Máximo de {MAX_PLAYLIST_SLOTS} elementos alcanzado."),
                                ));
                            } else {
                                match prepare_gif_from_bytes(
                                    name,
                                    &raw_bytes,
                                    is_gif,
                                    self.resize_mode,
                                    self.force_reencode,
                                ) {
                                    Ok(gif) => {
                                        self.status_banner = Some((
                                            true,
                                            format!("Añadido a la lista: {}", gif.name),
                                        ));
                                        self.playlist.push(PlaylistEntry { gif });
                                        self.persist_playlist();
                                    }
                                    Err(e) => {
                                        self.status_banner = Some((false, e));
                                    }
                                }
                            }
                        }
                        UrlDownloadAction::PreviewOnly | UrlDownloadAction::PreviewAndSaveToDevice => {
                            self.left_tab = LeftTab::Preview;
                            self.load_preview_source(
                                ctx,
                                GifSource::Url {
                                    name,
                                    raw_bytes,
                                    is_gif,
                                },
                            );
                            if action == UrlDownloadAction::PreviewAndSaveToDevice {
                                if self.connected_name.is_some() && !self.busy {
                                    if let Some(prev) = &self.preview {
                                        self.busy = true;
                                        self.ble.send(BleCommand::SendGif {
                                            gif_bytes: prev.gif.gif_bytes.clone(),
                                            slot: self.target_slot,
                                            precheck: true,
                                        });
                                    }
                                } else if self.connected_name.is_none() {
                                    self.status_banner = Some((
                                        false,
                                        "GIF cargado en vista previa (conecta el panel para subirlo a memoria).".to_string(),
                                    ));
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    self.status_banner = Some((false, e.clone()));
                    self.logs.push(format!("ERROR descargando URL: {e}"));
                }
            }
        }
    }

    fn start_url_download_for(
        &mut self,
        ctx: &egui::Context,
        url: String,
        action: UrlDownloadAction,
    ) {
        let url = url.trim().to_string();
        if url.is_empty() || self.downloading_url {
            return;
        }
        self.downloading_url = true;
        self.status_banner = None;
        self.logs.push(format!("Descargando GIF desde URL: {url}"));
        let (tx, rx) = mpsc::channel();
        self.url_rx = Some(rx);
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let res = download_bytes_from_url(&url);
            let _ = tx.send((action, res));
            ctx_clone.request_repaint();
        });
    }

    fn start_giphy_search(&mut self, ctx: &egui::Context, load_more: bool) {
        if self.giphy_searching {
            return;
        }
        let (q, offset) = if load_more {
            if !self.giphy_has_more || self.giphy_active_query.is_empty() {
                return;
            }
            (self.giphy_active_query.clone(), self.giphy_offset)
        } else {
            let trimmed = self.giphy_query.trim().to_string();
            if trimmed.is_empty() {
                return;
            }
            self.giphy_active_query = trimmed.clone();
            self.giphy_offset = 0;
            self.giphy_has_more = false;
            self.giphy_results.clear();
            (trimmed, 0)
        };

        self.giphy_searching = true;
        self.status_banner = None;
        self.logs
            .push(format!("Buscando en Giphy: '{q}' (offset {offset})..."));
        let (tx, rx) = mpsc::channel();
        self.giphy_rx = Some(rx);
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let res = search_giphy(&q, offset, 12);
            let _ = tx.send((load_more, res));
            ctx_clone.request_repaint();
        });
    }

    fn poll_giphy_search(&mut self, ctx: &egui::Context) {
        let mut finished = None;
        if let Some(rx) = &self.giphy_rx {
            if let Ok(res) = rx.try_recv() {
                finished = Some(res);
            }
        }
        if let Some((load_more, res)) = finished {
            self.giphy_searching = false;
            self.giphy_rx = None;
            match res {
                Ok(page) => {
                    let count = page.items.len();
                    self.giphy_offset = page.next_offset;
                    self.giphy_has_more = page.has_more;
                    let cards: Vec<GiphyCard> = page
                        .items
                        .into_iter()
                        .map(|item| {
                            let mut textures = Vec::with_capacity(item.thumb_frames.len());
                            let mut delays_ms = Vec::with_capacity(item.thumb_frames.len());
                            for (i, f) in item.thumb_frames.iter().enumerate() {
                                let img =
                                    ColorImage::from_rgba_unmultiplied(item.thumb_size, &f.rgba);
                                textures.push(ctx.load_texture(
                                    format!("giphy_thumb_{}_{}", item.id, i),
                                    img,
                                    TextureOptions::LINEAR,
                                ));
                                delays_ms.push(f.delay_ms);
                            }
                            GiphyCard {
                                item,
                                textures,
                                delays_ms,
                                frame_idx: 0,
                                last_switch: Instant::now(),
                            }
                        })
                        .collect();

                    if load_more {
                        self.giphy_results.extend(cards);
                    } else {
                        self.giphy_results = cards;
                    }
                    self.logs.push(format!(
                        "Giphy: +{} resultados (total en vista: {}).",
                        count,
                        self.giphy_results.len()
                    ));
                }
                Err(e) => {
                    self.status_banner = Some((false, e.clone()));
                    self.logs.push(format!("ERROR Giphy: {e}"));
                }
            }
        }
    }

    fn load_preview_source(&mut self, ctx: &egui::Context, source: GifSource) {
        let res = match &source {
            GifSource::File(path) => {
                load_and_prepare_gif(path, self.resize_mode, self.force_reencode)
            }
            GifSource::Url {
                name,
                raw_bytes,
                is_gif,
            } => prepare_gif_from_bytes(
                name.clone(),
                raw_bytes,
                *is_gif,
                self.resize_mode,
                self.force_reencode,
            ),
            GifSource::Prepared(gif) => Ok(gif.clone()),
        };

        match res {
            Ok(gif) => {
                let textures = gif
                    .preview_frames
                    .iter()
                    .enumerate()
                    .map(|(i, f)| {
                        let img = ColorImage::from_rgba_unmultiplied(
                            [usize::from(MATRIX_WIDTH), usize::from(MATRIX_HEIGHT)],
                            &f.rgba,
                        );
                        ctx.load_texture(
                            format!("preview_{}_{}", gif.name, i),
                            img,
                            TextureOptions::NEAREST,
                        )
                    })
                    .collect();
                self.preview = Some(ActivePreview {
                    gif,
                    source,
                    textures,
                    frame_idx: 0,
                    last_frame_switch: Instant::now(),
                });
                self.status_banner = None;
            }
            Err(e) => {
                self.status_banner = Some((false, e.clone()));
                self.logs.push(format!("ERROR cargando GIF: {e}"));
            }
        }
    }

    fn add_files_to_playlist(&mut self, paths: Vec<PathBuf>) {
        let mut added = false;
        for path in paths {
            if self.playlist.len() >= MAX_PLAYLIST_SLOTS {
                self.status_banner = Some((
                    false,
                    format!("Máximo de {MAX_PLAYLIST_SLOTS} elementos alcanzado."),
                ));
                break;
            }
            match load_and_prepare_gif(&path, self.resize_mode, self.force_reencode) {
                Ok(gif) => {
                    self.logs.push(format!(
                        "Añadido a lista: {} ({} bytes, {} frames)",
                        gif.name,
                        gif.gif_bytes.len(),
                        gif.preview_frames.len()
                    ));
                    self.playlist.push(PlaylistEntry { gif });
                    added = true;
                }
                Err(e) => {
                    self.logs.push(format!("ERROR añadiendo {:?}: {e}", path));
                }
            }
        }
        if added {
            self.persist_playlist();
        }
    }

    fn start_video_conversion(&mut self, ctx: &egui::Context) {
        let src = self.video_source.trim().to_string();
        if src.is_empty() || self.video_converting {
            return;
        }
        self.video_converting = true;
        self.status_banner = None;
        self.logs.push(format!("Convirtiendo vídeo a GIF 32x32: {src}"));

        let opts = VideoConvertOptions {
            fps: self.video_fps,
            start_sec: self.video_start_sec,
            max_duration_sec: self.video_duration_sec,
            resize_mode: self.video_resize_mode,
        };
        let (tx, rx) = mpsc::channel();
        self.video_rx = Some(rx);
        let ctx_clone = ctx.clone();
        std::thread::spawn(move || {
            let res = convert_video_to_gif(&src, &opts);
            let _ = tx.send(res);
            ctx_clone.request_repaint();
        });
    }

    fn poll_video_conversion(&mut self, ctx: &egui::Context) {
        let mut finished = None;
        if let Some(rx) = &self.video_rx {
            if let Ok(res) = rx.try_recv() {
                finished = Some(res);
            }
        }
        if let Some(res) = finished {
            self.video_converting = false;
            self.video_rx = None;
            match res {
                Ok(gif) => {
                    self.logs.push(format!(
                        "Vídeo convertido: {} ({} frames, {} B)",
                        gif.name,
                        gif.preview_frames.len(),
                        gif.gif_bytes.len()
                    ));
                    self.left_tab = LeftTab::Preview;
                    self.load_preview_source(ctx, GifSource::Prepared(gif));
                    self.status_banner = Some((
                        true,
                        "Vídeo convertido a GIF 32x32. Listo para enviar o añadir a la lista."
                            .to_string(),
                    ));
                }
                Err(e) => {
                    self.status_banner = Some((false, e.clone()));
                    self.logs.push(format!("ERROR vídeo: {e}"));
                }
            }
        }
    }

    fn draw_top_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(concat!("iPixel 32x32 v", env!("CARGO_PKG_VERSION")))
                    .size(20.0)
                    .strong()
                    .color(theme::TEXT_PRIMARY),
            );
            ui.add_space(12.0);

            if self.scanning {
                ui.spinner();
                ui.label(RichText::new("Escaneando...").color(theme::TEXT_SECONDARY));
            } else if ui.button("Escanear BLE").clicked() {
                self.scanning = true;
                self.ble.send(BleCommand::Scan);
            }

            let selected_label = self
                .selected_device_id
                .as_ref()
                .and_then(|id| self.devices.iter().find(|d| &d.id == id))
                .map(|d| format!("{} ({} dBm)", d.name, d.rssi.unwrap_or(0)))
                .unwrap_or_else(|| "Seleccionar panel...".to_string());

            egui::ComboBox::from_id_salt("ble_device_combo")
                .selected_text(selected_label)
                .width(230.0)
                .show_ui(ui, |ui| {
                    for dev in &self.devices {
                        let tag = if dev.matches_service { "* " } else { "" };
                        let label = format!("{}{} ({} dBm)", tag, dev.name, dev.rssi.unwrap_or(0));
                        ui.selectable_value(
                            &mut self.selected_device_id,
                            Some(dev.id.clone()),
                            label,
                        );
                    }
                });

            if let Some(name) = &self.connected_name {
                ui.label(RichText::new(format!("Conectado: {name}")).color(theme::SUCCESS));
                if ui.button("Desconectar").clicked() {
                    self.ble.send(BleCommand::Disconnect);
                }
            } else {
                let can_connect = self.selected_device_id.is_some() && !self.scanning;
                if ui.add_enabled(can_connect, primary_button("Conectar")).clicked() {
                    if let Some(id) = self.selected_device_id.clone() {
                        self.busy = true;
                        self.ble.send(BleCommand::Connect { device_id: id });
                    }
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let prev_mtu = self.mtu_chunk;
                egui::ComboBox::from_id_salt("mtu_combo")
                    .selected_text(format!("{} B", self.mtu_chunk))
                    .width(72.0)
                    .show_ui(ui, |ui| {
                        for val in [20usize, 100, 180, 244, 500] {
                            ui.selectable_value(&mut self.mtu_chunk, val, format!("{val} B"));
                        }
                    });
                if self.mtu_chunk != prev_mtu {
                    self.ble.send(BleCommand::SetMtuChunk(self.mtu_chunk));
                }
                ui.label(RichText::new("MTU").color(theme::TEXT_MUTED));
            });
        });

        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            let connected = self.connected_name.is_some();
            ui.add_enabled_ui(connected && !self.busy, |ui| {
                if ui.button("Encender").clicked() {
                    self.ble.send(BleCommand::SetPower(true));
                }
                if ui.button("Apagar").clicked() {
                    self.ble.send(BleCommand::SetPower(false));
                }
                ui.add_space(8.0);
                ui.label(RichText::new("Brillo").color(theme::TEXT_SECONDARY));
                let resp = ui.add(egui::Slider::new(&mut self.brightness, 1..=100).suffix("%"));
                if resp.drag_stopped() || (resp.changed() && !resp.dragged()) {
                    self.ble.send(BleCommand::SetBrightness(self.brightness));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Borrar memoria del panel").clicked() {
                        self.ble.send(BleCommand::DeleteAll);
                    }
                });
            });
        });
    }

    fn draw_single_gif_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, card_height: f32) {
        surface_frame().show(ui, |ui| {
            ui.set_min_height(card_height);
            ui.set_max_height(card_height);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.left_tab, LeftTab::Preview, "Vista previa 32x32");
                ui.selectable_value(&mut self.left_tab, LeftTab::Giphy, "Buscador Giphy");
                ui.selectable_value(&mut self.left_tab, LeftTab::Video, "Vídeo MP4 -> GIF");
            });
            ui.add_space(6.0);

            match self.left_tab {
                LeftTab::Giphy => {
                    self.draw_giphy_browser(ui, ctx);
                    return;
                }
                LeftTab::Video => {
                    self.draw_video_converter(ui, ctx);
                    return;
                }
                LeftTab::Preview => {}
            }

            ui.horizontal_wrapped(|ui| {
                if ui.button("Abrir archivo...").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Imágenes y GIFs", &["gif", "png", "jpg", "jpeg", "bmp", "webp"])
                        .pick_file()
                    {
                        self.load_preview_source(ctx, GifSource::File(path));
                    }
                }

                let prev_mode = self.resize_mode;
                let prev_force = self.force_reencode;
                egui::ComboBox::from_id_salt("resize_mode_combo")
                    .selected_text(self.resize_mode.label())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.resize_mode,
                            ResizeMode::PixelArtNearest,
                            ResizeMode::PixelArtNearest.label(),
                        );
                        ui.selectable_value(
                            &mut self.resize_mode,
                            ResizeMode::SmoothLanczos,
                            ResizeMode::SmoothLanczos.label(),
                        );
                    });
                ui.checkbox(&mut self.force_reencode, "Re-codificar");
                ui.checkbox(&mut self.show_led_grid, "Cuadrícula");

                if (self.resize_mode != prev_mode || self.force_reencode != prev_force)
                    && self.preview.is_some()
                {
                    if let Some(source) = self.preview.as_ref().map(|p| p.source.clone()) {
                        self.load_preview_source(ctx, source);
                    }
                }
            });

            ui.horizontal(|ui| {
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut self.url_input)
                        .hint_text("https://... (pega la URL de un GIF)")
                        .desired_width(ui.available_width() - 105.0),
                );
                let enter_pressed =
                    resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if self.downloading_url {
                    ui.spinner();
                } else if ui.button("Cargar URL").clicked() || enter_pressed {
                    let url = self.url_input.clone();
                    self.start_url_download_for(ctx, url, UrlDownloadAction::PreviewOnly);
                }
            });

            ui.add_space(10.0);

            let mut add_to_list: Option<ProcessedGif> = None;
            if let Some(prev) = &mut self.preview {
                let delay_ms = prev.gif.preview_frames[prev.frame_idx].delay_ms;
                if prev.gif.preview_frames.len() > 1
                    && prev.last_frame_switch.elapsed().as_millis() >= u128::from(delay_ms)
                {
                    prev.frame_idx = (prev.frame_idx + 1) % prev.textures.len();
                    prev.last_frame_switch = Instant::now();
                }
                if prev.gif.preview_frames.len() > 1 {
                    ctx.request_repaint_after(std::time::Duration::from_millis(20));
                }

                ui.horizontal(|ui| {
                    let size = 224.0;
                    let bezel = Frame::NONE
                        .fill(theme::MATRIX_BEZEL)
                        .stroke(Stroke::new(1.0, theme::BORDER_SUBTLE))
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(Margin::same(6));

                    bezel.show(ui, |ui| {
                        let (rect, _) =
                            ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 4.0, Color32::BLACK);
                        let tex = &prev.textures[prev.frame_idx];
                        ui.painter().image(
                            tex.id(),
                            rect,
                            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                            Color32::WHITE,
                        );

                        if self.show_led_grid {
                            let step = size / 32.0;
                            let stroke =
                                Stroke::new(0.6, Color32::from_rgba_unmultiplied(0, 0, 0, 120));
                            for i in 1..32 {
                                let offset = i as f32 * step;
                                ui.painter().line_segment(
                                    [
                                        Pos2::new(rect.left() + offset, rect.top()),
                                        Pos2::new(rect.left() + offset, rect.bottom()),
                                    ],
                                    stroke,
                                );
                                ui.painter().line_segment(
                                    [
                                        Pos2::new(rect.left(), rect.top() + offset),
                                        Pos2::new(rect.right(), rect.top() + offset),
                                    ],
                                    stroke,
                                );
                            }
                        }
                    });

                    ui.add_space(10.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&prev.gif.name).strong().color(theme::TEXT_PRIMARY));
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(format!(
                                "Escala: {}x{} -> 32x32",
                                prev.gif.original_size.0, prev.gif.original_size.1
                            ))
                            .color(theme::TEXT_SECONDARY),
                        );
                        ui.label(
                            RichText::new(format!("Fotogramas: {}", prev.gif.preview_frames.len()))
                                .color(theme::TEXT_SECONDARY),
                        );
                        ui.label(
                            RichText::new(format!(
                                "Tamaño: {:.2} KB ({} B)",
                                prev.gif.gif_bytes.len() as f32 / 1024.0,
                                prev.gif.gif_bytes.len()
                            ))
                            .color(theme::TEXT_SECONDARY),
                        );
                        ui.label(
                            RichText::new(if prev.gif.was_reencoded {
                                "Convertido a GIF89a 32x32"
                            } else {
                                "GIF 32x32 original"
                            })
                            .color(theme::TEXT_MUTED),
                        );
                    });
                });

                ui.add_space(12.0);
                let can_send = self.connected_name.is_some() && !self.busy;
                ui.horizontal_wrapped(|ui| {
                    ui.add_enabled_ui(can_send, |ui| {
                        if ui.add(primary_button("Enviar ahora")).clicked() {
                            self.busy = true;
                            self.ble.send(BleCommand::SendGif {
                                gif_bytes: prev.gif.gif_bytes.clone(),
                                slot: DIRECT_PLAY_SLOT,
                                precheck: false,
                            });
                        }
                    });
                    if ui.button("+ Añadir a lista").clicked() {
                        add_to_list = Some(prev.gif.clone());
                    }
                });

                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.add_enabled_ui(can_send, |ui| {
                        ui.label(RichText::new("Ranura").color(theme::TEXT_SECONDARY));
                        ui.add(egui::DragValue::new(&mut self.target_slot).range(1..=100));
                        if ui.button("Guardar en ranura").clicked() {
                            self.busy = true;
                            self.ble.send(BleCommand::SendGif {
                                gif_bytes: prev.gif.gif_bytes.clone(),
                                slot: self.target_slot,
                                precheck: true,
                            });
                        }
                        if ui.button("Borrar ranura").clicked() {
                            self.ble.send(BleCommand::DeleteSlots(vec![self.target_slot]));
                        }
                    });
                });
            } else {
                ui.add_space(40.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("Abre un GIF local o pega una URL arriba para previsualizarlo en la matriz 32x32.")
                            .color(theme::TEXT_MUTED),
                    );
                });
            }
            if let Some(gif) = add_to_list {
                if self.playlist.len() < MAX_PLAYLIST_SLOTS {
                    self.playlist.push(PlaylistEntry { gif });
                    self.persist_playlist();
                }
            }
        });
    }

    fn draw_video_converter(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label(
            RichText::new("Convierte un MP4 local o una URL directa de vídeo a GIF animado 32x32.")
                .color(theme::TEXT_SECONDARY),
        );
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            if ui.button("Elegir vídeo MP4...").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Vídeos", &["mp4", "mov", "avi", "wmv", "mkv", "webm"])
                    .pick_file()
                {
                    self.video_source = path.to_string_lossy().to_string();
                }
            }
        });

        ui.add_space(4.0);
        ui.label(RichText::new("Ruta local o URL directa (.mp4):").color(theme::TEXT_MUTED));
        ui.add(
            egui::TextEdit::singleline(&mut self.video_source)
                .hint_text("C:\\ruta\\video.mp4  o  https://.../video.mp4")
                .desired_width(ui.available_width()),
        );

        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            ui.label("Inicio (s):");
            ui.add(
                egui::DragValue::new(&mut self.video_start_sec)
                    .speed(0.2)
                    .range(0.0..=3600.0),
            );
            ui.add_space(8.0);
            ui.label("Duración max (s):");
            ui.add(egui::Slider::new(&mut self.video_duration_sec, 1.0..=12.0).fixed_decimals(1));
        });

        ui.horizontal_wrapped(|ui| {
            ui.label("FPS:");
            ui.add(egui::Slider::new(&mut self.video_fps, 2..=20));
            ui.add_space(8.0);
            ui.label("Escalado:");
            egui::ComboBox::from_id_salt("video_resize_combo")
                .selected_text(self.video_resize_mode.label())
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.video_resize_mode,
                        ResizeMode::SmoothLanczos,
                        ResizeMode::SmoothLanczos.label(),
                    );
                    ui.selectable_value(
                        &mut self.video_resize_mode,
                        ResizeMode::PixelArtNearest,
                        ResizeMode::PixelArtNearest.label(),
                    );
                });
        });

        ui.add_space(14.0);
        if self.video_converting {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    RichText::new("Extrayendo fotogramas y generando GIF 32x32...")
                        .color(theme::TEXT_SECONDARY),
                );
            });
        } else {
            let can_convert = !self.video_source.trim().is_empty();
            if ui
                .add_enabled(
                    can_convert,
                    primary_button("Convertir a GIF 32x32"),
                )
                .clicked()
            {
                self.start_video_conversion(ctx);
            }
        }
    }

    fn draw_giphy_browser(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.giphy_query)
                    .hint_text("pixel art, retro, cat, fire...")
                    .desired_width(ui.available_width() - 95.0),
            );
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if self.giphy_searching && self.giphy_results.is_empty() {
                ui.spinner();
            } else if ui.add(primary_button("Buscar")).clicked() || enter {
                self.start_giphy_search(ctx, false);
            }
        });

        ui.add_space(6.0);
        if self.downloading_url {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Descargando GIF seleccionado...").color(theme::TEXT_SECONDARY));
            });
            ui.add_space(4.0);
        }

        if self.giphy_results.is_empty() && !self.giphy_searching {
            ui.add_space(36.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("Escribe un término y pulsa 'Buscar' para ver miniaturas de giphy.com.")
                        .color(theme::TEXT_MUTED),
                );
            });
            return;
        }

        let mut pick_preview: Option<String> = None;
        let mut pick_playlist: Option<String> = None;
        let mut any_animated = false;

        for card in &mut self.giphy_results {
            if card.textures.len() > 1 {
                any_animated = true;
                let delay = card.delays_ms[card.frame_idx];
                if card.last_switch.elapsed().as_millis() >= u128::from(delay) {
                    card.frame_idx = (card.frame_idx + 1) % card.textures.len();
                    card.last_switch = Instant::now();
                }
            }
        }
        if any_animated {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }

        let mut trigger_load_more = false;
        let scroll_h = (ui.available_height() - 10.0).max(160.0);
        let scroll_out = egui::ScrollArea::vertical()
            .id_salt("giphy_grid_scroll")
            .max_height(scroll_h)
            .show(ui, |ui| {
                egui::Grid::new("giphy_grid")
                    .spacing([10.0, 10.0])
                    .show(ui, |ui| {
                        for (idx, card) in self.giphy_results.iter().enumerate() {
                            Frame::NONE
                                .fill(theme::BG_ELEVATED)
                                .stroke(Stroke::new(1.0, theme::BORDER_SUBTLE))
                                .corner_radius(CornerRadius::same(8))
                                .inner_margin(Margin::same(6))
                                .show(ui, |ui| {
                                    ui.vertical(|ui| {
                                        let tex = &card.textures[card.frame_idx];
                                        let thumb = egui::ImageButton::new((
                                            tex.id(),
                                            Vec2::new(112.0, 84.0),
                                        ));
                                        if ui
                                            .add(thumb)
                                            .on_hover_text(format!("Usar: {}", card.item.title))
                                            .clicked()
                                        {
                                            pick_preview = Some(card.item.gif_url.clone());
                                        }
                                        ui.horizontal(|ui| {
                                            if ui.small_button("Usar").clicked() {
                                                pick_preview = Some(card.item.gif_url.clone());
                                            }
                                            if ui.small_button("+ Lista").clicked() {
                                                pick_playlist = Some(card.item.gif_url.clone());
                                            }
                                        });
                                    });
                                });
                            if (idx + 1) % 3 == 0 {
                                ui.end_row();
                            }
                        }
                    });

                ui.add_space(8.0);
                ui.vertical_centered(|ui| {
                    if self.giphy_searching {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new("Cargando más GIFs...").color(theme::TEXT_SECONDARY));
                        });
                    } else if self.giphy_has_more && ui.button("Cargar más").clicked() {
                        trigger_load_more = true;
                    }
                });
            });

        let near_bottom = scroll_out.content_size.y > scroll_out.inner_rect.height()
            && scroll_out.state.offset.y + scroll_out.inner_rect.height()
                >= scroll_out.content_size.y - 60.0;
        if (near_bottom || trigger_load_more) && self.giphy_has_more && !self.giphy_searching {
            self.start_giphy_search(ctx, true);
        }

        if let Some(url) = pick_preview {
            self.url_input = url.clone();
            self.start_url_download_for(ctx, url, UrlDownloadAction::PreviewAndSaveToDevice);
        } else if let Some(url) = pick_playlist {
            self.start_url_download_for(ctx, url, UrlDownloadAction::AddToPlaylist);
        }
    }

    fn draw_playlist_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, card_height: f32) {
        surface_frame().show(ui, |ui| {
            ui.set_min_height(card_height);
            ui.set_max_height(card_height);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Lista de reproducción")
                        .size(16.0)
                        .strong()
                        .color(theme::TEXT_PRIMARY),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if !self.playlist.is_empty() && ui.button("Vaciar").clicked() {
                        self.playlist.clear();
                        self.persist_playlist();
                    }
                    if ui.button("+ Añadir GIFs...").clicked() {
                        if let Some(paths) = rfd::FileDialog::new()
                            .add_filter(
                                "Imágenes y GIFs",
                                &["gif", "png", "jpg", "jpeg", "bmp", "webp"],
                            )
                            .pick_files()
                        {
                            self.add_files_to_playlist(paths);
                        }
                    }
                });
            });

            ui.add_space(4.0);
            if ui
                .checkbox(
                    &mut self.clear_before_upload,
                    "Limpiar memoria del panel antes de subir",
                )
                .changed()
            {
                self.persist_playlist();
            }
            ui.add_space(6.0);

            let mut move_up = None;
            let mut move_down = None;
            let mut remove_idx = None;
            let mut preview_idx = None;

            let list_h = (ui.available_height() - 52.0).max(140.0);
            egui::ScrollArea::vertical()
                .id_salt("playlist_scroll")
                .max_height(list_h)
                .show(ui, |ui| {
                    if self.playlist.is_empty() {
                        ui.add_space(40.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new("Sin elementos. Añade GIFs desde archivo o desde la vista previa.")
                                    .color(theme::TEXT_MUTED),
                            );
                        });
                        return;
                    }

                    for (idx, item) in self.playlist.iter().enumerate() {
                        Frame::NONE
                            .fill(theme::BG_ELEVATED)
                            .corner_radius(CornerRadius::same(6))
                            .inner_margin(Margin::symmetric(10, 6))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(format!("{:02}", idx + 1))
                                            .monospace()
                                            .strong()
                                            .color(theme::ACCENT),
                                    );
                                    ui.label(
                                        RichText::new(&item.gif.name).color(theme::TEXT_PRIMARY),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "{:.1} KB · {} f",
                                            item.gif.gif_bytes.len() as f32 / 1024.0,
                                            item.gif.preview_frames.len()
                                        ))
                                        .color(theme::TEXT_MUTED),
                                    );
                                    ui.with_layout(
                                        Layout::right_to_left(Align::Center),
                                        |ui| {
                                            if ui.small_button("X").clicked() {
                                                remove_idx = Some(idx);
                                            }
                                            if idx + 1 < self.playlist.len()
                                                && ui.small_button("v").clicked()
                                            {
                                                move_down = Some(idx);
                                            }
                                            if idx > 0 && ui.small_button("^").clicked() {
                                                move_up = Some(idx);
                                            }
                                            if ui.small_button("Ver").clicked() {
                                                preview_idx = Some(idx);
                                            }
                                        },
                                    );
                                });
                            });
                        ui.add_space(4.0);
                    }
                });

            let mut changed = false;
            if let Some(i) = move_up {
                self.playlist.swap(i, i - 1);
                changed = true;
            }
            if let Some(i) = move_down {
                self.playlist.swap(i, i + 1);
                changed = true;
            }
            if let Some(i) = remove_idx {
                self.playlist.remove(i);
                changed = true;
            }
            if changed {
                self.persist_playlist();
            }
            if let Some(i) = preview_idx {
                if let Some(entry) = self.playlist.get(i) {
                    let gif = entry.gif.clone();
                    self.left_tab = LeftTab::Preview;
                    self.load_preview_source(ctx, GifSource::Prepared(gif));
                }
            }

            ui.add_space(10.0);
            let can_upload =
                self.connected_name.is_some() && !self.busy && !self.playlist.is_empty();
            ui.add_enabled_ui(can_upload, |ui| {
                let label = format!("Subir lista al panel ({} elementos)", self.playlist.len());
                if ui
                    .add_sized([ui.available_width(), 34.0], primary_button(&label))
                    .clicked()
                {
                    let items: Vec<(u8, Vec<u8>)> = self
                        .playlist
                        .iter()
                        .enumerate()
                        .map(|(i, entry)| ((i + 1) as u8, entry.gif.gif_bytes.clone()))
                        .collect();
                    self.busy = true;
                    self.ble.send(BleCommand::UploadPlaylist {
                        items,
                        clear_before: self.clear_before_upload,
                    });
                }
            });
        });
    }
}

impl eframe::App for IPixelApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_ble_events();
        self.poll_url_download(ctx);
        self.poll_giphy_search(ctx);
        self.poll_video_conversion(ctx);

        egui::TopBottomPanel::top("top_bar")
            .frame(
                Frame::NONE
                    .fill(theme::BG_SURFACE)
                    .stroke(Stroke::new(1.0, theme::BORDER_SUBTLE))
                    .inner_margin(Margin::symmetric(16, 12)),
            )
            .show(ctx, |ui| {
                self.draw_top_header(ui);
                if let Some((frac, msg)) = &self.progress {
                    ui.add_space(8.0);
                    ui.add(egui::ProgressBar::new(*frac).text(msg).show_percentage());
                }
                if let Some((ok, msg)) = &self.status_banner {
                    ui.add_space(6.0);
                    let color = if *ok { theme::SUCCESS } else { theme::DANGER };
                    ui.label(RichText::new(msg).color(color));
                }
            });

        egui::TopBottomPanel::bottom("log_panel")
            .frame(
                Frame::NONE
                    .fill(theme::BG_SURFACE)
                    .stroke(Stroke::new(1.0, theme::BORDER_SUBTLE))
                    .inner_margin(Margin::symmetric(16, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let toggle_label = if self.show_logs {
                        "Ocultar registro BLE"
                    } else {
                        "Mostrar registro BLE"
                    };
                    if ui.small_button(toggle_label).clicked() {
                        self.show_logs = !self.show_logs;
                    }
                    if self.show_logs && ui.small_button("Limpiar").clicked() {
                        self.logs.clear();
                    }
                    if let Some(last) = self.logs.last() {
                        ui.label(RichText::new(last).color(theme::TEXT_MUTED).small());
                    }
                });
                if self.show_logs {
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical()
                        .max_height(110.0)
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for line in &self.logs {
                                ui.label(
                                    RichText::new(line)
                                        .monospace()
                                        .small()
                                        .color(theme::TEXT_SECONDARY),
                                );
                            }
                        });
                }
            });

        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(theme::BG_CANVAS).inner_margin(Margin::same(16)))
            .show(ctx, |ui| {
                let card_height = (ui.available_height() - 32.0).max(360.0);
                ui.columns(2, |cols| {
                    self.draw_single_gif_panel(&mut cols[0], ctx, card_height);
                    self.draw_playlist_panel(&mut cols[1], ctx, card_height);
                });
            });
    }
}
