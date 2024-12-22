use std::{collections::HashMap, ops::RangeFrom};

use lazy_static::lazy_static;

use crate::version::*;

use super::packets::ProtocolState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ServerPacketType {
    LoginDisconnect, // login disconnected

    EncryptionRequest, // login
    LoginPluginRequest, // login
    CookieRequest, // login config play
    LoginSuccess, // login
    SetCompression, // login

    StartConfiguration, // game
    Kick, // config, game
    ServerCustomPayload,
    FinishConfiguration, // config
    BundleDelimiter, // game
    SystemChatMessage, // game
    Commands, // game
    TabCompleteResponse, // game
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ClientPacketType {
    Handshake,

    LoginRequest,
    EncryptionResponse,
    LoginPluginResponse,
    CookieResponse,

    LoginAcknowledged, // last packet before config mode

    ClientCustomPayload, // config / game

    ConfigurationAck, // game
    FinishConfiguration, // config
    ClientSettings, // config game
    UnsignedClientCommand, // game
    TabCompleteRequest, // game
}

pub struct PacketRegistry {
    server_packet_ids: HashMap<(ProtocolState, ServerPacketType, i32), u8>,
    server_packet_types: HashMap<(ProtocolState, u8, i32), ServerPacketType>,
    client_packet_ids: HashMap<(ProtocolState, ClientPacketType, i32), u8>,
    client_packet_types: HashMap<(ProtocolState, u8, i32), ClientPacketType>,
}

lazy_static! {
    static ref PACKET_REGISTRY: PacketRegistry = PacketRegistry::new();
}

#[allow(dead_code)]
impl PacketRegistry {
    
    pub fn instance() -> &'static Self {
        &PACKET_REGISTRY
    }
    
    pub fn get_server_packet_id(&self, state: ProtocolState, version: i32, packet_type: ServerPacketType) -> Option<i32> {
        self.server_packet_ids.get(&(state, packet_type, version)).copied().map(|id| id as i32)
    }
    
    pub fn get_server_packet_type(&self, state: ProtocolState, version: i32, packet_id: i32) -> Option<ServerPacketType> {
        if !(0..256).contains(&packet_id) {
            return None;
        }
        self.server_packet_types.get(&(state, packet_id as u8, version)).copied()
    }
    
    pub fn get_client_packet_id(&self, state: ProtocolState, version: i32, packet_type: ClientPacketType) -> Option<i32> {
        self.client_packet_ids.get(&(state, packet_type, version)).copied().map(|id| id as i32)
    }
    
    pub fn get_client_packet_type(&self, state: ProtocolState, version: i32, packet_id: i32) -> Option<ClientPacketType> {
        if !(0..256).contains(&packet_id) {
            return None;
        }
        self.client_packet_types.get(&(state, packet_id as u8, version)).copied()
    }

    fn new() -> Self {
        let mut registry = Self {
            server_packet_ids: HashMap::new(),
            server_packet_types: HashMap::new(),
            client_packet_ids: HashMap::new(),
            client_packet_types: HashMap::new(),
        };

        registry.register_packets();

        registry
    }

    fn register_packets(&mut self) {
        let mut client_packet_ids = HashMap::<(ProtocolState, ClientPacketType), Vec<(RangeFrom<i32>, u8)>>::new();
        let mut client_packet_types = HashMap::<(ProtocolState, u8), Vec<(RangeFrom<i32>, ClientPacketType)>>::new();
        let mut server_packet_ids = HashMap::<(ProtocolState, ServerPacketType), Vec<(RangeFrom<i32>, u8)>>::new();
        let mut server_packet_types = HashMap::<(ProtocolState, u8), Vec<(RangeFrom<i32>, ServerPacketType)>>::new();

        macro_rules! begin {
            (Client, $state:ident, $typ:ident; $( ($ver:ident, $id:literal) )*) => {{
                let state = super::packets::ProtocolState::$state;
                let packet_type = ClientPacketType::$typ;
                $(
                    client_packet_ids.entry((state, packet_type)).or_insert_with(Vec::new).push(($ver.., $id as u8));
                    client_packet_types.entry((state, $id as u8)).or_insert_with(Vec::new).push(($ver.., packet_type));
                )*
            }};
            (Server, $state:ident, $typ:ident; $( ($ver:ident, $id:literal) )*) => {{
                let state = super::packets::ProtocolState::$state;
                let packet_type = ServerPacketType::$typ;
                $(
                    server_packet_ids.entry((state, packet_type)).or_insert_with(Vec::new).push(($ver.., $id as u8));
                    server_packet_types.entry((state, $id as u8)).or_insert_with(Vec::new).push(($ver.., packet_type));
                )*
            }};
        }
        { // Handshake
            begin! {
                Client, Handshake, Handshake;
                (R1_8, 0x00)
            }
        }

        { // Login
            begin! {
                Client, Login, LoginRequest;
                (R1_8, 0x00)
            }
            begin! {
                Client, Login, EncryptionResponse;
                (R1_8, 0x01)
            }
            begin! {
                Client, Login, LoginPluginResponse;
                (R1_13, 0x02)
            }
            begin! {
                Client, Login, LoginAcknowledged;
                (R1_20_2, 0x03)
            }
            begin! {
                Client, Login, CookieResponse;
                (R1_20_5, 0x04)
            }

            begin! {
                Server, Login, LoginDisconnect;
                (R1_8, 0x00)
            }
            begin! {
                Server, Login, EncryptionRequest;
                (R1_8, 0x01)
            }
            begin! {
                Server, Login, LoginSuccess;
                (R1_8, 0x02)
            }
            begin! {
                Server, Login, SetCompression;
                (R1_8, 0x03)
            }
            begin! {
                Server, Login, LoginPluginRequest;
                (R1_13, 0x04)
            }
            begin! {
                Server, Login, CookieRequest;
                (R1_20_5, 0x05)
            }
        }

        { // Configuration state
            begin! {
                Client, Config, ClientSettings;
                (R1_20_2, 0x00)
            }

            begin! {
                Client, Config, ClientCustomPayload;
                (R1_20_2, 0x01)
                (R1_20_5, 0x02)
            }
            
            begin! {
                Client, Config, FinishConfiguration;
                (R1_20_2, 0x02)
                (R1_20_5, 0x03)
            }

            begin! {
                Server, Config, CookieRequest;
                (R1_20_5, 0x00)
            }
            begin! {
                Server, Config, ServerCustomPayload;
                (R1_20_2, 0x00)
                (R1_20_5, 0x01)
            }

            begin! {
                Server, Config, Kick;
                (R1_20_2, 0x01)
                (R1_20_5, 0x02)
            }
            begin! {
                Server, Config, FinishConfiguration;
                (R1_20_2, 0x02)
                (R1_20_5, 0x03)
            }
        }

        { // Game state
            begin! {
                Client, Game, ClientSettings;
                (R1_19_4, 0x08)
                (R1_20_2, 0x09)
                (R1_20_5, 0x0A)
                (R1_21_2, 0x0C)
            }
            begin! {
                Client, Game, UnsignedClientCommand;
                (R1_20_5, 0x04)
                (R1_21_2, 0x05)
            }
            begin! {
                Client, Game, TabCompleteRequest;
                (R1_8, 0x14)
                (R1_9, 0x01)
                (R1_12, 0x02)
                (R1_12_1, 0x01)
                (R1_13, 0x05)
                (R1_14, 0x06)
                (R1_19, 0x08)
                (R1_19_1, 0x09)
                (R1_19_3, 0x08)
                (R1_19_4, 0x09)
                (R1_20_2, 0x0A)
                (R1_20_5, 0x0B)
                (R1_21_2, 0x0D)
            }
            begin! {
                Client, Game, ConfigurationAck;
                (R1_20_2, 0x0B)
                (R1_20_5, 0x0C)
                (R1_21_2, 0x0E)
            }

            begin! {
                Client, Game, ClientCustomPayload;
                (R1_8, 0x17 )
                (R1_9, 0x09 )
                (R1_12, 0x0A )
                (R1_12_1, 0x09 )
                (R1_13, 0x0A )
                (R1_14, 0x0B )
                (R1_17, 0x0A )
                (R1_19, 0x0C )
                (R1_19_1, 0x0D )
                (R1_19_3, 0x0C )
                (R1_19_4, 0x0D )
                (R1_20_2, 0x0F )
                (R1_20_3, 0x10 )
                (R1_20_5, 0x12 )
                (R1_21_2, 0x14 )
            }

            begin! {
                Server, Game, CookieRequest;
                (R1_20_5, 0x16)
            }

            begin! {
                Server, Game, ServerCustomPayload;
                (R1_8, 0x3F )
                (R1_9, 0x18 )
                (R1_13, 0x19 )
                (R1_14, 0x18 )
                (R1_15, 0x19 )
                (R1_16, 0x18 )
                (R1_16_2, 0x17 )
                (R1_17, 0x18 )
                (R1_19, 0x15 )
                (R1_19_1, 0x16 )
                (R1_19_3, 0x15 )
                (R1_19_4, 0x17 )
                (R1_20_2, 0x18 )
                (R1_20_5, 0x19 )
            }
            
            begin! {
                Server, Game, Kick;
                (R1_8, 0x40)
                (R1_9, 0x1A)
                (R1_13, 0x1B)
                (R1_14, 0x1A)
                (R1_15, 0x1B)
                (R1_16, 0x1A)
                (R1_16_2, 0x19)
                (R1_17, 0x1A)
                (R1_19, 0x17)
                (R1_19_1, 0x19)
                (R1_19_3, 0x17)
                (R1_19_4, 0x1A)
                (R1_20_2, 0x1B)
                (R1_20_5, 0x1D)
            }
            begin! {
                Server, Game, StartConfiguration;
                (R1_20_2, 0x65)
                (R1_20_3, 0x67)
                (R1_20_5, 0x69)
                (R1_21_2, 0x70)
            }
            begin! {
                Server, Game, SystemChatMessage;
                (R1_19_4, 0x64)
                (R1_20_2, 0x67)
                (R1_20_3, 0x69)
                (R1_20_5, 0x6C)
                (R1_21_2, 0x73)
            }
            begin! {
                Server, Game, BundleDelimiter;
                (R1_19_4, 0x00)
            }
            begin! {
                Server, Game, Commands;
                (R1_13, 0x11)
                (R1_15, 0x12)
                (R1_16, 0x11)
                (R1_16_2, 0x10)
                (R1_17, 0x12)
                (R1_19, 0x0F)
                (R1_19_3, 0x0E)
                (R1_19_4, 0x10)
                (R1_20_2, 0x11)
            }
            begin! {
                Server, Game, TabCompleteResponse;
                (R1_8, 0x3A)
                (R1_9, 0x0E)
                (R1_13, 0x10)
                (R1_15, 0x11)
                (R1_16, 0x10)
                (R1_16_2, 0x0F)
                (R1_17, 0x11)
                (R1_19, 0x0E)
                (R1_19_3, 0x0D)
                (R1_19_4, 0x0F)
                (R1_20_2, 0x10)
            }
        }

        for version in SUPPORTED_VERSIONS.iter() {
            for ((state, packet_type), list) in server_packet_ids.iter() {
                for (version_range, packet_id) in list.iter().rev() {
                    if version_range.contains(version) {
                        self.server_packet_ids.insert((*state, *packet_type, *version), *packet_id);
                        self.server_packet_types.insert((*state, *packet_id, *version), *packet_type);
                        break;
                    }
                }
            }

            for ((state, packet_type), list) in client_packet_ids.iter() {
                for (version_range, packet_id) in list.iter().rev() {
                    if version_range.contains(version) {
                        self.client_packet_ids.insert((*state, *packet_type, *version), *packet_id);
                        self.client_packet_types.insert((*state, *packet_id, *version), *packet_type);
                        break;
                    }
                }
            }
        }
    }
}
