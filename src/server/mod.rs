use std::{collections::HashMap, future::Future, io::Cursor, path::{Path, PathBuf}, sync::atomic::Ordering, time::{Duration, Instant}};
use base64::Engine;
use command::{CommandRegistry, CommandRegistryBuilder};
use image::{imageops::FilterType, ImageFormat};
use log::{error, info, warn};
use packets::{PlayerPublicKey, ProtocolState, SystemChatMessage};
use proxy_handler::{ClientHandle, ConnectionHandle, PlayerSyncData};
use rsa::{RsaPrivateKey, RsaPublicKey};
use serde::{Deserialize, Serialize};
use slotmap::{DefaultKey, SlotMap};
use tokio::{net::TcpListener, runtime::Runtime, sync::RwLock, task::JoinHandle};
use crate::{auth::GameProfile, chat::Text, hash_map, plugin::PluginManager, util::{Handle, IOResult}};
use crate::util::WeakHandle;

pub(crate) mod backend;
pub(crate) mod brigadier;
pub(crate) mod compression;
pub(crate) mod encryption;
pub(crate) mod initial_handler;
pub(crate) mod packets;
pub(crate) mod packet_handler;
pub(crate) mod packet_ids;
pub(crate) mod proxy_handler;
pub(crate) mod status;
pub(crate) mod command;
pub(crate) mod nbt;

pub const NAME: &str = "Crust";
pub const GIT_COMMIT_ID: &str = env!("GIT_COMMIT");
pub const JENKINS_BUILD_NUMBER: &str = env!("BUILD_NUMBER");
pub const FULL_NAME: &str = env!("FULL_NAME");

