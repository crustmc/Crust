use std::{io::Cursor, net::SocketAddr};

use byteorder::{WriteBytesExt, BE};
use rand::RngCore;
use rsa::{pkcs8::EncodePublicKey, Pkcs1v15Encrypt};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::TcpStream};

use crate::{auth::GameProfile, chat::{Text, TextContent}, server::{packet_ids::{ClientPacketType, PacketRegistry}, packets::{read_and_decode_packet, EncryptionResponse, Packet, ProtocolState}}, util::{EncodingHelper, IOError, IOErrorKind, IOResult, VarInt}};

use self::packets::SetCompression;

use super::{encryption::*, packets::{self, encode_and_send_packet, EncryptionRequest, Handshake, LoginDisconnect, LoginRequest, LoginSuccess, PROTOCOL_READ_TIMEOUT, PROTOCOL_STATE_LOGIN, PROTOCOL_STATE_STATUS, PROTOCOL_STATE_TRANSFER}, proxy_handler::ProxyingData, ProxyServer};

macro_rules! check_timeout {
    ($fut:expr) => {
        tokio::time::timeout(PROTOCOL_READ_TIMEOUT, $fut)
    };
}

struct WriteBuffers<'a> {
    write_buf: &'a mut Vec<u8>,
    protocol_buf: &'a mut Vec<u8>,
}

pub async fn handle(mut stream: TcpStream, peer_addr: SocketAddr) {
    tokio::spawn(async move {
        let proxying_data = {
            let mut buffer = Vec::new();
            let handshake = match handshaking(&mut stream, &mut buffer).await {
                Err(e) => {
                    log::debug!("[{}] Handshake state failed: {}", peer_addr, e);
                    return;
                }
                Ok(handshake) => handshake,
            };
            buffer.clear();

            match handshake.next_state {
                PROTOCOL_STATE_STATUS => {
                    if let Err(e) = handle_status(stream, handshake.version).await {
                        log::debug!("[{}] Status state failed: {}", peer_addr, e);
                    }
                    return;
                }
                PROTOCOL_STATE_LOGIN | PROTOCOL_STATE_TRANSFER => {
                    match handle_login(&mut stream, handshake, &mut buffer, peer_addr).await {
                        Ok(state) => state,
                        Err(e) => {
                            log::debug!("[{}] Login state failed: {}", peer_addr, e);
                            return;
                        }
                    }
                }
                _ => { // invalid state
                    return;
                }
            }
        };
        super::proxy_handler::handle(stream, proxying_data).await;
    });
}

async fn handle_status(mut stream: TcpStream, version: i32) -> IOResult<()> {
    let mut state = 1;
    let mut write_buf = Vec::new();
    loop {
        write_buf.clear();
        let length = VarInt::decode_async(&mut stream, 3).await?.get();
        if length != 1 && length != 9 {
            return Err(IOError::new(IOErrorKind::InvalidData, "Bad packet length"));
        }
        let id = stream.read_u8().await?;
        match id {
            0 => { // status request
                if state == 2 {
                    return Err(IOError::new(IOErrorKind::InvalidData, "Status request in ping state"));
                }
                state = 2;
                VarInt(0).encode(&mut write_buf, 5)?; // packet id
                EncodingHelper::write_string(&mut write_buf, &super::status::get_status_response(version).to_string())?; // response

                VarInt(write_buf.len() as i32).encode_async(&mut stream, 3).await?; // length
                stream.write_all(&write_buf).await?; // content
            }
            1 => { // ping
                if state == 1 {
                    return Err(IOError::new(IOErrorKind::InvalidData, "Ping request in status state"));
                }
                let time = stream.read_u64().await?;
                VarInt(1).encode(&mut write_buf, 5)?; // packet id
                WriteBytesExt::write_u64::<BE>(&mut write_buf, time)?; // time

                VarInt(write_buf.len() as i32).encode_async(&mut stream, 3).await?; // length
                stream.write_all(&write_buf).await?; // content
                break;
            }
            _ => {
                return Err(IOError::new(IOErrorKind::InvalidData, "Bad status packet id"));
            }
        }
    }
    Ok(())
}

async fn handshaking(stream: &mut TcpStream, buffer: &mut Vec<u8>) -> IOResult<Handshake> {
    let handshake_length = check_timeout!(VarInt::decode_async(stream, 3)).await??.get();
    if handshake_length < 6 || handshake_length > 1000 {
        return Err(IOError::new(IOErrorKind::InvalidData, "Bad handshake length"));
    }
    buffer.resize(handshake_length as usize, 0);
    check_timeout!(stream.read_exact(buffer)).await??;
    let mut reader = Cursor::new(&*buffer);

    let id = VarInt::decode(&mut reader, 5)?.get();
    if id != 0 {
        return Err(IOError::new(IOErrorKind::InvalidData, "Bad handshake packet id"));
    }
    let handshake = Handshake::decode(&mut reader, 0)?;

    if handshake.next_state == PROTOCOL_STATE_LOGIN || handshake.next_state == PROTOCOL_STATE_TRANSFER {
        if !crate::version::is_supported(handshake.version) {
            send_login_disconnect(stream, buffer, Text::new(TextContent::literal("§cUnsupported protocol version".into())), handshake.version, -1, &mut None).await.ok();
            return Err(IOError::new(IOErrorKind::InvalidData, "Unsupported protocol version"));
        }
    } else if handshake.next_state != PROTOCOL_STATE_STATUS {
        return Err(IOError::new(IOErrorKind::InvalidData, "Invalid next state"));
    }
    Ok(handshake)
}

