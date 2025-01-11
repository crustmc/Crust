use log::{debug, error, info, warn};
use std::fmt::Display;
use std::{
    io::Cursor,
    net::SocketAddr,
    ops::DerefMut,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc,
    },
    time::SystemTime,
};
use std::time::Duration;
use tokio::{
    net::{tcp::OwnedReadHalf, TcpStream},
    sync::{mpsc::Sender, Mutex, Notify, RwLock},
    task::AbortHandle,
};
use tokio::time::sleep;
use uuid::Uuid;
use super::{
    encryption::{PacketDecryption, PacketEncryption},
    packet_handler::ClientPacketHandler,
    packets::{ClientSettings, PlayerPublicKey, ProtocolState},
    ProxyServer,
};
use crate::server::packets::ClientCustomPayload;
use crate::util::{IOError, IOResult};
use crate::{
    auth::LoginResult,
    chat::Text,
    server::{
        packet_ids::{PacketRegistry, ServerPacketType},
        packets::{self, encode_and_send_packet, read_and_decode_packet, Kick},
        ProxiedPlayer,
    },
    util::{Handle, VarInt, WeakHandle},
};

pub(crate) struct ProxyingData {
    pub login_result: LoginResult,
    pub version: i32,
    pub compression_threshold: i32,
    pub encryption: Option<(PacketEncryption, PacketDecryption)>,
    pub player_public_key: Option<PlayerPublicKey>,
    pub protocol_state: ProtocolState,
    pub address: SocketAddr,
}

pub(crate) struct PlayerSyncData {
    pub is_switching_server: Mutex<bool>,
    pub config_ack_notify: Notify,
    pub game_ack_notify: Notify,
    pub client_settings: Mutex<Option<ClientSettings>>,
    pub brand_packet: Mutex<Option<ClientCustomPayload>>,
}

pub struct ClientHandle {
    pub player: WeakHandle<ProxiedPlayer>,
    pub version: i32,
    pub connection: ConnectionHandle,
}

