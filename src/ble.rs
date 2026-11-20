use crate::protocol::{
    build_brightness_cmd, build_delete_all_cmd, build_delete_slots_cmd, build_gif_block_packet,
    build_playlist_header_cmd, build_power_cmd, build_precheck_slot_cmd, compute_crc32, parse_ack,
    AckStatus, FRAME_BLOCK_SIZE, SERVICE_UUID, WRITE_CHAR_UUID,
};
use btleplug::api::{
    Central, Characteristic, CharPropFlags, Manager as _, Peripheral as _, ScanFilter, WriteType,
};
use btleplug::platform::{Adapter, Manager, Peripheral};
use eframe::egui;
use std::sync::mpsc::Sender;
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{self as tokio_mpsc, UnboundedReceiver, UnboundedSender};
use tokio::time::{sleep, timeout};

#[derive(Debug, Clone)]
pub struct DiscoveredDevice {
    pub id: String,
    pub name: String,
    pub rssi: Option<i16>,
    pub matches_service: bool,
}

#[derive(Debug, Clone)]
pub enum BleCommand {
    Scan,
    Connect {
        device_id: String,
    },
    Disconnect,
    SetMtuChunk(usize),
    SetPower(bool),
    SetBrightness(u8),
    DeleteAll,
    DeleteSlots(Vec<u8>),
    SendGif {
        gif_bytes: Vec<u8>,
        slot: u8,
        precheck: bool,
    },
    UploadPlaylist {
        items: Vec<(u8, Vec<u8>)>,
        clear_before: bool,
    },
}

#[derive(Debug, Clone)]
pub enum BleEvent {
    Log(String),
    ScanFinished(Vec<DiscoveredDevice>),
    Connected(String),
    Disconnected,
    Progress(f32, String),
    TransferSuccess(String),
    Error(String),
}

struct ConnectedState {
    peripheral: Peripheral,
    write_char: Characteristic,
    write_type: WriteType,
    notify_rx: UnboundedReceiver<Vec<u8>>,
}

pub struct BleHandle {
    cmd_tx: UnboundedSender<BleCommand>,
}

impl BleHandle {
    pub fn spawn(event_tx: Sender<BleEvent>, repaint_ctx: egui::Context) -> Self {
        let (cmd_tx, mut cmd_rx) = tokio_mpsc::unbounded_channel();

        std::thread::spawn(move || {
            let rt = Runtime::new().expect("No se pudo crear runtime tokio");
            rt.block_on(async move {
                let mut worker = BleWorker::new(event_tx, repaint_ctx);
                while let Some(cmd) = cmd_rx.recv().await {
                    worker.handle_command(cmd).await;
                }
            });
        });

        Self { cmd_tx }
    }

    pub fn send(&self, cmd: BleCommand) {
        let _ = self.cmd_tx.send(cmd);
    }
}

struct BleWorker {
    event_tx: Sender<BleEvent>,
    repaint_ctx: egui::Context,
    adapter: Option<Adapter>,
    known_peripherals: Vec<Peripheral>,
    connected: Option<ConnectedState>,
    mtu_chunk: usize,
}

impl BleWorker {
    fn new(event_tx: Sender<BleEvent>, repaint_ctx: egui::Context) -> Self {
        Self {
            event_tx,
            repaint_ctx,
            adapter: None,
            known_peripherals: Vec::new(),
            connected: None,
            mtu_chunk: 180,
        }
    }

    fn emit(&self, ev: BleEvent) {
        let _ = self.event_tx.send(ev);
        self.repaint_ctx.request_repaint();
    }

    fn log(&self, msg: impl Into<String>) {
        self.emit(BleEvent::Log(msg.into()));
    }

