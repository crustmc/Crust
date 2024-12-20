use std::{io::Cursor, net::SocketAddr, ops::DerefMut, sync::{atomic::{AtomicBool, AtomicU8, Ordering}, Arc}, time::SystemTime};
use std::fmt::Display;
use tokio::{net::{tcp::OwnedReadHalf, TcpStream}, sync::{mpsc::Sender, Mutex, Notify, RwLock}, task::AbortHandle};

use crate::{auth::GameProfile, chat::Text, server::{packet_ids::{PacketRegistry, ServerPacketType}, packets::{self, encode_and_send_packet, read_and_decode_packet, Kick}, ProxiedPlayer}, util::VarInt};
use crate::util::{IOError, IOResult};
use super::{encryption::{PacketDecryption, PacketEncryption}, packet_handler::ClientPacketHandler, packets::{ClientSettings, PlayerPublicKey, ProtocolState}, ProxyServer, SlotId};

pub(crate) struct ProxyingData {
    pub profile: GameProfile,
    pub version: i32,
    pub compression_threshold: i32,
    pub encryption: Option<(PacketEncryption, PacketDecryption)>,
    pub player_public_key: Option<PlayerPublicKey>,
    pub protocol_state: ProtocolState,
    pub address: SocketAddr,
}

pub(crate) struct PlayerSyncData {
    pub is_switching_server: AtomicBool,
    pub config_ack_notify: Notify,
    pub client_settings: Mutex<Option<ClientSettings>>,
    pub version: i32,
}

pub struct ClientHandle {
    pub player_id: SlotId,
    pub connection: ConnectionHandle,
}