pub async fn send_login_disconnect(stream: &mut TcpStream, buffer: &mut Vec<u8>, text: Text, version: i32, compression: i32, encryption: &mut Option<PacketEncryption>) -> IOResult<()> {
    packets::get_full_server_packet_buf_write_buffer(buffer, &LoginDisconnect {
        text
    }, version, ProtocolState::Login)?;
    encode_and_send_packet(stream, buffer, &mut vec![], compression, encryption).await?;
    Ok(())
}

async fn handle_login(stream: &mut TcpStream, handshake: Handshake, buffer: &mut Vec<u8>, address: SocketAddr) -> IOResult<ProxyingData> {
    #[derive(Debug, PartialEq, Eq)]
    enum LoginState {
        Request,
        Encryption,
        LoginAck,
    }
    let mut write_buf = Vec::new();
    let mut protocol_buf = Vec::new();

    let version = handshake.version;
    let mut login_state = LoginState::Request;
    let mut login_request = None;
    let mut compression_threshold = -1;
    let mut encryption = None;
    let mut decryption = None;
    let mut server_id = None;
    let mut verify_token = None;
    let mut profile = None;
    loop {
        buffer.clear();
        read_and_decode_packet(stream, buffer, &mut protocol_buf, compression_threshold, &mut decryption).await?;

        let mut reader = Cursor::new(&*buffer);
        let id = VarInt::decode_simple(&mut reader)?.get();

        let buffers = WriteBuffers {
            write_buf: &mut write_buf,
            protocol_buf: &mut protocol_buf,
        };

        let packet_type = PacketRegistry::instance().get_client_packet_type(ProtocolState::Login, version, id);
        if let Some(packet_type) = packet_type {
            match packet_type {
                ClientPacketType::LoginRequest => {
                    if login_state != LoginState::Request {
                        return Err(IOError::new(IOErrorKind::InvalidData, format!("Received login request in {:?} state", login_state)));
                    }
                    let request = LoginRequest::decode(&mut reader, version)?;
                    if !crate::util::is_username_valid(&request.name) {
                        send_login_disconnect(stream, buffers.write_buf, Text::new(TextContent::literal("§cInvalid username".into())), version, compression_threshold, &mut encryption).await.ok();
                        return Err(IOError::new(IOErrorKind::InvalidData, "Bad username"));
                    }
                    let cfg = ProxyServer::instance().config();
                    if cfg.offline_mode_encryption || cfg.online_mode {
                        if !cfg.online_mode {
                            profile = Some(GameProfile {
                                id: crate::util::generate_uuid(&request.name).to_string(),
                                name: request.name.clone(),
                                properties: vec![],
                            });
                        }
                        login_state = LoginState::Encryption;
                        let data = send_encryption(stream, handshake.version, compression_threshold, buffers).await?;
                        server_id = Some(data.0);
                        verify_token = Some(data.1);
                    } else {
                        profile = Some(finish_login(stream, GameProfile {
                            id: crate::util::generate_uuid(&request.name).to_string(),
                            name: request.name.clone(),
                            properties: Vec::new(),
                        }, handshake.version, buffers, &mut compression_threshold, &mut None).await?);
                        login_state = LoginState::LoginAck;
                    };
                    login_request = Some(request);
                }
                ClientPacketType::EncryptionResponse => {
                    if login_state != LoginState::Encryption {
                        return Err(IOError::new(IOErrorKind::InvalidData, format!("Received encryption response in {:?} state", login_state)));
                    }
                    let response = EncryptionResponse::decode(&mut reader, version)?;
                    let secret = ProxyServer::instance().rsa_private_key().decrypt(Pkcs1v15Encrypt, &response.shared_secret)
                        .map_err(|e| IOError::new(IOErrorKind::InvalidData, format!("Failed to decrypt shared secret: {}", e)))?;
                    if secret.len() != 16 {
                        send_login_disconnect(stream, buffers.write_buf, Text::new(TextContent::literal("§cInvalid shared secret".into())), version, compression_threshold, &mut encryption).await.ok();
                        return Err(IOError::new(IOErrorKind::InvalidData, "Bad shared secret length"));
                    }

                    if let Some(ref token) = response.verify_token {
                        let token = ProxyServer::instance().rsa_private_key().decrypt(Pkcs1v15Encrypt, token)
                            .map_err(|e| IOError::new(IOErrorKind::InvalidData, format!("Failed to decrypt verify token: {}", e)))?;
                        if verify_token.as_ref().unwrap().as_slice() != token {
                            send_login_disconnect(stream, buffers.write_buf, Text::new(TextContent::literal("§cInvalid verify token".into())), version, compression_threshold, &mut encryption).await.ok();
                            return Err(IOError::new(IOErrorKind::InvalidData, "Bad verify token"));
                        }
                    }

                    let secret = secret.try_into().unwrap();

                    let cfg = ProxyServer::instance().config();
                    if cfg.online_mode {
                        match crate::auth::has_joined(&login_request.as_ref().unwrap().name, server_id.as_ref().unwrap(), &secret, match cfg.prevent_proxy_connections {
                            true => Some(stream.peer_addr().unwrap().ip()),
                            false => None,
                        }).await? {
                            Some(p) => profile = Some(p),
                            None => return Err(IOError::new(IOErrorKind::InvalidData, "Failed to verify user")),
                        }
                    }

                    encryption = Some(PacketEncryption::new(&secret));
                    decryption = Some(PacketDecryption::new(&secret));

                    profile = Some(finish_login(stream, profile.take().unwrap(), version, buffers, &mut compression_threshold, &mut encryption).await?);
                    login_state = LoginState::LoginAck;
                }
                ClientPacketType::LoginPluginResponse => {
                    return Err(IOError::new(IOErrorKind::InvalidData, "invalid login payload response"));
                }
                ClientPacketType::LoginAcknowledged => {
                    if login_state != LoginState::LoginAck {
                        return Err(IOError::new(IOErrorKind::InvalidData, format!("Received login acknowledge in {:?} state", login_state)));
                    }
                    return Ok(ProxyingData {
                        version,
                        profile: profile.unwrap(),
                        compression_threshold,
                        encryption: match encryption {
                            Some(encryption) => Some((encryption, decryption.unwrap())),
                            None => None,
                        },
                        player_public_key: login_request.unwrap().public_key,
                        protocol_state: ProtocolState::Config,
                        address,
                    });
                }
                ClientPacketType::CookieResponse => {
                    return Err(IOError::new(IOErrorKind::InvalidData, "invalid cookie response"));
                }
                _ => {
                    return Err(IOError::new(IOErrorKind::InvalidData, format!("Bad login packet id={}", id)));
                }
            }
        } else {
            return Err(IOError::new(IOErrorKind::InvalidData, format!("Bad login packet id={}", id)));
        }
    }
}