#[derive(Debug, Serialize, Deserialize)]
pub struct ProxyConfig {
    pub bind_address: String,
    pub worker_threads: usize,
    pub compression_threshold: i32,
    pub motd: String,
    pub favicon: Option<PathBuf>,
    pub connection_throttle_time: i32,
    pub connection_throttle_limit: u8,
    pub max_players: i32,
    pub online_mode: bool,
    pub offline_mode_encryption: bool,
    pub prevent_proxy_connections: bool,
    pub servers: Vec<ServerConfig>,
    pub spigot_forward: bool,
    pub priorities: Vec<String>,
    pub max_packet_per_second: i32,
    pub restrict_tab_completes: bool,
    pub proxy_protocol: bool,
    pub groups: HashMap<String, Vec<String>>,
    pub users: HashMap<String, Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerConfig {
    pub label: String,
    pub address: String,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0:25577".to_owned(),
            worker_threads: 0,
            compression_threshold: 256,
            motd: "A Rust Minecraft Proxy".to_owned(),
            connection_throttle_time: 5000,
            connection_throttle_limit: 20,
            favicon: None,
            max_players: 100,
            online_mode: false,
            offline_mode_encryption: false,
            prevent_proxy_connections: false,
            spigot_forward: true,
            restrict_tab_completes: true,
            servers: vec![
                ServerConfig {
                    label: "lobby".to_owned(),
                    address: "127.0.0.1:25565".to_owned(),
                }
            ],
            priorities: vec!["lobby".to_owned()],
            max_packet_per_second: 2000,
            proxy_protocol: false,
            groups: hash_map! {
                "admin".to_owned() => vec!["crust.command.end".to_owned(), "crust.command.gkick".to_owned(), "crust.command.server".to_owned()],
                "default".to_owned() => vec!["crust.command.server".to_owned()]
            },
            users: hash_map!("Outfluencer".to_owned() => vec!["admin".to_owned()]),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ServerInfo {
    pub label: String,
    pub address: String,
}

pub struct ServerList {
    priorities: Vec<String>,
    servers: SlotMap<SlotId, ServerInfo>,
    servers_by_name: HashMap<String, SlotId>,
}

impl ServerList {
    pub fn get_priorities(&self) -> &[String] {
        &self.priorities
    }

    pub fn all_servers(&self) -> impl Iterator<Item=(SlotId, &ServerInfo)> {
        self.servers.iter()
    }

    pub fn get_server_id_by_name(&self, label: &str) -> Option<SlotId> {
        self.servers_by_name.get(label).copied()
    }

    pub fn get_server_by_name(&self, label: &str) -> Option<&ServerInfo> {
        self.servers_by_name.get(label).map(|id| self.servers.get(*id).unwrap())
    }

    pub fn get_server(&self, id: SlotId) -> Option<&ServerInfo> {
        self.servers.get(id)
    }

    pub fn add_server(&mut self, server: ServerInfo) -> SlotId {
        let label = server.label.clone();
        let id = self.servers.insert(server);
        self.servers_by_name.insert(label, id);
        id
    }

    pub fn remove_server_by_name(&mut self, label: &str) -> bool {
        if let Some(id) = self.servers_by_name.remove(label) {
            self.servers.remove(id);
            return true;
        }
        false
    }

    pub fn remove_server(&mut self, id: SlotId) -> bool {
        if let Some(server) = self.servers.get(id) {
            self.servers_by_name.remove(&server.label);
            self.servers.remove(id);
            return true;
        }
        false
    }

    pub fn list_servers(&self) -> impl Iterator<Item=&ServerInfo> {
        self.servers.values()
    }
}

pub type SlotId = DefaultKey;

pub struct ProxyServer {
    runtime: Runtime,
    config: ProxyConfig,
    command_registry: CommandRegistry,
    servers: RwLock<ServerList>,
    rsa_priv_key: RsaPrivateKey,
    rsa_pub_key: RsaPublicKey,
    players: RwLock<SlotMap<SlotId, Handle<ProxiedPlayer>>>,
    pub player_count: usize,
    favicon: Option<String>,
}

static mut INSTANCE: Option<ProxyServer> = None;

impl ProxyServer {
    
    pub fn config(&self) -> &ProxyConfig {
        &self.config
    }
    
    pub fn command_registry(&self) -> &CommandRegistry {
        &self.command_registry
    }
    
    pub fn servers(&self) -> &RwLock<ServerList> {
        &self.servers
    }
    
    pub fn players(&self) -> &RwLock<SlotMap<SlotId, Handle<ProxiedPlayer>>> {
        &self.players
    }
    
    pub async fn get_player_by_name(&self, name: &str) -> Option<WeakHandle<ProxiedPlayer>> {
        let lock = self.players.read().await;
        let player = lock.iter().find( |(_, player_ref)| player_ref.profile.name.eq_ignore_ascii_case(name));
        if let Some((_, player_ref)) = player {
            Some(player_ref.downgrade())
        } else {
            None
        }
    }

    pub fn get_player_by_name_blocking(&self, name: &str) -> Option<WeakHandle<ProxiedPlayer>> {
        let lock = self.players.blocking_read();
        let player = lock.iter().find( |(_, player_ref)| player_ref.profile.name.eq_ignore_ascii_case(name));
        if let Some((_, player_ref)) = player {
            Some(player_ref.downgrade())
        } else {
            None
        }
    }
    
    pub fn rsa_private_key(&self) -> &RsaPrivateKey {
        &self.rsa_priv_key
    }
    
    pub fn rsa_public_key(&self) -> &RsaPublicKey {
        &self.rsa_pub_key
    }
    
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }
    
    pub fn block_on<F: Future<Output = T>, T>(&self, future: F) -> T {
        self.runtime.block_on(future)
    }
    
    pub fn spawn_task<F: Future<Output = T> + Send + 'static, T: Send + 'static>(&self, future: F) -> JoinHandle<T> {
        self.runtime.spawn(future)
    }

    pub fn instance() -> &'static Self {
        unsafe {
            match INSTANCE {
                Some(ref instance) => instance,
                None => panic!("ProxyServer instance not initialized"),
            }
        }
    }
}