    async fn ensure_adapter(&mut self) -> Result<Adapter, String> {
        if let Some(a) = &self.adapter {
            return Ok(a.clone());
        }
        let manager = Manager::new()
            .await
            .map_err(|e| format!("Error inicializando Bluetooth: {e}"))?;
        let adapters = manager
            .adapters()
            .await
            .map_err(|e| format!("Error listando adaptadores BLE: {e}"))?;
        let adapter = adapters
            .into_iter()
            .next()
            .ok_or_else(|| "No se encontró ningún adaptador Bluetooth en Windows.".to_string())?;
        self.adapter = Some(adapter.clone());
        Ok(adapter)
    }

    async fn handle_command(&mut self, cmd: BleCommand) {
        match cmd {
            BleCommand::SetMtuChunk(size) => {
                self.mtu_chunk = size.clamp(20, 512);
                self.log(format!("Tamaño de fragmento MTU ajustado a {} bytes.", self.mtu_chunk));
            }
            BleCommand::Scan => {
                if let Err(e) = self.scan().await {
                    self.emit(BleEvent::Error(e));
                    self.emit(BleEvent::ScanFinished(Vec::new()));
                }
            }
            BleCommand::Connect { device_id } => {
                if let Err(e) = self.connect(&device_id).await {
                    self.emit(BleEvent::Error(e));
                }
            }
            BleCommand::Disconnect => {
                self.disconnect().await;
            }
            BleCommand::SetPower(on) => {
                if let Err(e) = self.write_raw(&build_power_cmd(on)).await {
                    self.emit(BleEvent::Error(e));
                } else {
                    self.log(if on { "Pantalla encendida." } else { "Pantalla apagada." });
                }
            }
            BleCommand::SetBrightness(val) => {
                if let Err(e) = self.write_raw(&build_brightness_cmd(val)).await {
                    self.emit(BleEvent::Error(e));
                } else {
                    self.log(format!("Brillo ajustado al {}%.", val));
                }
            }
            BleCommand::DeleteAll => {
                if let Err(e) = self.write_raw(&build_delete_all_cmd()).await {
                    self.emit(BleEvent::Error(e));
                } else {
                    self.emit(BleEvent::TransferSuccess(
                        "Memoria del panel borrada.".to_string(),
                    ));
                }
            }
            BleCommand::DeleteSlots(slots) => {
                if slots.is_empty() {
                    return;
                }
                if let Err(e) = self.write_raw(&build_delete_slots_cmd(&slots)).await {
                    self.emit(BleEvent::Error(e));
                } else {
                    self.emit(BleEvent::TransferSuccess(format!(
                        "Ranuras {:?} borradas.",
                        slots
                    )));
                }
            }
            BleCommand::SendGif {
                gif_bytes,
                slot,
                precheck,
            } => {
                if let Err(e) = self.send_single_gif(&gif_bytes, slot, precheck, 0.0, 1.0).await {
                    self.emit(BleEvent::Error(e));
                } else {
                    self.emit(BleEvent::TransferSuccess(format!(
                        "GIF enviado correctamente (ranura {}).",
                        slot
                    )));
                }
            }
            BleCommand::UploadPlaylist {
                items,
                clear_before,
            } => {
                if let Err(e) = self.upload_playlist(items, clear_before).await {
                    self.emit(BleEvent::Error(e));
                } else {
                    self.emit(BleEvent::TransferSuccess(
                        "Lista de reproducción subida correctamente.".to_string(),
                    ));
                }
            }
        }
    }