async fn send_encryption(stream: &mut TcpStream, version: i32, compression: i32, buffers: WriteBuffers<'_>) -> IOResult<(String, [u8; 6])> {
    buffers.write_buf.clear();
    buffers.protocol_buf.clear();
    let mut server_id = [0u8; 6];
    rand::thread_rng().fill_bytes(&mut server_id);
    let server_id = hex::encode(server_id);

    let mut verify_token = [0u8; 6];
    rand::thread_rng().fill_bytes(&mut verify_token);
    packets::get_full_server_packet_buf_write_buffer(buffers.write_buf, &EncryptionRequest {
        server_id: server_id.clone(),
        should_authenticate: ProxyServer::instance().config().online_mode,
        verify_token: verify_token.to_vec(),
        public_key: ProxyServer::instance().rsa_public_key().to_public_key_der().unwrap().into_vec(),
    }, version, ProtocolState::Login)?;
    encode_and_send_packet(stream, buffers.write_buf, buffers.protocol_buf, compression, &mut None).await?;

    Ok((server_id, verify_token))
}

async fn finish_login(stream: &mut TcpStream, profile: GameProfile, version: i32, buffers: WriteBuffers<'_>, compression: &mut i32, encryption: &mut Option<PacketEncryption>) -> IOResult<GameProfile> {
    buffers.write_buf.clear();
    buffers.protocol_buf.clear();
    *compression = ProxyServer::instance().config().compression_threshold;
    if *compression >= 0 {
        packets::get_full_server_packet_buf_write_buffer(buffers.write_buf, &SetCompression {
            compression: *compression
        }, version, ProtocolState::Login)?;
        encode_and_send_packet(stream, buffers.write_buf, buffers.protocol_buf, -1, encryption).await?;
    }

    buffers.write_buf.clear();
    buffers.protocol_buf.clear();
    packets::get_full_server_packet_buf_write_buffer(buffers.write_buf, &LoginSuccess { profile: profile.clone() }, version, ProtocolState::Login)?;
    encode_and_send_packet(stream, buffers.write_buf, buffers.protocol_buf, *compression, encryption).await?;
    Ok(profile)
}