pub fn run_server() {
    info!("Starting {}..", FULL_NAME);
    let config_path = Path::new("config.json");
    let config = if !config_path.exists() {
        let default_config = ProxyConfig::default();
        let default_config_json = serde_json::to_string_pretty(&default_config).unwrap();
        if let Err(e) = std::fs::write("config.json", default_config_json) {
            log::error!("Failed to write default config: {}", e);
        }
        default_config
    } else {
        match std::fs::read("config.json") {
            Ok(json) => {
                match serde_json::from_slice(&json) {
                    Ok(config) => config,
                    Err(e) => {
                        log::error!("Failed to parse config: {}", e);
                        return;
                    }
                }
            }
            Err(e) => {
                log::error!("Failed to read config: {}", e);
                return;
            }
        }
    };

    let icon_path = config.favicon.as_ref();
    let icon = if let Some(icon_path) = icon_path {
        if icon_path.is_file() {
            match image::open(icon_path) {
                Ok(mut image) => {
                    if image.width() != 64 || image.height() != 64 {
                        image = image.resize_exact(64, 64, FilterType::Lanczos3);
                    }
                    let mut png_bytes = Vec::new();
                    if let Err(e) = image.write_to(&mut Cursor::new(&mut png_bytes), ImageFormat::Png) {
                        warn!("Failed to encode favicon: {}", e);
                        None
                    } else {
                        let base64 = String::from("data:image/png;base64,") + &base64::engine::general_purpose::STANDARD.encode(&png_bytes);
                        Some(base64)
                    }
                }
                Err(e) => {
                    log::error!("Failed to load favicon: {}", e);
                    None
                }
            }
        } else {
            log::error!("Favicon path is not a valid file! Skipping icon...");
            None
        }
    } else { None };
    

    info!("Loaded proxy config.");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .worker_threads(match config.worker_threads {
            0 => num_cpus::get(),
            n => n,
        })
        .build();
    if let Err(e) = runtime {
        log::error!("Failed to create runtime: {}", e);
        return;
    }
    let runtime = runtime.unwrap();
    info!("Started runtime with {} worker threads.", runtime.metrics().num_workers());

    let priv_key = RsaPrivateKey::new(&mut rand::thread_rng(), 1024);
    if let Err(e) = priv_key {
        error!("Failed to generate RSA key pair: {}", e);
        return;
    }
    let priv_key = priv_key.unwrap();
    let pub_key = RsaPublicKey::from(&priv_key);

    let mut server_list = ServerList {
        priorities: config.priorities.clone(),
        servers: SlotMap::new(),
        servers_by_name: HashMap::new(),
    };
    for entry in &config.servers {
        server_list.add_server(ServerInfo {
            label: entry.label.clone(),
            address: entry.address.clone(),
        });
    }

    let commands = command::core_impl::register_all(CommandRegistryBuilder::new());

    unsafe {
        INSTANCE = Some(ProxyServer {
            runtime,
            command_registry: commands.build(),
            rsa_priv_key: priv_key,
            rsa_pub_key: pub_key,
            servers: RwLock::new(server_list),
            player_count: 0,
            players: RwLock::new(SlotMap::new()),
            config,
            favicon: icon,
        });
    }
    
    ProxyServer::instance().block_on(async move {
        if !PluginManager::load_plugins() {
            log::error!("Error while loading plugins, shutting down.");
            return;
        }
    });
    
    ProxyServer::instance().spawn_task(async move {
        let listener = TcpListener::bind(&ProxyServer::instance().config.bind_address).await.unwrap();

        info!("Listening on {}", listener.local_addr().unwrap());
        let mut map = HashMap::new();
        let mut time = Instant::now();
        let connection_throttle = ProxyServer::instance().config.connection_throttle_time > 0;
        let interval = Duration::from_millis(ProxyServer::instance().config().connection_throttle_time as u64);
        let limit = ProxyServer::instance().config().connection_throttle_limit;
        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    if connection_throttle {
                        let counter = map.entry(peer_addr.ip()).or_insert(0u8);
                        *counter += 1;

                        let now = Instant::now();
                        let mut clear = false;
                        if now.duration_since(time) >= interval {
                            *counter = 0;
                            time = now;
                            clear = true;
                        }

                        if *counter > limit {
                            continue;
                        }
                        if clear {
                            map.clear();
                        }
                    }
                    initial_handler::handle(stream, peer_addr).await;
                }
                Err(err) => {
                    // probably out of file descriptors
                    log::debug!("Failed to accept connection: {}", err);
                }
            }
        }
    });
}

pub struct ProxiedPlayer {
    pub player_id: SlotId,
    pub profile: GameProfile,
    pub player_public_key: Option<PlayerPublicKey>,
    pub current_server: SlotId,
    pub client_handle: ConnectionHandle,
    pub server_handle: Option<ConnectionHandle>,
    pub protocol_version: i32,
    pub(crate) sync_data: PlayerSyncData,
}

impl ProxiedPlayer {

    pub fn has_permission(&self, perm: &str) -> bool {
        let mut groups = ProxyServer::instance().config.users.get(&self.profile.name);
        if groups.is_none() {
            let uuid = &self.profile.id.replace("-", "").to_string();
            groups = ProxyServer::instance().config.users.get(uuid);
        }
        if let Some(groups) = groups {
            for group in groups {
                let perms = ProxyServer::instance().config.groups.get(group);
                if let Some(perms) = perms {
                    if perms.contains(&perm.to_string()) {
                        return true;
                    }
                } else {
                    error!("Group {} is not configured, but used by {}", group, &self.profile.name);
                }
            }
        }

        if let Some(perms) = ProxyServer::instance().config.groups.get("default") {
            if perms.contains(&perm.to_string()) {
                return true;
            }
        }
        // todo call permission event
        false
    }

