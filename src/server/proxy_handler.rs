use core::sync;
use std::{io::Cursor, net::SocketAddr, ops::DerefMut, sync::{atomic::{AtomicBool, AtomicU8, Ordering}, Arc}, time::SystemTime};

use byteorder::WriteBytesExt;
use tokio::{net::{tcp::OwnedReadHalf, TcpStream}, sync::{mpsc::Sender, Mutex, Notify, RwLock}, task::AbortHandle};

use crate::{auth::GameProfile, chat::{Text, TextBuilder}, server::{encryption, initial_handler::send_login_disconnect, packet_ids::{PacketRegistry, ServerPacketType}, packets::{self, encode_and_send_packet, read_and_decode_packet, Kick}, ProxiedPlayer}, util::VarInt};

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
        encode_and_send_packet(&mut stream, &buf, &mut vec![], data.compression_threshold, &mut encryption).await.unwrap();
        return;
    }

    let (server_name, server_id, backend) = server_data.unwrap();
    log::info!("{} <-> [{}]", display_name, server_name);

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
    let (_backend_profile, backend_handle) = backend.begin_proxying(handle, player_sync_data).await;

    tokio::spawn(async move {
        let disconnect_guard = disconnect_lock.write().await;
        let _ = write_task.await;
        let mut lock = ProxyServer::instance().players().write().await;
        if let Some(player) = lock.remove(player_id) {
            *unsafe { core::mem::transmute::<_, &mut usize>(&ProxyServer::instance().player_count as *const usize) } -= 1;
            drop(lock);
            if let Some(ref backend_handle) = player.server_handle {
                backend_handle.disconnect().await;
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

async fn read_task(packet_limit: bool, display_name: String, partner: ConnectionHandle, self_handle: ConnectionHandle, player_id: SlotId, version: i32, sync_data: Option<Arc<PlayerSyncData>>) {
    let mut read_buf = Vec::new();
    let mut protocol_buf = Vec::new();
    let mut read = self_handle.reader.lock().await;
    let mut decryption = self_handle.decryption.lock().await;
    let sync_data = sync_data.unwrap();
    let mut packet_per_second = 0usize;
    let mut last_second = SystemTime::now();
    loop {
        let res = read_and_decode_packet(read.deref_mut(), &mut read_buf, &mut protocol_buf, self_handle.compression_threshold, decryption.deref_mut()).await;
        if let Err(_e) = res {
            partner.disconnect().await;
            self_handle.disconnect().await;
            break;
        }

        if packet_limit {
            packet_per_second += 1;
            if packet_per_second >= 2000 {
                if let Ok(elapsed) = last_second.elapsed() {
                    if elapsed.as_millis() < 1000 {
                        partner.disconnect().await;
                        self_handle.disconnect().await;
                        log::warn!("{} sent to many packets", display_name);
                        break;
                    }
                    last_second = SystemTime::now();
                    packet_per_second = 0;
                }
            }
        }

        let packet_id = VarInt::decode_simple(&mut Cursor::new(&read_buf));
        if let Err(_e) = packet_id {
            self_handle.disconnect().await;
            break;
        }
        let packet_id = packet_id.unwrap().get();
        let res = ClientPacketHandler::handle_packet(packet_id, &read_buf[VarInt::get_size(packet_id)..],
                                                     version, player_id, &self_handle, &sync_data).await;
        if let Err(_e) = res {
            partner.disconnect().await;
            self_handle.disconnect().await;
            break;
        }
        if res.unwrap() && !partner.queue_packet(read_buf, false).await {
            partner.disconnect().await;
            //break;
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
}


impl ConnectionHandle {
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

    pub(crate) async fn spawn_read_task(&self, packet_limiter: bool, display_name: String, partner: ConnectionHandle, player_id: SlotId, version: i32) {
        let mut old_read_task = self.read_task.lock().await;
        if old_read_task.is_some() {
            panic!("Read task already running!");
        }
        let read_task = tokio::spawn(read_task(packet_limiter, display_name, partner, self.clone(), player_id, version, self.sync_data.clone()));
        old_read_task.replace(read_task.abort_handle());
    }

    pub async fn sync(&self) {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let _ = self.sender.send(PacketSending::Sync(sender)).await;
        let _ = receiver.await;
    }

    pub async fn queue_packet(&self, packet: Vec<u8>, bypass_drop: bool) -> bool {
        self.sender.send(PacketSending::Packet(packet, bypass_drop)).await.is_ok()
    }

    pub async fn drop_redundant(&self, drop: bool) -> bool {
        self.sender.send(PacketSending::DropRedundant(drop)).await.is_ok()
    }
    pub async fn on_bundle(&self) -> bool {
        self.sender.send(PacketSending::BundleReceived).await.is_ok()
    }

    pub async fn goto_config(&self, version: i32) -> bool {
        self.sender.send(PacketSending::StartConfig(version)).await.is_ok()
    }

    pub async fn disconnect(&self) {
        self.write_task.abort();
        if let Some(task) = self.read_task.lock().await.take() {
            task.abort();
        }
    }

    pub async fn wait_for_disconnect(&self) {
        let _ = self.disconnect_wait.read().await;
    }
}
