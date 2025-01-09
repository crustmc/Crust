use std::{
    future::Future,
    io::Cursor,
    net::{IpAddr, SocketAddr},
    ops::DerefMut,
    pin::Pin,
};

use rand::RngCore;
use rsa::{Pkcs1v15Encrypt, RsaPublicKey};
use tokio::net::{TcpStream, ToSocketAddrs};
use uuid::Uuid;

use crate::{
    auth::GameProfile,
    chat::Text,
    server::{
        encryption::{PacketDecryption, PacketEncryption},
        packets::{EncryptionResponse, Kick, Packet},
    },
    util::{IOError, IOErrorKind, VarInt, WeakHandle},
    version::R1_20_2,
};

use self::packets::{LoginAcknowledged, LoginDisconnect, SetCompression};

use super::{
    packet_handler::ServerPacketHandler, packet_ids::{PacketRegistry, ServerPacketType}, packets::{
        self, encode_and_send_packet, read_and_decode_packet, CookieRequest, CookieResponse,
        EncryptionRequest, Handshake, LoginPluginRequest, LoginPluginResponse, LoginRequest,
        LoginSuccess, PlayerPublicKey, ProtocolState, PROTOCOL_STATE_LOGIN,
    }, proxy_handler::{ClientHandle, ConnectionHandle, PacketSending}, ProxiedPlayer, ProxyServer, SlotId
};

#[derive(Debug)]
pub enum ConnectError {
    SocketConnectError(IOError),
    IO(IOError),
    Kicked(String),
    ServerInOnlineMode,
    InvalidPublicKeyFormat,
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::SocketConnectError(e) => write!(f, "Failed to connect to server: {}", e),
            Self::IO(e) => write!(f, "IO error occurred in login state: {}", e),
            Self::Kicked(reason) => write!(f, "Kicked: {}", reason),
            Self::ServerInOnlineMode => write!(f, "Server is in online mode"),
            Self::InvalidPublicKeyFormat => write!(f, "Invalid public key format"),
        }
    }
}

impl std::error::Error for ConnectError {}

pub struct EstablishedBackend {
    profile: GameProfile,
    stream: TcpStream,
    compression_threshold: i32,
    version: i32,
    encryption: Option<(PacketEncryption, PacketDecryption)>,
    address: SocketAddr,
}