pub async fn handle(mut stream: TcpStream, data: ProxyingData) {
    let display_name = format!("[{}|{}]", data.login_result.name, data.address);
    let uuid = Uuid::try_parse(data.login_result.id.as_str());
    if uuid.is_err() {
        error!("{} Could not parse UUID {}", display_name, data.login_result.id.as_str());
        return;
    }

    let proxy_server = ProxyServer::instance();

    let uuid = uuid.unwrap();

    let data_address = data.address.clone();
    let data_login_result = data.login_result.clone();
    let data_player_public_key = data.player_public_key.clone();

    let (mut encryption, decryption) = match data.encryption {
        Some(encryption) => (Some(encryption.0), Some(encryption.1)),
        None => (None, None),
    };

    let (read, mut write) = stream.into_split();
    let compression_threshold = data.compression_threshold;

    let (sender, mut receiver) = tokio::sync::mpsc::channel(1000);

    let write_task = tokio::spawn(async move {
        let mut protocol_buf = Vec::new();
        let mut drop_redundant = false;
        let mut in_bundle = false;

        while let Some(event) = receiver.recv().await {
            match event {
                PacketSending::Packet(packet, bypass) => {
                    if drop_redundant && !bypass {
                        break;
                    }
                    let res = encode_and_send_packet(
                        &mut write,
                        &packet,
                        &mut protocol_buf,
                        compression_threshold,
                        &mut encryption,
                    )
                    .await;
                    if let Err(_e) = res {
                        // TODO: Handle error
                        break;
                    }
                }
                PacketSending::Sync(sender) => {
                    let _ = sender.send(());
                }
                PacketSending::DropRedundant(drop) => {
                    drop_redundant = drop;
                }
                PacketSending::BundleReceived => {
                    in_bundle = !in_bundle;
                    if let Err(_e) = encode_and_send_packet(
                        &mut write,
                        &[0],
                        &mut protocol_buf,
                        compression_threshold,
                        &mut encryption,
                    )
                    .await
                    {
                        break;
                    }
                }
                PacketSending::StartConfig(version) => {
                    if in_bundle {
                        in_bundle = !in_bundle;
                        if let Err(_e) = encode_and_send_packet(
                            &mut write,
                            &[0],
                            &mut protocol_buf,
                            compression_threshold,
                            &mut encryption,
                        )
                        .await
                        {
                            break;
                        }
                    }
                    if let Some(packet_id) = PacketRegistry::instance().get_server_packet_id(
                        ProtocolState::Game,
                        version,
                        ServerPacketType::ClientboundStartConfigurationPacket,
                    ) {
                        if let Err(_e) = encode_and_send_packet(
                            &mut write,
                            &{
                                let mut packet = vec![];
                                VarInt(packet_id).encode(&mut packet, 5).unwrap();
                                packet
                            },
                            &mut protocol_buf,
                            compression_threshold,
                            &mut encryption,
                        )
                        .await
                        {
                            break;
                        }
                    }
                }

                PacketSending::StartGame(version) => {
                    if in_bundle {
                        unreachable!("cant be in bundle while in config state")
                    }
                    if let Some(packet_id) = PacketRegistry::instance().get_server_packet_id(
                        ProtocolState::Config,
                        version,
                        ServerPacketType::ClientboundFinishConfigurationPacket,
                    ) {
                        if let Err(_e) = encode_and_send_packet(
                            &mut write,
                            &{
                                let mut packet = vec![];
                                VarInt(packet_id).encode(&mut packet, 5).unwrap();
                                packet
                            },
                            &mut protocol_buf,
                            compression_threshold,
                            &mut encryption,
                        )
                        .await
                        {
                            break;
                        }
                    }
                }
            }
        }
    });

    let player_sync_data = PlayerSyncData {
        is_switching_server: Mutex::new(false),
        game_ack_notify: Notify::new(),
        config_ack_notify: Notify::new(),
        client_settings: Mutex::new(None),
        brand_packet: Mutex::new(None),
    };
    let handle = ConnectionHandle::new(
        display_name.clone(),
        sender,
        read,
        data.protocol_state,
        write_task.abort_handle(),
        compression_threshold,
        decryption,
        data.address,
    );
    let disconnect_lock = handle.disconnect_wait.clone();

    let mut player = Handle::new(ProxiedPlayer {
        name: data.login_result.name.clone(),
        uuid,
        client_handle: handle.clone(),
        current_server: None,
        login_result: data.login_result,
        protocol_version: data.version,
        server_handle: None,
        player_public_key: data.player_public_key,
        sync_data: player_sync_data,
    });

    let mut players_by_name = proxy_server.player_by_name.write().await;
    let mut players_by_uuid = proxy_server.player_by_uuid.write().await;

    if players_by_uuid.contains_key(&player.uuid) || players_by_name.contains_key(&player.name.to_ascii_lowercase()) {
        &player.kick(Text::new("§cYou are already connected to this proxy")).await.ok();
        return;
    }

    players_by_uuid.insert(player.uuid, player.downgrade());
    players_by_name.insert(player.name.to_ascii_lowercase(), player.downgrade());
    *unsafe {
        core::mem::transmute::<_, &mut usize>(
            &proxy_server.player_count as *const usize,
        )
    } += 1;

    drop(players_by_name);
    drop(players_by_uuid);


    let handle = ClientHandle {
        player: player.downgrade(),
        version: data.version,
        connection: handle,
    };
    let con_handle = handle.connection.clone();

    debug!("{} Connecting to priority servers...", display_name);
    let server_data = 'l: {
        let servers = ProxyServer::instance().servers().read().await;
        for server in servers.get_priorities() {
            let server_id = servers.get_server_by_name(server);
            if server_id.is_none() {
                warn!("{} Skipping, prioritized server not found!", display_name);
                continue;
            }
            let default_server = server_id.unwrap();
            let addr = default_server.address.clone();
            let label = default_server.label.clone();

            let backend = super::backend::connect(
                data_address,
                addr,
                "127.0.0.1".to_string(),
                25565,
                data_login_result.clone(),
                data_player_public_key.clone(),
                data.version,
            )
                .await;
            if let Err(e) = backend {
                warn!("[{}] Failed to connect to backend: {}", display_name, e);
                continue;
            }

            break 'l Some((server.to_owned(), label, backend.unwrap()));
        }
        None
    };

    let player_handle_clone = player.clone();

    tokio::spawn(async move {
        let disconnect_guard = disconnect_lock.write().await;
        let _ = write_task.await;
        let proxy_server = ProxyServer::instance();
        let mut player_by_name = proxy_server.player_by_name.write().await;
        let mut player_by_uuid = proxy_server.player_by_uuid.write().await;

        player_by_name.remove(&player_handle_clone.name.to_ascii_lowercase());
        player_by_uuid.remove(&player_handle_clone.uuid);
        *unsafe {
            core::mem::transmute::<_, &mut usize>(
                &proxy_server.player_count as *const usize,
            )
        } -= 1;
        drop(player_by_name);
        drop(player_by_uuid);

        if let Some(ref backend_handle) = player_handle_clone.server_handle {
            backend_handle.disconnect("client disconnected").await;
        }
        
        drop(disconnect_guard);
    });

    if server_data.is_none() {
        player.kick(Text::new("§cNo server found for you to connect")).await.ok();
        return;
    }

    let (server_name, label, backend) = server_data.unwrap();
    let (_backend_profile, backend_handle) = backend.begin_proxying(&server_name, handle).await;

    player.current_server = Some(label);
    player.server_handle = Some(backend_handle.clone());

    con_handle
        .spawn_read_task(
            true,
            display_name,
            backend_handle,
            player.downgrade(),
            data.version,
        )
        .await;
}

