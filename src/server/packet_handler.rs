use std::{future::Future, io::Cursor, pin::Pin, sync::{atomic::Ordering, Arc}};

use crate::{chat::Text, server, server::{packets::{self, Packet}, ProxyServer}, util::IOResult};
use crate::server::packets::{ClientCustomPayload, ServerCustomPayload};
use crate::util::EncodingHelper;
use crate::version::R1_13;
use self::{command::CommandSender, packets::{TabCompleteRequest, TabCompleteResponse}};

use super::{brigadier::{ArgumentProperty, CommandNode, CommandNodeType, Commands, StringParserType, SuggestionsType}, command, packet_ids::{ClientPacketType, PacketRegistry, ServerPacketType}, packets::{ClientSettings, Kick, ProtocolState, SystemChatMessage, UnsignedClientCommand}, proxy_handler::ConnectionHandle, PlayerSyncData, SlotId};

pub struct ClientPacketHandler;

impl ClientPacketHandler {
    pub async fn handle_packet(packet_id: i32, buffer: &[u8], version: i32, player_id: SlotId, client_handle: &ConnectionHandle, sync_data: &Arc<PlayerSyncData>) -> IOResult<bool> {
        if let Some(packet_type) = PacketRegistry::instance().get_client_packet_type(client_handle.protocol_state(), version, packet_id) { match packet_type {
            ClientPacketType::FinishConfiguration => {
                client_handle.set_protocol_state(ProtocolState::Game);
            }
            ClientPacketType::ConfigurationAck => {
                client_handle.set_protocol_state(ProtocolState::Config);
                if sync_data.is_switching_server.load(Ordering::Relaxed) {
                    sync_data.config_ack_notify.notify_one();
                    return Ok(false);
                }
            }
            ClientPacketType::ClientSettings => {
                let packet = ClientSettings::decode(&mut Cursor::new(buffer), version)?;
                *sync_data.client_settings.lock().await = Some(packet);
            },
            ClientPacketType::ClientCustomPayload => {
                let packet = ClientCustomPayload::decode(&mut Cursor::new(buffer), version)?;
                if (version < R1_13 && packet.channel == "MC|Brand") || (version >= R1_13 && packet.channel == "minecraft:brand") {
                    *sync_data.brand_packet.lock().await = Some(packet);
                }
            }
            ClientPacketType::UnsignedClientCommand => {
                let packet = UnsignedClientCommand::decode(&mut Cursor::new(buffer), version)?;
                let line = packet.message;
                let command_name = line.split_ascii_whitespace().next().unwrap_or("").to_string();
                if ProxyServer::instance().command_registry().get_command_by_name(&command_name).is_none() {
                    return Ok(true);
                }
                tokio::task::spawn_blocking(move || { // Needs to be blocking because commands are executed synchronously
                    if ProxyServer::instance().command_registry().execute(&CommandSender::Player(player_id), &line) {
                        return true;
                    } else {
                        log::debug!("Command not found '{}' passing command to server", line);
                    }
                    false
                });
                return Ok(false);
            }
            ClientPacketType::TabCompleteRequest => {
                let packet = TabCompleteRequest::decode(&mut Cursor::new(buffer), version)?;
                let cursor = packet.cursor;
                if cursor.starts_with("/") {
                    let transaction_id = packet.transaction_id;
                    let response = tokio::task::spawn_blocking(move || { // Needs to be blocking because commands are executed synchronously
                        ProxyServer::instance().command_registry().tab_complete(&CommandSender::Player(player_id), &cursor[1..])
                    }).await?;
                    if let Some(response) = response {
                        if let Some(response) = response {
                            let packet = packets::get_full_server_packet_buf(&TabCompleteResponse {
                                transaction_id,
                                commands: None, // TODO: Implement for versions < R1_13
                                suggestions: Some(response),
                            }, version, client_handle.protocol_state())?.unwrap();
                            let _ = client_handle.queue_packet(packet, false).await;
                        }
                        return Ok(false);
                    }
                }
            }
            _ => {}
        } }
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
        if let Some(packet_type) = PacketRegistry::instance().get_server_packet_type(server_handle.protocol_state(), version, packet_id) { match packet_type {
            ServerPacketType::BundleDelimiter => {
                let _ = client_handle.on_bundle().await;
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
                        let _ = client_handle.queue_packet(data, false).await;
                    }
                }
                return Ok(false);
            }
            ServerPacketType::Commands => {
                let mut commands = Commands::decode(&mut Cursor::new(buffer), version)?;
                for info in ProxyServer::instance().command_registry().all_commands() {
                    let arg_index = commands.nodes.len();
                    commands.nodes.push(CommandNode {
                        childrens: Vec::new(),
                        executable: true,
                        redirect_index: None,
                        node_type: CommandNodeType::Argument {
                            name: "args".to_string(),
                            parser_id: 5, // StringArgumentType
                            properties: Some(ArgumentProperty::String(StringParserType::GreedyPhrase)),
                            suggestions_type: info.tab_completer.as_ref().map(|_| SuggestionsType::AskServer),
                        }
                    });
                    for name in &info.names {
                        let node_index = commands.nodes.len();
                        commands.nodes.push(CommandNode {
                            childrens: vec![arg_index],
                            executable: false,
                            redirect_index: None,
                            node_type: CommandNodeType::Literal(name.clone()),
                        });
                        commands.nodes[commands.root_index].childrens.push(node_index);
                    }
                }
                if let Some(packet_buf) = packets::get_full_server_packet_buf(&commands, version, server_handle.protocol_state())? {
                    let _ = client_handle.queue_packet(packet_buf, false).await;
                }
                return Ok(false);
            },
            ServerPacketType::ServerCustomPayload => {
                let mut packet = ServerCustomPayload::decode(&mut Cursor::new(buffer), version)?;
                if (version < R1_13 && packet.channel == "MC|Brand") || (version >= R1_13 && packet.channel == "minecraft:brand") {
                    let server_brand = EncodingHelper::read_string(&mut Cursor::new(&mut packet.data), u16::MAX as usize)?;
                    packet.data.clear();
                    EncodingHelper::write_string(&mut packet.data, format!("{} -> {}", server::NAME, server_brand).as_str())?;
                    if let Some(packet_buf) = packets::get_full_server_packet_buf(&packet, version, server_handle.protocol_state())? {
                        let _ = client_handle.queue_packet(packet_buf, false).await;
                    }
                    return Ok(false);
                }
                
                // TODO hanlde bungeecord plugin messages

            }
            _ => {}
        } }
        Ok(true)
    }
}