    async fn scan(&mut self) -> Result<(), String> {
        let adapter = self.ensure_adapter().await?;
        self.log("Escaneando dispositivos BLE durante 4 segundos...");
        adapter
            .start_scan(ScanFilter::default())
            .await
            .map_err(|e| format!("Error iniciando escaneo BLE: {e}"))?;

        sleep(Duration::from_secs(4)).await;
        let _ = adapter.stop_scan().await;

        let peripherals = adapter
            .peripherals()
            .await
            .map_err(|e| format!("Error leyendo periféricos: {e}"))?;

        let mut list = Vec::new();
        for p in &peripherals {
            if let Ok(Some(props)) = p.properties().await {
                let name = props
                    .local_name
                    .clone()
                    .unwrap_or_else(|| "(Sin nombre)".to_string());
                let matches_service = props.services.contains(&SERVICE_UUID);
                // Keep named devices or ones advertising the iPixel service.
                if props.local_name.is_some() || matches_service {
                    list.push(DiscoveredDevice {
                        id: p.id().to_string(),
                        name,
                        rssi: props.rssi,
                        matches_service,
                    });
                }
            }
        }

        // Prioritize devices matching service or "LED"/"iPixel" in name, then strongest RSSI.
        list.sort_by(|a, b| {
            let a_likely = a.matches_service
                || a.name.to_uppercase().contains("LED")
                || a.name.to_uppercase().contains("PIXEL");
            let b_likely = b.matches_service
                || b.name.to_uppercase().contains("LED")
                || b.name.to_uppercase().contains("PIXEL");
            b_likely
                .cmp(&a_likely)
                .then_with(|| b.rssi.unwrap_or(-127).cmp(&a.rssi.unwrap_or(-127)))
        });

        self.known_peripherals = peripherals;
        self.log(format!("Escaneo completado: {} dispositivos encontrados.", list.len()));
        self.emit(BleEvent::ScanFinished(list));
        Ok(())
    }