async fn read_task(
    packet_limit: bool,
    _display_name: String,
    partner: ConnectionHandle,
    self_handle: ConnectionHandle,
    player: WeakHandle<ProxiedPlayer>,
    version: i32,
) {
    let mut read_buf = Vec::new();
    let mut protocol_buf = Vec::new();
    let mut read = self_handle.reader.lock().await;
    let mut decryption = self_handle.decryption.lock().await;
    let mut packet_per_second = 0usize;
    let mut last_second = SystemTime::now();
    let mut should_forward = true;
    loop {
        let res = read_and_decode_packet(
            read.deref_mut(),
            &mut read_buf,
            &mut protocol_buf,
            self_handle.compression_threshold,
            decryption.deref_mut(),
        )
        .await;
        if let Err(e) = res {
            partner.disconnect(&e.to_string()).await;
            self_handle.disconnect(&e.to_string()).await;
            break;
        }

        if packet_limit {
            packet_per_second += 1;
            if packet_per_second >= 2000 {
                if let Ok(elapsed) = last_second.elapsed() {
                    if elapsed.as_millis() < 1000 {
                        self_handle.disconnect("to many packets").await;
                        partner.disconnect("to many packets").await;
                        break;
                    }
                    last_second = SystemTime::now();
                    packet_per_second = 0;
                }
            }
        }

        let packet_id = VarInt::decode_simple(&mut Cursor::new(&read_buf));
        if let Err(e) = packet_id {
            self_handle.disconnect(&e.to_string()).await;
            break;
        }
        let packet_id = packet_id.unwrap().get();
        let res = ClientPacketHandler::handle_packet(
            packet_id,
            &read_buf[VarInt::get_size(packet_id)..],
            version,
            &player,
            &self_handle,
        )
        .await;
        if let Err(e) = res {
            partner.disconnect(&e.to_string()).await;
            self_handle.disconnect(&e.to_string()).await;
            break;
        }
        if should_forward && res.unwrap() {
            if let Err(e) = partner.queue_packet(read_buf, false).await {
                partner.disconnect(&e.to_string()).await;
                should_forward = false;
            }
        }
        read_buf = Vec::new();
    }
}

pub(crate) enum PacketSending {
    Packet(Vec<u8>, bool),
    Sync(tokio::sync::oneshot::Sender<()>),
    DropRedundant(bool),
    BundleReceived,
    StartConfig(i32),
    StartGame(i32),
}

#[derive(Clone)]
pub struct ConnectionHandle {
    name: String,
    sender: Sender<PacketSending>,
    pub(crate) protocol_state: Arc<AtomicU8>,
    pub(crate) compression_threshold: i32,
    pub(crate) reader: Arc<Mutex<OwnedReadHalf>>,
    pub(crate) decryption: Arc<Mutex<Option<PacketDecryption>>>,
    write_task: AbortHandle,
    pub(crate) read_task: Arc<Mutex<Option<AbortHandle>>>,
    pub(crate) disconnect_wait: Arc<RwLock<()>>,
    pub address: SocketAddr,
    pub(crate) closed: Arc<AtomicBool>,
}