#[inline]
pub async fn handle(mut stream: TcpStream, data: ProxyingData) {
    let display_name = format!("[{}|{}]", data.profile.name, data.address);
    log::debug!("{} Connecting to priority servers...", display_name);
    let server_data = 'l: {
        let servers = ProxyServer::instance().servers().read().await;
        for server in servers.get_priorities() {
            let server_id = servers.get_server_id_by_name(server);
            if server_id.is_none() {
                log::warn!("{} Skipping, prioritized server not found!", display_name);
                continue;
            }
            let server_id = server_id.unwrap();
            let default_server = servers.get_server(server_id).unwrap();
            let addr = default_server.address.clone();

            let backend = super::backend::connect(data.address, addr, "127.0.0.1".to_string(), 25565, data.profile.clone(), data.player_public_key.clone(), data.version).await;
            if let Err(e) = backend {
                log::warn!("[{}] Failed to connect to backend: {}", display_name, e);
                continue;
            }

            break 'l Some((server.to_owned(), server_id, backend.unwrap()));
        }
        None
    };

    if server_data.is_none() {
        let mut encryption = match data.encryption {
            Some((enc, _)) => Some(enc),
            _ => None
        };
        let mut buf = vec![];
        packets::get_full_server_packet_buf_write_buffer(&mut buf, &Kick {
            text: Text::new("§cNo server found for you to connect")
        }, data.version, data.protocol_state).unwrap();
        encode_and_send_packet(&mut stream, &buf, &mut vec![], data.compression_threshold, &mut encryption).await.ok();
        return;
    }

    let (server_name, server_id, backend) = server_data.unwrap();
    let (read, mut write) = stream.into_split();
    let (mut encryption, decryption) = match data.encryption {
        Some(encryption) => (Some(encryption.0), Some(encryption.1)),
        None => (None, None)
    };
    let compression_threshold = data.compression_threshold;

    let (sender, mut receiver) = tokio::sync::mpsc::channel(256);

    let write_task = tokio::spawn(async move {
        let mut protocol_buf = Vec::new();
        let mut drop_redundant = false;
        let mut in_bundle = false;
        while let Some(event) = receiver.recv().await {
            match event {
                PacketSending::Packet(packet, drop_bypass) => {
                    if drop_redundant && !drop_bypass {
                        continue;
                    }
                    let res = encode_and_send_packet(&mut write, &packet, &mut protocol_buf, compression_threshold, &mut encryption).await;
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
                    if let Err(_e) = encode_and_send_packet(&mut write, &[0], &mut protocol_buf, compression_threshold, &mut encryption).await {
                        break;
                    }
                }
                PacketSending::StartConfig(version) => {
                    if in_bundle {
                        in_bundle = !in_bundle;
                        if let Err(_e) = encode_and_send_packet(&mut write, &[0], &mut protocol_buf, compression_threshold, &mut encryption).await {
                            break;
                        }
                    }
                    if let Some(packet_id) = PacketRegistry::instance().get_server_packet_id(ProtocolState::Game, version, ServerPacketType::StartConfiguration) {
                        if let Err(_e) = encode_and_send_packet(&mut write, &{
                            let mut packet = vec![];
                            VarInt(packet_id).encode(&mut packet, 5).unwrap();
                            packet
                        }, &mut protocol_buf, compression_threshold, &mut encryption).await {
                            break;
                        }
                    }
                }
            }
        }
    });

    let player_sync_data = Arc::new(PlayerSyncData {
        is_switching_server: AtomicBool::new(false),
        config_ack_notify: Notify::new(),
        client_settings: Mutex::new(None),
        version: data.version,
    });
    let handle = ConnectionHandle::new(display_name.clone(), sender, read, data.protocol_state, write_task.abort_handle(), compression_threshold, decryption, Some(player_sync_data.clone()), data.address);
    let disconnect_lock = handle.disconnect_wait.clone();

    let player = ProxiedPlayer {
        player_id: unsafe {
            #[allow(
                invalid_value
            )] core::mem::MaybeUninit::zeroed().assume_init()
        },
        client_handle: handle.clone(),
        current_server: server_id,
        profile: data.profile,
        protocol_version: data.version,
        server_handle: None,
        player_public_key: data.player_public_key,
        sync_data: player_sync_data.clone(),
    };
    let player_id = {
        let mut players = ProxyServer::instance().players().write().await;
        let player_id = players.insert(player);
        *unsafe { core::mem::transmute::<_, &mut usize>(&ProxyServer::instance().player_count as *const usize) } += 1;
        players.get_mut(player_id).unwrap().player_id = player_id;
        drop(players);
        player_id
    };

    let handle = ClientHandle {
        player_id,
        connection: handle,
    };
    let con_handle = handle.connection.clone();
    let (_backend_profile, backend_handle) = backend.begin_proxying(&server_name, handle, player_sync_data).await;

    tokio::spawn(async move {
        let disconnect_guard = disconnect_lock.write().await;
        let _ = write_task.await;
        let mut lock = ProxyServer::instance().players().write().await;
        if let Some(player) = lock.remove(player_id) {
            *unsafe { core::mem::transmute::<_, &mut usize>(&ProxyServer::instance().player_count as *const usize) } -= 1;
            drop(lock);
            if let Some(ref backend_handle) = player.server_handle {
                backend_handle.disconnect("client disconnected").await;
            }
        } else {
            panic!("Tried to remove player that is for whatever reason not in the player list! This is not intended to happen!");
        }
        drop(disconnect_guard);
    });
    let mut lock = ProxyServer::instance().players().write().await;
    if let Some(player) = lock.get_mut(player_id) {
        player.server_handle = Some(backend_handle.clone());
    } else {
        return;
    }
    drop(lock);

    con_handle.spawn_read_task(true, display_name, backend_handle, player_id, data.version).await;
}

#[inline]
async fn read_task(packet_limit: bool, _display_name: String, partner: ConnectionHandle, self_handle: ConnectionHandle, player_id: SlotId, version: i32, sync_data: Option<Arc<PlayerSyncData>>) {
    let mut read_buf = Vec::new();
    let mut protocol_buf = Vec::new();
    let mut read = self_handle.reader.lock().await;
    let mut decryption = self_handle.decryption.lock().await;
    let sync_data = sync_data.unwrap();
    let mut packet_per_second = 0usize;
    let mut last_second = SystemTime::now();
    let mut should_forward = true;
    loop {
        let res = read_and_decode_packet(read.deref_mut(), &mut read_buf, &mut protocol_buf, self_handle.compression_threshold, decryption.deref_mut()).await;
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
        let res = ClientPacketHandler::handle_packet(packet_id, &read_buf[VarInt::get_size(packet_id)..], version, player_id, &self_handle, &sync_data).await;
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
    sync_data: Option<Arc<PlayerSyncData>>,
    pub address: SocketAddr,
    pub(crate) closed: Arc<AtomicBool>,

}

impl Display for ConnectionHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", format!("[{}, {:?}, {:?}]", self.name, self.protocol_state, self.protocol_state))
    }
}