impl EstablishedBackend {
    pub async fn begin_proxying(
        self,
        server_name: &str,
        partner: ClientHandle,
    ) -> (GameProfile, ConnectionHandle) {
        let player_name = self.profile.name.clone();
        let Self {
            profile,
            stream,
            compression_threshold,
            encryption,
            address,
            ..
        } = self;
        let synced_protocol_state = partner.connection.protocol_state.clone();
        let (read, mut write) = stream.into_split();
        let player = partner.player.clone();
        let version = partner.version;

        let partner_handle = partner.connection.clone();
        let (mut encryption, decryption) = match encryption {
            Some(encryption) => (Some(encryption.0), Some(encryption.1)),
            None => (None, None),
        };

        let (handle_sender, handle_receiver) = tokio::sync::oneshot::channel::<ConnectionHandle>();
        let player_ = player.clone();
        let read_task = tokio::spawn(async move {
            let self_handle = handle_receiver.await.unwrap();
            let mut protocol_buf = Vec::new();
            let mut read = self_handle.reader.lock().await;
            let mut decryption = self_handle.decryption.lock().await;
            loop {
                let mut read_buf = Vec::new();
                let res = read_and_decode_packet(
                    read.deref_mut(),
                    &mut read_buf,
                    &mut protocol_buf,
                    compression_threshold,
                    &mut decryption,
                ).await;

                if let Err(e) = res {
                    self_handle.disconnect(&e.to_string()).await;
                    break;
                }

                let packet_id = VarInt::decode_simple(&mut Cursor::new(&read_buf));
                if let Err(e) = packet_id {
                    self_handle.disconnect(&e.to_string()).await;
                    break;
                }
                let packet_id = packet_id.unwrap().get();

                let res = ServerPacketHandler::handle_packet(
                    packet_id,
                    &read_buf[VarInt::get_size(packet_id)..],
                    version,
                    &player_,
                    &self_handle,
                    &partner.connection,
                )
                    .await;
                if let Err(e) = &res {
                    self_handle.disconnect(&e.to_string()).await;
                    break;
                }
                if res.unwrap() {
                    if let Err(e) = partner.connection.queue_packet(read_buf, false).await {
                        // TODO: handle when client is disconnected
                        self_handle.disconnect(&e.to_string()).await;
                        break;
                    }
                }
            }
        });

        let (sender, mut receiver) = tokio::sync::mpsc::channel(256);

        let write_task = tokio::spawn(async move {
            let mut protocol_buf = Vec::new();
            while let Some(event) = receiver.recv().await {
                match event {
                    PacketSending::Packet(packet, bypass) => {
                        if encode_and_send_packet(
                            &mut write,
                            &packet,
                            &mut protocol_buf,
                            compression_threshold,
                            &mut encryption,
                        )
                            .await
                            .is_err()
                        {
                            // could not forward packet to player, he disconnected
                            break;
                        }
                    }
                    PacketSending::Sync(sender) => {
                        let _ = sender.send(());
                    }
                    _ => {}
                }
            }
        });

        let mut handle = ConnectionHandle::new(
            format!("[{}] <-> [{}]", server_name, player_name),
            sender,
            read,
            ProtocolState::Config,
            write_task.abort_handle(),
            compression_threshold,
            decryption,
            address,
        );

        log::info!("[{}] <-> [{}]: connected", player_name, server_name);

        let disconnect_lock = handle.disconnect_wait.clone();

        handle.protocol_state = synced_protocol_state; // synchronize protocol state
        handle
            .read_task
            .lock()
            .await
            .replace(read_task.abort_handle());

        if handle_sender.send(handle.clone()).is_err() {
            panic!("Failed to send connection handle");
        }

        let handle_ = handle.clone();
        tokio::spawn(async move {
            let disconnect_guard = disconnect_lock.write().await;
            let _ = write_task.await;
            drop(disconnect_guard);
            let servers = ProxyServer::instance().servers().read().await;
            for server_name in servers.get_priorities() {
                let server = servers.get_server_id_by_name(&server_name);
                if let Some(server) = server {
                    if switch_server_helper(player.clone(), server).await {
                        return;
                    }
                }
            }

            let mut buf = vec![];
            packets::get_full_server_packet_buf_write_buffer(
                &mut buf,
                &Kick {
                    text: Text::new("§cYou have been kicked, no fallback server found."),
                },
                self.version,
                handle_.protocol_state(),
            )
                .unwrap();
            partner_handle.queue_packet(buf, true).await.ok();
            partner_handle.sync().await.ok();
            partner_handle.disconnect("no fallback server found").await;
        });
        (profile, handle)
    }
}

fn switch_server_helper(
    player: WeakHandle<ProxiedPlayer>,
    server: SlotId,
) -> Pin<Box<dyn Future<Output=bool> + Send>> {
    let block = async move {
        if let Some(player) = player.upgrade() {
            let switched = ProxiedPlayer::switch_server(player, server).await;
            if let Some(success) = switched {
                let success = success.await;
                if let Ok(success) = success {
                    if success {
                        return true;
                    }
                }
            } else {
                return true;
            }
        } else {
            return true;
        }
        false
    };
    Box::pin(block)
}