impl Display for ConnectionHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            format!(
                "[{}, {:?}, {:?}]",
                self.name, self.protocol_state, self.protocol_state
            )
        )
    }
}

impl ConnectionHandle {
    pub(crate) fn new(
        name: String,
        sender: Sender<PacketSending>,
        reader: OwnedReadHalf,
        protocol_state: ProtocolState,
        write_task: AbortHandle,
        compression_threshold: i32,
        decryption: Option<PacketDecryption>,
        address: SocketAddr,
    ) -> Self {
        Self {
            name,
            sender,
            reader: Arc::new(Mutex::new(reader)),
            write_task,
            read_task: Arc::new(Mutex::new(None)),
            compression_threshold,
            decryption: Arc::new(Mutex::new(decryption)),
            protocol_state: Arc::new(AtomicU8::new(protocol_state as u8)),
            disconnect_wait: Arc::new(RwLock::new(())),
            address,
            closed: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn protocol_state(&self) -> ProtocolState {
        unsafe { core::mem::transmute(self.protocol_state.load(Ordering::Relaxed)) }
    }

    pub fn set_protocol_state(&self, state: ProtocolState) {
        self.protocol_state.store(state as u8, Ordering::Relaxed);
    }

    pub(crate) async fn spawn_read_task(
        &self,
        packet_limiter: bool,
        display_name: String,
        partner: ConnectionHandle,
        player: WeakHandle<ProxiedPlayer>,
        version: i32,
    ) {
        let mut old_read_task = self.read_task.lock().await;
        if old_read_task.is_some() {
            panic!("Read task already running!");
        }
        let read_task = tokio::spawn(read_task(
            packet_limiter,
            display_name,
            partner,
            self.clone(),
            player,
            version,
        ));
        old_read_task.replace(read_task.abort_handle());
    }

    pub async fn sync(&self) -> IOResult<()> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.sender
            .send(PacketSending::Sync(sender))
            .await
            .map_err(|_| IOError::new(std::io::ErrorKind::Other, "Failed to queue sync packet!"))?;
        receiver
            .await
            .map_err(|_| IOError::new(std::io::ErrorKind::Other, "Failed to receive sync packet!"))
    }

    pub async fn queue_packet(&self, packet: Vec<u8>, bypass: bool) -> IOResult<()> {
        self.sender
            .send(PacketSending::Packet(packet, bypass))
            .await
            .map_err(|_| IOError::new(std::io::ErrorKind::Other, "Failed to queue packet!"))
    }

    pub async fn drop_redundant(&self, drop: bool) -> IOResult<()> {
        self.sender
            .send(PacketSending::DropRedundant(drop))
            .await
            .map_err(|_| {
                IOError::new(
                    std::io::ErrorKind::Other,
                    "Failed to queue drop redundant packet!",
                )
            })
    }

    pub async fn on_bundle(&self) -> IOResult<()> {
        self.sender
            .send(PacketSending::BundleReceived)
            .await
            .map_err(|_| {
                IOError::new(
                    std::io::ErrorKind::Other,
                    "Failed to queue bundle received packet!",
                )
            })
    }

    pub async fn goto_config(&self, version: i32) -> IOResult<()> {
        self.sender
            .send(PacketSending::StartConfig(version))
            .await
            .map_err(|_| {
                IOError::new(
                    std::io::ErrorKind::Other,
                    "Failed to queue start config packet!",
                )
            })
    }
    pub async fn goto_game(&self, version: i32) -> IOResult<()> {
        self.sender
            .send(PacketSending::StartGame(version))
            .await
            .map_err(|_| {
                IOError::new(
                    std::io::ErrorKind::Other,
                    "Failed to queue start config packet!",
                )
            })
    }

    pub async fn disconnect(&self, reason: &str) {
        if self.closed.load(Ordering::Relaxed) {
            debug!("{} disconnected twice: {}", self.name, reason);
            return;
        }
        self.closed.swap(true, Ordering::Relaxed);
        info!("{} disconnected: {}", self.name, reason);
        self.write_task.abort();
        if let Some(task) = self.read_task.lock().await.take() {
            task.abort();
        }
    }

    pub async fn wait_for_disconnect(&self) {
        let _ = self.disconnect_wait.read().await;
    }
}