impl ConnectionHandle {
    #[inline]
    pub(crate) fn new(name: String, sender: Sender<PacketSending>, reader: OwnedReadHalf, protocol_state: ProtocolState, write_task: AbortHandle,
                      compression_threshold: i32, decryption: Option<PacketDecryption>, sync_data: Option<Arc<PlayerSyncData>>, address: SocketAddr) -> Self {
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
            sync_data,
            address,
            closed: Arc::new(AtomicBool::new(false)),
        }
    }

    #[inline]
    pub fn protocol_state(&self) -> ProtocolState {
        unsafe { core::mem::transmute(self.protocol_state.load(Ordering::Relaxed)) }
    }

    #[inline]
    pub fn set_protocol_state(&self, state: ProtocolState) {
        self.protocol_state.store(state as u8, Ordering::Relaxed);
    }

    #[inline]
    pub(crate) async fn spawn_read_task(&self, packet_limiter: bool, display_name: String, partner: ConnectionHandle, player_id: SlotId, version: i32) {
        let mut old_read_task = self.read_task.lock().await;
        if old_read_task.is_some() {
            panic!("Read task already running!");
        }
        let read_task = tokio::spawn(read_task(packet_limiter, display_name, partner, self.clone(), player_id, version, self.sync_data.clone()));
        old_read_task.replace(read_task.abort_handle());
    }

    #[inline]
    pub async fn sync(&self) -> IOResult<()> {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        self.sender.send(PacketSending::Sync(sender)).await.map_err(|_| IOError::new(std::io::ErrorKind::Other, "Failed to queue sync packet!"))?;
        receiver.await.map_err(|_| IOError::new(std::io::ErrorKind::Other, "Failed to receive sync packet!"))
    }

    #[inline]
    pub async fn queue_packet(&self, packet: Vec<u8>, bypass_drop: bool) -> IOResult<()> {
        self.sender.send(PacketSending::Packet(packet, bypass_drop)).await.map_err(|_| IOError::new(std::io::ErrorKind::Other, "Failed to queue packet!"))
    }

    #[inline]
    pub async fn drop_redundant(&self, drop: bool) -> IOResult<()> {
        self.sender.send(PacketSending::DropRedundant(drop)).await.map_err(|_| IOError::new(std::io::ErrorKind::Other, "Failed to queue drop redundant packet!"))
    }

    #[inline]
    pub async fn on_bundle(&self) -> IOResult<()> {
        self.sender.send(PacketSending::BundleReceived).await.map_err(|_| IOError::new(std::io::ErrorKind::Other, "Failed to queue bundle received packet!"))
    }

    #[inline]
    pub async fn goto_config(&self, version: i32) -> IOResult<()> {
        self.sender.send(PacketSending::StartConfig(version)).await.map_err(|_| IOError::new(std::io::ErrorKind::Other, "Failed to queue start config packet!"))
    }

    #[inline]
    pub async fn disconnect(&self, reason: &str) {
        if self.closed.load(Ordering::Relaxed) {
            log::debug!("{} disconnected twice: {}", self.name, reason);
            return;
        }
        self.closed.swap(true, Ordering::Relaxed);
        log::info!("{} disconnected: {}", self.name, reason);
        self.write_task.abort();
        if let Some(task) = self.read_task.lock().await.take() {
            task.abort();
        }
    }

    #[inline]
    pub async fn wait_for_disconnect(&self) {
        let _ = self.disconnect_wait.read().await;
    }
}