fn sanitize_address(addr: SocketAddr) -> std::io::Result<String> {
    // Ensure the address is resolved (for this example, we assume the address is already resolved)
    if addr.ip().is_unspecified() {
        return Err(IOError::new(IOErrorKind::Other, "Unresolved address"));
    }

    // Get the string representation of the address
    let address_str = addr.ip().to_string();

    // Check if the address is IPv6 and remove the scope if present
    if let IpAddr::V6(_) = addr.ip() {
        if let Some(percent_idx) = address_str.find('%') {
            return Ok(address_str[..percent_idx].to_string());
        }
    }

    // Return the address as-is for IPv4 or if no scope is found in IPv6
    Ok(address_str)
}

pub async fn connect<A: ToSocketAddrs>(
    client_ip: SocketAddr,
    addr: A,
    hs_host: String,
    hs_port: u16,
    mut profile: GameProfile,
    player_public_key: Option<PlayerPublicKey>,
    version: i32,
) -> Result<EstablishedBackend, ConnectError> {
    let mut stream = TcpStream::connect(addr)
        .await
        .map_err(ConnectError::SocketConnectError)?;
    let address = stream.peer_addr().map_err(ConnectError::IO)?;

    let mut write_buf = Vec::new();
    let mut protocol_buf = Vec::new();
    let mut compression_threshold = -1;
    let mut encryption = None;
    let mut decryption = None;

    let mut host = hs_host;
    if ProxyServer::instance().config().spigot_forward {
        host = format!(
            "{}\0{}\0{}",
            &host,
            sanitize_address(client_ip).map_err(ConnectError::IO)?,
            profile.id
        );
        if profile.properties.len() > 0 {
            host = format!(
                "{}\0{}",
                &host,
                serde_json::to_string(&profile.properties).unwrap()
            );
        }
    };

    packets::get_full_client_packet_buf_write_buffer(
        &mut write_buf,
        &Handshake {
            version,
            host,
            port: hs_port,
            next_state: PROTOCOL_STATE_LOGIN,
        },
        version,
        ProtocolState::Handshake,
    )
        .unwrap();
    encode_and_send_packet(
        &mut stream,
        &write_buf,
        &mut protocol_buf,
        compression_threshold,
        &mut encryption,
    )
        .await
        .map_err(ConnectError::IO)?;

    packets::get_full_client_packet_buf_write_buffer(
        &mut write_buf,
        &LoginRequest {
            name: profile.name.clone(),
            uuid: Some(Uuid::parse_str(&profile.id).unwrap()),
            public_key: player_public_key,
        },
        version,
        ProtocolState::Login,
    )
        .unwrap();
    encode_and_send_packet(
        &mut stream,
        &write_buf,
        &mut protocol_buf,
        compression_threshold,
        &mut encryption,
    )
        .await
        .map_err(ConnectError::IO)?;

    let mut read_buf = Vec::new();
    loop {
        read_and_decode_packet(
            &mut stream,
            &mut read_buf,
            &mut protocol_buf,
            compression_threshold,
            &mut decryption,
        )
            .await
            .map_err(ConnectError::IO)?;
        let mut reader = Cursor::new(&read_buf);
        let packet_id = VarInt::decode_simple(&mut reader)
            .map_err(ConnectError::IO)?
            .get();
        write_buf.clear();
        let packet_type = PacketRegistry::instance().get_server_packet_type(
            ProtocolState::Login,
            version,
            packet_id,
        );
        if let Some(packet_type) = packet_type {
            match packet_type {
                ServerPacketType::LoginDisconnect => {
                    let disconnect =
                        LoginDisconnect::decode(&mut reader, version).map_err(ConnectError::IO)?;
                    return Err(ConnectError::Kicked(disconnect.text.get_string()));
                }
                ServerPacketType::EncryptionRequest => {
                    let enc_request = EncryptionRequest::decode(&mut reader, version)
                        .map_err(ConnectError::IO)?;
                    if enc_request.should_authenticate {
                        return Err(ConnectError::ServerInOnlineMode);
                    }

                    use rsa::pkcs8::DecodePublicKey;

                    let public_key = RsaPublicKey::from_public_key_der(&enc_request.public_key)
                        .map_err(|_| ConnectError::InvalidPublicKeyFormat)?;

                    let mut secret_key = [0u8; 16];
                    rand::thread_rng().fill_bytes(&mut secret_key);

                    let encrypted_secret_key = public_key
                        .encrypt(&mut rand::thread_rng(), Pkcs1v15Encrypt, &secret_key)
                        .unwrap();
                    let encrypted_verify_token = public_key
                        .encrypt(
                            &mut rand::thread_rng(),
                            Pkcs1v15Encrypt,
                            &enc_request.verify_token,
                        )
                        .unwrap();

                    VarInt(1)
                        .encode(&mut write_buf, 5)
                        .map_err(ConnectError::IO)?; // Encryption response packet id
                    packets::get_full_client_packet_buf_write_buffer(
                        &mut write_buf,
                        &EncryptionResponse {
                            shared_secret: encrypted_secret_key,
                            verify_token: Some(encrypted_verify_token),
                            encryption_data: None,
                        },
                        version,
                        ProtocolState::Login,
                    )
                        .unwrap();
                    encode_and_send_packet(
                        &mut stream,
                        &write_buf,
                        &mut protocol_buf,
                        compression_threshold,
                        &mut encryption,
                    )
                        .await
                        .map_err(ConnectError::IO)?;

                    encryption = Some(PacketEncryption::new(&secret_key));
                    decryption = Some(PacketDecryption::new(&secret_key));
                }
                ServerPacketType::LoginSuccess => {
                    let login_success =
                        LoginSuccess::decode(&mut reader, version).map_err(ConnectError::IO)?;
                    profile = login_success.profile;
                    if version >= R1_20_2 {
                        packets::get_full_client_packet_buf_write_buffer(
                            &mut write_buf,
                            &LoginAcknowledged,
                            version,
                            ProtocolState::Login,
                        )
                            .unwrap();
                        encode_and_send_packet(
                            &mut stream,
                            &write_buf,
                            &mut protocol_buf,
                            compression_threshold,
                            &mut encryption,
                        )
                            .await
                            .map_err(ConnectError::IO)?;
                    }
                    break;
                }
                ServerPacketType::SetCompression => {
                    compression_threshold = SetCompression::decode(&mut reader, version)
                        .map_err(ConnectError::IO)?
                        .compression;
                }
                ServerPacketType::LoginPluginRequest => {
                    let payload = LoginPluginRequest::decode(&mut reader, version)
                        .map_err(ConnectError::IO)?;
                    packets::get_full_client_packet_buf_write_buffer(
                        &mut write_buf,
                        &LoginPluginResponse {
                            id: payload.id,
                            data: None,
                        },
                        version,
                        ProtocolState::Login,
                    )
                        .unwrap();
                    encode_and_send_packet(
                        &mut stream,
                        &write_buf,
                        &mut protocol_buf,
                        compression_threshold,
                        &mut encryption,
                    )
                        .await
                        .map_err(ConnectError::IO)?;
                }
                ServerPacketType::CookieRequest => {
                    let cookie_request =
                        CookieRequest::decode(&mut reader, version).map_err(ConnectError::IO)?;
                    packets::get_full_client_packet_buf_write_buffer(
                        &mut write_buf,
                        &CookieResponse {
                            cookie: cookie_request.cookie,
                            data: None,
                        },
                        version,
                        ProtocolState::Login,
                    )
                        .unwrap();
                    encode_and_send_packet(
                        &mut stream,
                        &write_buf,
                        &mut protocol_buf,
                        compression_threshold,
                        &mut encryption,
                    )
                        .await
                        .map_err(ConnectError::IO)?;
                }
                _ => {}
            }
        }
    }

    let encryption = match encryption {
        Some(encryption) => Some((encryption, decryption.unwrap())),
        None => None,
    };

    Ok(EstablishedBackend {
        profile,
        stream,
        compression_threshold,
        version,
        encryption,
        address,
    })
}