    async fn connect(&mut self, device_id: &str) -> Result<(), String> {
        self.disconnect().await;

        let peripheral = self
            .known_peripherals
            .iter()
            .find(|p| p.id().to_string() == device_id)
            .cloned()
            .ok_or_else(|| "Dispositivo no encontrado. Vuelve a escanear.".to_string())?;

        let name = peripheral
            .properties()
            .await
            .ok()
            .flatten()
            .and_then(|p| p.local_name)
            .unwrap_or_else(|| device_id.to_string());

        self.log(format!("Conectando a {name}..."));
        peripheral
            .connect()
            .await
            .map_err(|e| format!("Error conectando a {name}: {e}"))?;

        peripheral
            .discover_services()
            .await
            .map_err(|e| format!("Error descubriendo servicios GATT: {e}"))?;

        let chars = peripheral.characteristics();
        let write_char = chars
            .iter()
            .find(|c| c.service_uuid == SERVICE_UUID && c.uuid == WRITE_CHAR_UUID)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "No se encontró la característica de escritura ({WRITE_CHAR_UUID}) en el servicio {SERVICE_UUID}."
                )
            })?;

        let write_type = if write_char.properties.contains(CharPropFlags::WRITE) {
            WriteType::WithResponse
        } else {
            WriteType::WithoutResponse
        };

        for c in chars.iter().filter(|c| c.service_uuid == SERVICE_UUID) {
            self.log(format!("Char {} props: {:?}", c.uuid, c.properties));
        }

        let mut subscribed_count = 0usize;
        for c in chars.iter().filter(|c| c.service_uuid == SERVICE_UUID) {
            if c.properties.contains(CharPropFlags::NOTIFY)
                || c.properties.contains(CharPropFlags::INDICATE)
            {
                if let Err(e) = peripheral.subscribe(c).await {
                    self.log(format!("Aviso: no se pudo suscribir a {}: {e}", c.uuid));
                } else {
                    self.log(format!("Suscrito a notificaciones en {}", c.uuid));
                    subscribed_count += 1;
                }
            }
        }

        if subscribed_count == 0 {
            self.log("Aviso: no se encontraron características Notify/Indicate en 00FA.");
        }

        let (notify_tx, notify_rx) = tokio_mpsc::unbounded_channel();
        let mut notif_stream = peripheral
            .notifications()
            .await
            .map_err(|e| format!("Error abriendo stream de notificaciones: {e}"))?;

        tokio::spawn(async move {
            use btleplug::api::ValueNotification;
            use futures::StreamExt;
            while let Some(ValueNotification { value, .. }) = notif_stream.next().await {
                if notify_tx.send(value).is_err() {
                    break;
                }
            }
        });

        self.connected = Some(ConnectedState {
            peripheral,
            write_char,
            write_type,
            notify_rx,
        });

        self.emit(BleEvent::Connected(name.clone()));
        self.log(format!("Conectado a {name} (WriteType: {:?}).", write_type));

        // Handshake inicial (getLedType): sincroniza hora/estado como hace la app Android al conectar
        let init_cmd = [8, 0, 1, 0x80, 12, 0, 0, 0];
        if let Err(e) = self.write_raw(&init_cmd).await {
            self.log(format!("Aviso en handshake inicial: {e}"));
        } else if let Ok(ack) = self.wait_for_ack(2).await {
            self.log(format!("Handshake inicial ACK: {:?}", ack));
        }

        // Fija el brillo al 100% automáticamente al conectar
        if let Err(e) = self.write_raw(&build_brightness_cmd(100)).await {
            self.log(format!("Aviso fijando brillo al 100%: {e}"));
        } else {
            self.log("Brillo fijado automáticamente al 100%.");
        }
        Ok(())
    }

    async fn disconnect(&mut self) {
        if let Some(state) = self.connected.take() {
            let _ = state.peripheral.disconnect().await;
            self.emit(BleEvent::Disconnected);
            self.log("Desconectado.");
        }
    }

    async fn write_raw(&mut self, data: &[u8]) -> Result<(), String> {
        let state = self
            .connected
            .as_ref()
            .ok_or_else(|| "No hay ningún panel conectado.".to_string())?;

        for chunk in data.chunks(self.mtu_chunk) {
            state
                .peripheral
                .write(&state.write_char, chunk, state.write_type)
                .await
                .map_err(|e| format!("Error escribiendo BLE: {e}"))?;
            if state.write_type == WriteType::WithoutResponse {
                sleep(Duration::from_millis(8)).await;
            }
        }
        Ok(())
    }

    async fn drain_notifications(&mut self) {
        if let Some(state) = &mut self.connected {
            while state.notify_rx.try_recv().is_ok() {}
        }
    }

    async fn wait_for_ack(&mut self, timeout_secs: u64) -> Result<AckStatus, String> {
        let state = self
            .connected
            .as_mut()
            .ok_or_else(|| "Panel desconectado durante la espera de ACK.".to_string())?;

        let deadline = Duration::from_secs(timeout_secs);
        loop {
            match timeout(deadline, state.notify_rx.recv()).await {
                Ok(Some(bytes)) => {
                    let _ = self.event_tx.send(BleEvent::Log(format!(
                        "RX Notify ({} B): {:02X?}",
                        bytes.len(),
                        bytes
                    )));
                    if let Some(ack) = parse_ack(&bytes) {
                        return Ok(ack);
                    }
                }
                Ok(None) => return Err("Canal de notificaciones BLE cerrado.".to_string()),
                Err(_) => return Err("Timeout esperando confirmación (ACK) del panel.".to_string()),
            }
        }
    }

    async fn send_single_gif(
        &mut self,
        gif_bytes: &[u8],
        slot: u8,
        precheck: bool,
        progress_base: f32,
        progress_span: f32,
    ) -> Result<(), String> {
        if gif_bytes.is_empty() {
            return Err("El archivo GIF está vacío.".to_string());
        }

        if precheck {
            self.drain_notifications().await;
            let pre_cmd = build_precheck_slot_cmd(slot);
            self.write_raw(&pre_cmd).await?;
            match self.wait_for_ack(4).await {
                Ok(AckStatus::InvalidCommand) => {
                    return Err("El panel rechazó la pre-comprobación de ranura.".to_string())
                }
                Ok(AckStatus::OutOfMemory) => {
                    return Err("Memoria del panel llena.".to_string())
                }
                Ok(_) => {}
                Err(e) => self.log(format!("Aviso en pre-comprobación (continuando): {e}")),
            }
        }

        let crc = compute_crc32(gif_bytes);
        let blocks: Vec<&[u8]> = gif_bytes.chunks(FRAME_BLOCK_SIZE).collect();
        let total_blocks = blocks.len();

        for (idx, block) in blocks.iter().enumerate() {
            let is_continuation = idx > 0;
            let packet = build_gif_block_packet(block, gif_bytes.len(), crc, is_continuation, slot);
            self.log(format!(
                "TX GIF bloque {}/{} ({} B, slot {}, crc {:08X})",
                idx + 1,
                total_blocks,
                packet.len(),
                slot,
                crc
            ));

            let mut attempts = 0u8;
            loop {
                attempts += 1;
                self.drain_notifications().await;
                self.write_raw(&packet).await?;

                match self.wait_for_ack(5).await {
                    Ok(AckStatus::ContinueNextBlock) => break,
                    Ok(AckStatus::Completed) => {
                        let p = progress_base + progress_span;
                        self.emit(BleEvent::Progress(p, "Completado".to_string()));
                        return Ok(());
                    }
                    Ok(AckStatus::InvalidCommand) => {
                        return Err("El panel respondió: Comando inválido (0x00).".to_string());
                    }
                    Ok(AckStatus::OutOfMemory) => {
                        return Err("El panel respondió: Memoria insuficiente (0x02).".to_string());
                    }
                    Ok(AckStatus::Unknown(code)) => {
                        self.log(format!("ACK desconocido ({code}) en bloque {}/{total_blocks}", idx + 1));
                        break;
                    }
                    Err(e) if attempts < 3 => {
                        self.log(format!(
                            "Reintentando bloque {}/{total_blocks} (intento {attempts}/3): {e}",
                            idx + 1
                        ));
                    }
                    Err(e) => return Err(e),
                }
            }

            let frac = (idx + 1) as f32 / total_blocks as f32;
            self.emit(BleEvent::Progress(
                progress_base + progress_span * frac,
                format!("Bloque {}/{}", idx + 1, total_blocks),
            ));
        }

        Ok(())
    }

    async fn upload_playlist(
        &mut self,
        items: Vec<(u8, Vec<u8>)>,
        clear_before: bool,
    ) -> Result<(), String> {
        if items.is_empty() {
            return Err("La lista de reproducción está vacía.".to_string());
        }

        if clear_before {
            self.log("Limpiando memoria del panel antes de subir...");
            self.emit(BleEvent::Progress(0.02, "Borrando memoria...".to_string()));
            self.write_raw(&build_delete_all_cmd()).await?;
            sleep(Duration::from_millis(350)).await;
        }

        let slots: Vec<u8> = items.iter().map(|(slot, _)| *slot).collect();
        let header = build_playlist_header_cmd(&slots);
        self.log(format!("Enviando cabecera de lista ({} elementos)...", items.len()));
        self.drain_notifications().await;
        self.write_raw(&header).await?;

        match self.wait_for_ack(5).await {
            Ok(AckStatus::ContinueNextBlock) | Ok(AckStatus::Completed) => {}
            Ok(AckStatus::InvalidCommand) => {
                return Err("El panel rechazó la cabecera de lista de reproducción.".to_string())
            }
            Ok(AckStatus::OutOfMemory) => {
                return Err("Memoria del panel insuficiente para la lista.".to_string())
            }
            Ok(AckStatus::Unknown(c)) => {
                self.log(format!("Aviso: ACK inesperado ({c}) en cabecera de lista."));
            }
            Err(e) => {
                self.log(format!("Aviso esperando ACK de cabecera (continuando): {e}"));
            }
        }

        let total = items.len() as f32;
        for (i, (slot, gif_bytes)) in items.into_iter().enumerate() {
            let base = i as f32 / total;
            let span = 1.0 / total;
            self.log(format!("Subiendo elemento {}/{} en ranura {}...", i + 1, total as usize, slot));
            self.send_single_gif(&gif_bytes, slot, false, base, span)
                .await?;
            sleep(Duration::from_millis(200)).await;
        }

        Ok(())
    }
}