    pub async fn send_message(&self, message: Text) -> IOResult<()> {
        let chat = SystemChatMessage {
            message,
            pos: 0,
        };
        let data = packets::get_full_server_packet_buf(&chat, self.protocol_version, self.client_handle.protocol_state())?;
        if let Some(data) = data {
            return self.client_handle.queue_packet(data, false).await;
        } else {
            println!("packet not in current state");
        }
        Ok(())
    }
    
    pub async fn kick<T: Into<Text>>(&self, text: T) -> IOResult<()> {
        let kick_packet = packets::Kick { text: text.into() };
        let data = packets::get_full_server_packet_buf(&kick_packet, self.protocol_version, self.client_handle.protocol_state())?;
        if let Some(data) = data {
            self.client_handle.queue_packet(data, true).await?;
            self.client_handle.sync().await?;
        }
        self.client_handle.disconnect("player kicked").await;
        Ok(())
    }

    pub async fn switch_server(mut player: Handle<ProxiedPlayer>, server_id: SlotId) -> Option<JoinHandle<bool>> {
        if player.client_handle.closed.load(Ordering::Relaxed) {
            return None;
        }

        if let Err(true) = player.sync_data.is_switching_server.compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed) {
            return None;
        }
        let version = player.protocol_version;
        let join_handle = tokio::spawn(async move {
            if player.client_handle.closed.load(Ordering::Relaxed) {
                return false;
            }
            
            let (addr, server_name) = {
                let server_list = ProxyServer::instance().servers().read().await;
                let server = server_list.get_server(server_id);
                if server.is_none() {
                    player.sync_data.is_switching_server.store(false, Ordering::Relaxed);
                    return false;
                }
                let server = server.unwrap();
                (server.address.clone(), server.label.clone())
            };

            let username = player.profile.name.clone();
            let backend = backend::connect(
                player.client_handle.address, addr, "127.0.0.1".to_string(), 25565,
                player.profile.clone(), player.player_public_key.clone(), version
            ).await;
            if let Err(e) = backend {
                log::error!("[{}] Failed to connect to backend: {}", username, e);
                player.sync_data.is_switching_server.store(false, Ordering::Relaxed);
                let _ = player.send_message(Text::new(format!("§cCould not connect: {}", e))).await;
                return false;
            }
            let backend = backend.unwrap();

            if let ProtocolState::Game = player.client_handle.protocol_state() {
                let _ = player.client_handle.drop_redundant(true).await;
                if let Some(ref server_handle) = player.server_handle {
                    server_handle.disconnect("client is switching servers").await;
                    server_handle.wait_for_disconnect().await;
                }

                let _ = player.client_handle.goto_config(version).await;
                player.sync_data.config_ack_notify.notified().await;

                let _ = player.client_handle.drop_redundant(false).await;
            } else {
                warn!("Player {} is not in game state, cancelling server switch.", username);
                player.sync_data.is_switching_server.store(false, Ordering::Relaxed);
                return false;
            }

            if let Some(read_task) = player.client_handle.read_task.lock().await.take() {
                read_task.abort();
            }

            let (profile, server_handle) = backend.begin_proxying(&server_name, ClientHandle {
                player: player.downgrade(),
                version,
                connection: player.client_handle.clone(),
            }).await;

            let settings = player.sync_data.client_settings.lock().await;

            if let Some(packet) = settings.as_ref() {
                if let Some(data) = packets::get_full_client_packet_buf(packet, version, player.client_handle.protocol_state()).unwrap() {
                    if let Err(_e) = server_handle.queue_packet(data, true).await {
                        drop(settings);
                        player.sync_data.is_switching_server.store(false, Ordering::Relaxed);
                        return false;
                    }
                }
            }
            drop(settings);

            let display_name = format!("[{} - {}]", username, server_name);

            player.client_handle.spawn_read_task(false, display_name, server_handle.clone(), player.downgrade(), version).await;

            player.current_server = server_id;
            player.server_handle = Some(server_handle);
            player.profile = profile;
            player.sync_data.is_switching_server.store(false, Ordering::Relaxed);
            true
        });
        Some(join_handle)
    }
}
