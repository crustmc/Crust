use std::{future::Future, io::Cursor, pin::Pin, sync::{atomic::Ordering, Arc}};

use crate::{chat::Text, server::{packets::{self, Packet}, ProxyServer}, util::IOResult};

use self::commands::Command;

use super::{commands, packet_ids::{ClientPacketType, PacketRegistry, ServerPacketType}, packets::{ClientSettings, Kick, ProtocolState, SystemChatMessage, UnsignedClientCommand}, proxy_handler::ConnectionHandle, PlayerSyncData, SlotId};

pub struct ClientPacketHandler;

impl ClientPacketHandler {
    pub async fn handle_packet(packet_id: i32, buffer: &[u8], version: i32, player_id: SlotId, client_handle: &ConnectionHandle, sync_data: &Arc<PlayerSyncData>) -> IOResult<bool> {
        match PacketRegistry::instance().get_client_packet_type(client_handle.protocol_state(), version, packet_id) {
            Some(packet_type) => match packet_type {
                ClientPacketType::FinishConfiguration => {
                    client_handle.set_protocol_state(ProtocolState::Game);
                }
                ClientPacketType::ConfigurationAck => {
                    client_handle.set_protocol_state(ProtocolState::Config);
                    if sync_data.is_switching_server.load(Ordering::Relaxed) {
                        sync_data.config_ack_notify.notify_one();
                    }
                }
                ClientPacketType::ClientSettings => {
                    let packet = ClientSettings::decode(&mut Cursor::new(buffer), version)?;
                    *sync_data.client_settings.lock().await = Some(packet);
                }
                ClientPacketType::UnsignedClientCommand => {
                    let packet = UnsignedClientCommand::decode(&mut Cursor::new(buffer), version)?;
                    let line = packet.message;
                    let mut split: Vec<&str> = line.split_ascii_whitespace().collect();
                    if split.len() > 0 {
                        let a = split.get(0).unwrap().to_ascii_lowercase();
                        let command = a.as_str();
                        split.remove(0);
                        match command {
                            "server" => {
                                commands::CommandServer::execute(player_id, split).await;
                                return Ok(false);
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            },
            None => {}
        }
        Ok(true)
    }
}

pub fn switch_server_helper(player: SlotId, server_id: SlotId) -> Pin<Box<dyn Future<Output=()> + Send>> {
    let block = async move {
        let players = ProxyServer::instance().players().read().await;
        if let Some(player) = players.get(player) {
            if player.current_server == server_id {
                player.send_message(Text::new("§cYou're already connected to this server")).await.ok();
                return;
            }
            player.switch_server(server_id).await;
        }
    };
    Box::pin(block)
}


pub struct ServerPacketHandler;

impl ServerPacketHandler {
    pub async fn handle_packet(packet_id: i32, buffer: &[u8], version: i32, _player_id: SlotId, server_handle: &ConnectionHandle, _: &Arc<PlayerSyncData>, client_handle: &ConnectionHandle) -> IOResult<bool> {
        match PacketRegistry::instance().get_server_packet_type(server_handle.protocol_state(), version, packet_id) {
            Some(packet_type) => match packet_type {
                ServerPacketType::BundleDelimiter => {
                    client_handle.on_bundle().await;
                    return Ok(false);
                }
                ServerPacketType::Kick => {
                    let kick = Kick::decode(&mut Cursor::new(buffer), version)?;
                    let state = server_handle.protocol_state();
                    if state == ProtocolState::Game {
                        let chat = SystemChatMessage {
                            message: kick.text,
                            pos: 0,
                        };
                        let data = packets::get_full_server_packet_buf(&chat, version, state)?;
                        if let Some(data) = data {
                            client_handle.queue_packet(data, false).await;
                        }
                    }
                    return Ok(false);
                }
                _ => {}
            },
            None => {}
        }
        Ok(true)
    }
}