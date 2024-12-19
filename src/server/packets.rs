use std::{io::{Cursor, ErrorKind, Read, Write}, time::Duration};

use byteorder::{ReadBytesExt, WriteBytesExt, BE};
use either::Either;
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;

use crate::{auth::{GameProfile, Property}, chat::Text, server::nbt, util::{EncodingHelper, IOError, IOErrorKind, IOResult, VarInt}, version::*};

use super::{brigadier::Suggestions, compression::RefSizeLimitedReader, encryption::{PacketDecryption, PacketEncryption}, nbt::NbtType, packet_ids::{ClientPacketType, PacketRegistry, ServerPacketType}};

pub const PROTOCOL_READ_TIMEOUT: Duration = Duration::from_secs(30);

pub const PROTOCOL_STATE_STATUS: i32 = 1;
pub const PROTOCOL_STATE_LOGIN: i32 = 2;
pub const PROTOCOL_STATE_TRANSFER: i32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ProtocolState {
    Handshake,
    Login,
    Config,
    Game,
}

pub trait ServerPacket {
    fn get_type(&self) -> ServerPacketType;
}

pub trait ClientPacket {
    fn get_type(&self) -> ClientPacketType;
}

pub trait Packet {
    fn decode<R: Read + ?Sized>(src: &mut R, version: i32) -> IOResult<Self>
    where
        Self: Sized;
    fn encode<W: Write + ?Sized>(&self, dst: &mut W, version: i32) -> IOResult<()>;
}

pub fn get_full_server_packet_buf<P: Packet + ServerPacket>(packet: &P, version: i32, protocol: ProtocolState) -> IOResult<Option<Vec<u8>>> {
    if let Some(packet_id) = PacketRegistry::instance().get_server_packet_id(protocol, version, packet.get_type()) {
        let mut data = Vec::new();
        VarInt(packet_id).encode_simple(&mut data)?;
        packet.encode(&mut data, version)?;
        return Ok(Some(data));
    }
    panic!("packet not found: {:#?}, version: {:#?}, protocol: {:#?}", packet.get_type(), version, protocol);
    //Ok(None)
}
pub fn get_full_server_packet_buf_write_buffer<P: Packet + ServerPacket>(buffer: &mut Vec<u8>, packet: &P, version: i32, protocol: ProtocolState) -> IOResult<bool> {
    if let Some(packet_id) = PacketRegistry::instance().get_server_packet_id(protocol, version, packet.get_type()) {
        buffer.clear();
        VarInt(packet_id).encode_simple(buffer)?;
        packet.encode(buffer, version)?;
        return Ok(true);
    }
    panic!("packet not found: {:#?}, version: {:#?}, protocol: {:#?}", packet.get_type(), version, protocol);
    //Ok(false)
}

pub fn get_full_client_packet_buf<P: Packet + ClientPacket>(packet: &P, version: i32, protocol: ProtocolState) -> IOResult<Option<Vec<u8>>> {
    if let Some(packet_id) = PacketRegistry::instance().get_client_packet_id(protocol, version, packet.get_type()) {
        let mut data = Vec::new();
        VarInt(packet_id).encode_simple(&mut data)?;
        packet.encode(&mut data, version)?;
        return Ok(Some(data));
    }
    panic!("packet not found: {:#?}, version: {:#?}, protocol: {:#?}", packet.get_type(), version, protocol);
    //Ok(None)
}

pub fn get_full_client_packet_buf_write_buffer<P: Packet + ClientPacket>(buffer: &mut Vec<u8>, packet: &P, version: i32, protocol: ProtocolState) -> IOResult<bool> {
    if let Some(packet_id) = PacketRegistry::instance().get_client_packet_id(protocol, version, packet.get_type()) {
        buffer.clear();
        VarInt(packet_id).encode_simple(buffer)?;
        packet.encode(buffer, version)?;
        return Ok(true);
    }
    panic!("packet not found: {:#?}, version: {:#?}, protocol: {:#?}", packet.get_type(), version, protocol);
    //Ok(false)
}


#[derive(Debug)]
pub struct Handshake {
    pub version: i32,
    pub host: String,
    pub port: u16,
    pub next_state: i32,
}

impl ClientPacket for Handshake {
    fn get_type(&self) -> ClientPacketType {
        ClientPacketType::Handshake
    }
}

impl Packet for Handshake {
    fn decode<R: Read + ?Sized>(src: &mut R, _: i32) -> IOResult<Self>
    where
        Self: Sized,
    {
        Ok(Self {
            version: VarInt::decode_simple(src)?.get(),
            host: EncodingHelper::read_string(src, 255)?,
            port: src.read_u16::<BE>()?,
            next_state: VarInt::decode_simple(src)?.get(),
        })
    }

    fn encode<W: Write + ?Sized>(&self, dst: &mut W, _: i32) -> IOResult<()> {
        VarInt(self.version).encode(dst, 5)?;
        EncodingHelper::write_string(dst, &self.host)?;
        dst.write_u16::<BE>(self.port)?;
        VarInt(self.next_state).encode(dst, 5)?;
        Ok(())
    }
}

pub struct LoginDisconnect {
    pub text: Text,
}

impl ServerPacket for LoginDisconnect {
    fn get_type(&self) -> ServerPacketType {
        ServerPacketType::LoginDisconnect
    }
}

impl Packet for LoginDisconnect {
    fn decode<R: Read + ?Sized>(src: &mut R, _: i32) -> IOResult<Self>
    where
        Self: Sized,
    {
        let string = EncodingHelper::read_string(src, i16::MAX as usize)?;
        let value: Value = serde_json::from_str(&string)?;
        let text = crate::chat::deserialize_json(&value).map_err(|err| IOError::new(IOErrorKind::InvalidData, err))?;
        Ok(Self {
            text
        })
    }

    fn encode<W: Write + ?Sized>(&self, dst: &mut W, _: i32) -> IOResult<()> {
        let text = crate::chat::serialize_json(&self.text);
        let string = serde_json::to_string(&text)?;
        EncodingHelper::write_string(dst, &string)?;
        Ok(())
    }
}


pub struct Kick {
    pub text: Text,
}

impl ServerPacket for Kick {
    fn get_type(&self) -> ServerPacketType {
        ServerPacketType::Kick
    }
}

impl Packet for Kick {
    fn decode<R: Read + ?Sized>(src: &mut R, version: i32) -> IOResult<Self>
    where
        Self: Sized,
    {
        if version >= R1_20_3 {
            let nbt = nbt::read_networking_nbt(src, version)?;
            if let Some(nbt_tag) = nbt.left() {
                if let Some(nbt_tag) = nbt_tag {
                    let json = nbt_tag.to_json();
                    let text = crate::chat::deserialize_json(&json).map_err(|err| IOError::new(ErrorKind::InvalidData, err))?;
                    Ok(Self {
                        text
                    })
                } else {
                    Err(IOError::new(ErrorKind::InvalidData, "invalid nbt type"))
                }
            } else {
                panic!("this is impossible")
            }
        } else {
            let string = EncodingHelper::read_string(src, i16::MAX as usize)?;
            let value: Value = serde_json::from_str(&string)?;
            let text = crate::chat::deserialize_json(&value).map_err(|err| IOError::new(IOErrorKind::InvalidData, err))?;
            Ok(Self {
                text
            })
        }
    }

    fn encode<W: Write + ?Sized>(&self, dst: &mut W, version: i32) -> IOResult<()> {
        if version >= R1_20_3 {
            let json = crate::chat::serialize_json(&self.text);
            let nbt = NbtType::from_json(&json)?;
            nbt::write_networking_nbt(dst, version, &Either::Left(Some(nbt)))?;
        } else {
            let text = crate::chat::serialize_json(&self.text);
            let string = serde_json::to_string(&text)?;
            EncodingHelper::write_string(dst, &string)?;
        }
        Ok(())
    }
}


pub struct SetCompression {
    pub compression: i32,
}

impl ServerPacket for SetCompression {
    fn get_type(&self) -> ServerPacketType {
        ServerPacketType::SetCompression
    }
}

impl Packet for SetCompression {
    fn decode<R: Read + ?Sized>(src: &mut R, _: i32) -> IOResult<Self>
    where
        Self: Sized,
    {
        Ok(Self {
            compression: VarInt::decode_simple(src)?.get()
        })
    }

    fn encode<W: Write + ?Sized>(&self, dst: &mut W, _: i32) -> IOResult<()> {
        VarInt(self.compression).encode_simple(dst)?;
        Ok(())
    }
}


pub struct LoginAcknowledged;

impl ClientPacket for LoginAcknowledged {
    fn get_type(&self) -> ClientPacketType {
        ClientPacketType::LoginAcknowledged
    }
}

impl Packet for LoginAcknowledged {
    fn decode<R: Read + ?Sized>(_: &mut R, _: i32) -> IOResult<Self>
    where
        Self: Sized,
    {
        Ok(Self)
    }

    fn encode<W: Write + ?Sized>(&self, _: &mut W, _: i32) -> IOResult<()> {
        Ok(())
    }
}

pub struct LoginRequest {
    pub name: String,
    pub public_key: Option<PlayerPublicKey>,
    pub uuid: Option<Uuid>,
}

impl ClientPacket for LoginRequest {
    fn get_type(&self) -> ClientPacketType {
        ClientPacketType::LoginRequest
    }
}

impl Packet for LoginRequest {
    fn decode<R: Read + ?Sized>(src: &mut R, version: i32) -> IOResult<Self>
    where
        Self: Sized,
    {
        let name = EncodingHelper::read_string(src, 16)?;
        let public_key = if version >= R1_19 && version < R1_19_3 && src.read_u8()? != 0 {
            Some(PlayerPublicKey::decode(src, version)?)
        } else {
            None
        };
        let uuid = if version >= R1_19_1 {
            if version >= R1_20_2 || src.read_u8()? != 0 {
                Some(EncodingHelper::read_uuid(src)?)
            } else {
                None
            }
        } else {
            None
        };
        Ok(Self { name, public_key, uuid })
    }

    fn encode<W: Write + ?Sized>(&self, dst: &mut W, version: i32) -> IOResult<()> {
        EncodingHelper::write_string(dst, &self.name)?;
        if version >= R1_19 && version < R1_19_3 {
            if let Some(ref key) = self.public_key {
                dst.write_u8(1)?;
                key.encode(dst, version)?;
            } else {
                dst.write_u8(0)?;
            }
        }
        if version >= R1_19_1 {
            if version >= R1_20_2 {
                EncodingHelper::write_uuid(dst, self.uuid.as_ref().unwrap())?;
            } else if let Some(ref uuid) = self.uuid {
                dst.write_u8(1)?;
                EncodingHelper::write_uuid(dst, uuid)?;
            } else {
                dst.write_u8(0)?;
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct PlayerPublicKey {
    pub expiry: u64,
    pub key: Vec<u8>,
    pub signature: Vec<u8>,
}

impl PlayerPublicKey {
    fn decode<R: Read + ?Sized>(src: &mut R, _: i32) -> IOResult<Self>
    where
        Self: Sized,
    {
        Ok(Self {
            expiry: src.read_u64::<BE>()?,
            key: EncodingHelper::read_byte_array(src, 512)?,
            signature: EncodingHelper::read_byte_array(src, 4096)?,
        })
    }

    fn encode<W: Write + ?Sized>(&self, dst: &mut W, _: i32) -> IOResult<()> {
        dst.write_u64::<BE>(self.expiry)?;
        EncodingHelper::write_byte_array(dst, &self.key)?;
        EncodingHelper::write_byte_array(dst, &self.signature)?;
        Ok(())
    }
}

pub struct EncryptionRequest {
    pub server_id: String,
    pub public_key: Vec<u8>,
    pub verify_token: Vec<u8>,
    pub should_authenticate: bool,
}

impl ServerPacket for EncryptionRequest {
    fn get_type(&self) -> ServerPacketType {
        ServerPacketType::EncryptionRequest
    }
}

impl Packet for EncryptionRequest {
    fn decode<R: Read + ?Sized>(src: &mut R, version: i32) -> IOResult<Self>
    where
        Self: Sized,
    {
        let server_id = EncodingHelper::read_string(src, 20)?;
        let public_key = EncodingHelper::read_byte_array(src, 256)?;
        let verify_token = EncodingHelper::read_byte_array(src, 256)?;
        let should_authenticate = if version >= R1_20_5 {
            src.read_u8()? != 0
        } else { true };

        Ok(Self { server_id, public_key, verify_token, should_authenticate })
    }

    fn encode<W: Write + ?Sized>(&self, dst: &mut W, version: i32) -> IOResult<()> {
        EncodingHelper::write_string(dst, &self.server_id)?;
        EncodingHelper::write_byte_array(dst, &self.public_key)?;
        EncodingHelper::write_byte_array(dst, &self.verify_token)?;
        if version >= R1_20_5 {
            dst.write_u8(self.should_authenticate as u8)?;
        }
        Ok(())
    }
}

pub struct EncryptionResponse {
    pub shared_secret: Vec<u8>,
    pub verify_token: Option<Vec<u8>>,
    pub encryption_data: Option<EncryptionData>,
}

impl ClientPacket for EncryptionResponse {
    fn get_type(&self) -> ClientPacketType {
        ClientPacketType::EncryptionResponse
    }
}

impl Packet for EncryptionResponse {
    fn decode<R: Read + ?Sized>(src: &mut R, version: i32) -> IOResult<Self>
    where
        Self: Sized,
    {
        let shared_secret = EncodingHelper::read_byte_array(src, 128)?;
        let verify_token;
        let encryption_data;
        if version < R1_19 || version >= R1_19_3 || src.read_u8()? != 0 {
            verify_token = Some(EncodingHelper::read_byte_array(src, 128)?);
            encryption_data = None;
        } else {
            verify_token = None;
            encryption_data = Some(EncryptionData {
                salt: src.read_i64::<BE>()?,
                signature: EncodingHelper::read_byte_array(src, 32767)?,
            });
        }
        Ok(Self {
            shared_secret,
            verify_token,
            encryption_data,
        })
    }

    fn encode<W: Write + ?Sized>(&self, dst: &mut W, version: i32) -> IOResult<()> {
        EncodingHelper::write_byte_array(dst, &self.shared_secret)?;
        if version >= R1_19 && version < R1_19_3 {
            if let Some(ref token) = self.verify_token {
                dst.write_u8(1)?;
                EncodingHelper::write_byte_array(dst, token)?;
            } else {
                dst.write_u8(0)?;
                let enc_data = self.encryption_data.as_ref().unwrap();
                dst.write_i64::<BE>(enc_data.salt)?;
                EncodingHelper::write_byte_array(dst, &enc_data.signature)?;
            }
        } else {
            let verify_token = self.verify_token.as_ref().unwrap();
            EncodingHelper::write_byte_array(dst, verify_token)?;
        }
        Ok(())
    }
}

pub struct EncryptionData {
    pub salt: i64,
    pub signature: Vec<u8>,
}

pub struct LoginSuccess {
    pub profile: GameProfile,
}

impl ServerPacket for LoginSuccess {
    fn get_type(&self) -> ServerPacketType {
        ServerPacketType::LoginSuccess
    }
}

impl Packet for LoginSuccess {
    fn decode<R: Read + ?Sized>(src: &mut R, version: i32) -> IOResult<Self>
    where
        Self: Sized,
    {
        let id = if version >= R1_16 {
            EncodingHelper::read_uuid(src)?.to_string()
        } else {
            let id = EncodingHelper::read_string(src, 36)?;
            Uuid::parse_str(&id).map_err(|_| IOError::new(IOErrorKind::InvalidData, "Failed to parse UUID"))?;
            id
        };
        let name = EncodingHelper::read_string(src, 16)?;
        let mut properties = Vec::new();
        if version >= R1_19 {
            let num_props = VarInt::decode(src, 5)?.get();
            for _ in 0..num_props {
                let name = EncodingHelper::read_string(src, 255)?;
                let value = EncodingHelper::read_string(src, 32767)?;
                let signature = if src.read_u8()? != 0 {
                    Some(EncodingHelper::read_string(src, 255)?)
                } else {
                    None
                };
                properties.push(Property { name, value, signature });
            }
        }
        if version >= R1_20_5 && version < R1_21_2 {
            src.read_u8()?; // whether the client should ignore corrupted packets from the server
        }

        Ok(Self {
            profile: GameProfile { id, name, properties },
        })
    }

    fn encode<W: Write + ?Sized>(&self, dst: &mut W, version: i32) -> IOResult<()> {
        if version >= R1_16 {
            EncodingHelper::write_uuid(dst, &Uuid::parse_str(&self.profile.id)
                .map_err(|_| IOError::new(IOErrorKind::InvalidData, "Failed to parse UUID"))?)?; // uuid
        } else {
            EncodingHelper::write_string(dst, &self.profile.id)?; // uuid
        }
        EncodingHelper::write_string(dst, &self.profile.name)?; // username
        if version >= R1_19 {
            VarInt(self.profile.properties.len() as i32).encode(dst, 5)?; // properties length
            for property in &self.profile.properties {
                EncodingHelper::write_string(dst, &property.name)?; // property name
                EncodingHelper::write_string(dst, &property.value)?; // property value
                dst.write_u8(property.signature.is_some() as u8)?; // has signature
                if let Some(ref sig) = property.signature {
                    EncodingHelper::write_string(dst, sig)?; // signature
                }
            }
        }
        if version >= R1_20_5 && version < R1_21_2 {
            dst.write_u8(1)?; // whether the client should ignore corrupted packets from the server
        }
        Ok(())
    }
}

pub struct LoginPluginRequest {
    pub id: i32,
    pub channel: String,
    pub data: Vec<u8>,
}

impl ServerPacket for LoginPluginRequest {
    fn get_type(&self) -> ServerPacketType {
        ServerPacketType::LoginPluginRequest
    }
}

impl Packet for LoginPluginRequest {
    fn decode<R: Read + ?Sized>(src: &mut R, _: i32) -> IOResult<Self>
    where
        Self: Sized,
    {
        let id = VarInt::decode(src, 5)?.get();
        let channel = EncodingHelper::read_string(src, 255)?;
        let mut data = Vec::new();
        src.read_to_end(&mut data)?;
        Ok(Self { id, channel, data })
    }

    fn encode<W: Write + ?Sized>(&self, dst: &mut W, _: i32) -> IOResult<()> {
        VarInt(self.id).encode(dst, 5)?;
        EncodingHelper::write_string(dst, &self.channel)?;
        dst.write_all(&self.data)?;
        Ok(())
    }
}

pub struct LoginPluginResponse {
    pub id: i32,
    pub data: Option<Vec<u8>>,
}

impl ClientPacket for LoginPluginResponse {
    fn get_type(&self) -> ClientPacketType {
        ClientPacketType::LoginPluginResponse
    }
}

impl Packet for LoginPluginResponse {
    fn decode<R: Read + ?Sized>(src: &mut R, _: i32) -> IOResult<Self>
    where
        Self: Sized,
    {
        let id = VarInt::decode(src, 5)?.get();
        let data = match src.read_u8()? {
            0 => None,
            _ => {
                let mut reader = RefSizeLimitedReader::new(src, 1048576);
                let mut data = Vec::new();
                reader.read_to_end(&mut data)?;
                Some(data)
            }
        };
        Ok(Self { id, data })
    }

    fn encode<W: Write + ?Sized>(&self, dst: &mut W, _: i32) -> IOResult<()> {
        VarInt(self.id).encode(dst, 5)?;
        match self.data {
            Some(ref data) => {
                dst.write_u8(1)?;
                dst.write_all(data)?;
            }
            None => {
                dst.write_u8(0)?;
            }
        }
        Ok(())
    }
}

pub struct CookieRequest {
    pub cookie: String,
}

impl ServerPacket for CookieRequest {
    fn get_type(&self) -> ServerPacketType {
        ServerPacketType::CookieRequest
    }
}

impl Packet for CookieRequest {
    fn decode<R: Read + ?Sized>(src: &mut R, _: i32) -> IOResult<Self>
    where
        Self: Sized,
    {
        Ok(Self { cookie: EncodingHelper::read_string(src, 32767)? })
    }

    fn encode<W: Write + ?Sized>(&self, dst: &mut W, _: i32) -> IOResult<()> {
        EncodingHelper::write_string(dst, &self.cookie)?;
        Ok(())
    }
}

pub struct CookieResponse {
    pub cookie: String,
    pub data: Option<Vec<u8>>,
}

impl ClientPacket for CookieResponse {
    fn get_type(&self) -> ClientPacketType {
        ClientPacketType::CookieResponse
    }
}

impl Packet for CookieResponse {
    fn decode<R: Read + ?Sized>(src: &mut R, _: i32) -> IOResult<Self>
    where
        Self: Sized,
    {
        let cookie = EncodingHelper::read_string(src, 32767)?;
        let data = match src.read_u8()? {
            0 => None,
            _ => Some(EncodingHelper::read_byte_array(src, 5120)?),
        };
        Ok(Self { cookie, data })
    }

    fn encode<W: Write + ?Sized>(&self, dst: &mut W, _: i32) -> IOResult<()> {
        EncodingHelper::write_string(dst, &self.cookie)?;
        match &self.data {
            Some(data) => {
                dst.write_u8(1)?;
                EncodingHelper::write_byte_array(dst, data)?;
            }
            None => dst.write_u8(0)?,
        }
        Ok(())
    }
}


pub struct ClientSettings {
    pub local: String,
    pub view_distance: i8,
    pub chat_flags: i32,
    pub chat_colours: bool,
    pub skin_parts: i8,
    pub main_hand: i32,
    pub disable_text_filtering: bool,
    pub allow_server_listing: bool,
    pub particel_status: i32,
}


impl ClientPacket for ClientSettings {
    fn get_type(&self) -> ClientPacketType {
        ClientPacketType::ClientSettings
    }
}

impl Packet for ClientSettings {
    fn decode<R: Read + ?Sized>(src: &mut R, version: i32) -> IOResult<Self>
    where
        Self: Sized,
    {
        let local = EncodingHelper::read_string(src, 16)?;
        let view_distance = src.read_i8()?;
        let chat_flags = if version > R1_9 {
            VarInt::decode_simple(src)?.get()
        } else {
            src.read_u8()? as i32
        };
        let chat_colours = src.read_u8()? != 0;
        let skin_parts = src.read_i8()?;
        let mut main_hand = 0;
        if version >= R1_9 {
            main_hand = VarInt::decode_simple(src)?.get();
        }
        let mut disable_text_filtering = false;
        if version >= R1_17 {
            disable_text_filtering = src.read_u8()? != 0;
        }

        let mut allow_server_listing = false;
        if version >= R1_18 {
            allow_server_listing = src.read_u8()? != 0;
        }
        let mut particel_status = 0;
        if version >= R1_21_2 {
            particel_status = VarInt::decode_simple(src)?.get();
        }
        Ok(ClientSettings {
            local,
            view_distance,
            allow_server_listing,
            chat_colours,
            chat_flags,
            disable_text_filtering,
            main_hand,
            particel_status,
            skin_parts,
        })
    }

    fn encode<W: Write + ?Sized>(&self, dst: &mut W, version: i32) -> IOResult<()> {
        EncodingHelper::write_string(dst, &self.local)?;
        dst.write_i8(self.view_distance)?;

        if version > R1_9 {
            VarInt(self.chat_flags).encode_simple(dst)?;
        } else {
            dst.write_u8(self.chat_flags as u8)?;
        }

        dst.write_u8(if self.chat_colours { 1 } else { 0 })?;
        dst.write_i8(self.skin_parts)?;
        if version >= R1_9 {
            VarInt(self.main_hand).encode_simple(dst)?;
        }
        if version >= R1_17 {
            dst.write_u8(if self.disable_text_filtering { 1 } else { 0 })?;
        }
        if version >= R1_18 {
            dst.write_u8(if self.allow_server_listing { 1 } else { 0 })?;
        }
        if version >= R1_21_2 {
            VarInt(self.particel_status).encode_simple(dst)?;
        }
        Ok(())
    }
}

pub struct UnsignedClientCommand {
    pub message: String,
}

impl ClientPacket for UnsignedClientCommand {
    fn get_type(&self) -> ClientPacketType {
        ClientPacketType::UnsignedClientCommand
    }
}

impl Packet for UnsignedClientCommand {
    fn decode<R: Read + ?Sized>(src: &mut R, _: i32) -> IOResult<Self>
    where
        Self: Sized,
    {
        Ok(UnsignedClientCommand {
            message: EncodingHelper::read_string(src, i16::MAX as usize)?
        })
    }

    fn encode<W: Write + ?Sized>(&self, dst: &mut W, _: i32) -> IOResult<()> {
        EncodingHelper::write_string(dst, &self.message)?;
        Ok(())
    }
}

pub struct SystemChatMessage {
    pub message: Text,
    pub pos: i32,
}

impl ServerPacket for SystemChatMessage {
    fn get_type(&self) -> ServerPacketType {
        ServerPacketType::SystemChatMessage
    }
}

impl Packet for SystemChatMessage {
    fn decode<R: Read + ?Sized>(src: &mut R, version: i32) -> IOResult<Self>
    where
        Self: Sized,
    {
        let nbt = nbt::read_networking_nbt(src, version)?;
        if let Some(nbt_tag) = nbt.left() {
            if let Some(nbt_tag) = nbt_tag {
                let json = nbt_tag.to_json();
                let text = crate::chat::deserialize_json(&json).map_err(|err| IOError::new(ErrorKind::InvalidData, err))?;
                let pos = if version >= R1_19_1 {
                    if src.read_u8()? != 0 { 2 } else { 0 }
                } else {
                    VarInt::decode(src, 5)?.get()
                };
                Ok(Self {
                    message: text,
                    pos,
                })
            } else {
                Err(IOError::new(ErrorKind::InvalidData, "invalid nbt type"))
            }
        } else {
            todo!()
        }
    }

    fn encode<W: Write + ?Sized>(&self, dst: &mut W, version: i32) -> IOResult<()> {
        let json = crate::chat::serialize_json(&self.message);
        let nbt = NbtType::from_json(&json)?;
        nbt::write_networking_nbt(dst, version, &Either::Left(Some(nbt)))?;
        if version >= R1_19_1 {
            dst.write_u8(self.pos as u8)?;
        } else {
            VarInt(self.pos).encode_simple(dst)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct TabCompleteRequest {
    pub transaction_id: Option<i32>,
    pub cursor: String,
    pub assume_command: Option<bool>,
    pub position: Option<i64>,
}

impl ClientPacket for TabCompleteRequest {
    fn get_type(&self) -> ClientPacketType {
        ClientPacketType::TabCompleteRequest
    }
}

impl Packet for TabCompleteRequest {

    fn decode<R: Read + ?Sized>(src: &mut R, version: i32) -> IOResult<Self>
        where
            Self: Sized {
        let transaction_id = if version >= R1_13 { Some(VarInt::decode(src, 5)?.get()) } else { None };
        let cursor = EncodingHelper::read_string(src, if version > R1_13 { 32500 } else if version == R1_13 { 256 } else { 32767 })?;
        let mut assume_command = None;
        let mut position = None;
        if version < R1_13 {
            if version >= R1_9 {
                assume_command = Some(src.read_u8()? != 0);
            }
            if src.read_u8()? != 0 {
                position = Some(src.read_i64::<BE>()?);
            }
        }
        Ok(Self {
            transaction_id,
            cursor,
            assume_command,
            position,
        })
    }

    fn encode<W: Write + ?Sized>(&self, dst: &mut W, version: i32) -> IOResult<()> {
        if version >= R1_13 {
            VarInt(self.transaction_id.unwrap()).encode_simple(dst)?;
        }
        EncodingHelper::write_string(dst, &self.cursor)?;
        if version < R1_13 {
            if version >= R1_9 {
                dst.write_u8(self.assume_command.unwrap() as u8)?;
            }
            if let Some(position) = self.position {
                dst.write_u8(1)?;
                dst.write_i64::<BE>(position)?;
            } else {
                dst.write_u8(0)?;
            }
        }
        Ok(())
    }
}

pub struct TabCompleteResponse {
    pub transaction_id: Option<i32>,
    pub suggestions: Option<Suggestions>,
    pub commands: Option<Vec<String>>,
}

impl ServerPacket for TabCompleteResponse {
    fn get_type(&self) -> ServerPacketType {
        ServerPacketType::TabCompleteResponse
    }
}

impl Packet for TabCompleteResponse {

    fn decode<R: Read + ?Sized>(src: &mut R, version: i32) -> IOResult<Self>
        where
            Self: Sized {
        let mut transaction_id = None;
        let mut suggestions = None;
        let mut commands = None;
        if version >= R1_13 {   
            transaction_id = Some(VarInt::decode_simple(src)?.get());
            suggestions = Some(Suggestions::decode(src, version)?);
        } else {
            let mut list = Vec::new();
            for _ in 0..VarInt::decode_simple(src)?.get() {
                list.push(EncodingHelper::read_string(src, 32767)?);
            }
            commands = Some(list);
        }
        Ok(Self {
            transaction_id,
            suggestions,
            commands,
        })
    }

    fn encode<W: Write + ?Sized>(&self, dst: &mut W, version: i32) -> IOResult<()> {
        if version >= R1_13 {
            VarInt(self.transaction_id.unwrap()).encode_simple(dst)?;
            self.suggestions.as_ref().unwrap().encode(dst, version)?;
        } else {
            let commands = self.commands.as_ref().unwrap();
            VarInt(commands.len() as i32).encode_simple(dst)?;
            for command in commands {
                EncodingHelper::write_string(dst, command)?;
            }
        }
        Ok(())
    }
}

pub async fn read_and_decode_packet<R: AsyncRead + Unpin + ?Sized>(src: &mut R, dest_buf: &mut Vec<u8>, temp_buf: &mut Vec<u8>, compression: i32, decryption: &mut Option<PacketDecryption>) -> IOResult<()> {
    tokio::time::timeout(PROTOCOL_READ_TIMEOUT, async move {
        let size = match decryption {
            Some(decrypt) => VarInt::decode_encrypted_async(src, 3, decrypt).await,
            None => VarInt::decode_async(src, 3).await,
        }?.get() as usize;

        temp_buf.clear();
        dest_buf.clear();
        if compression != -1 {
            temp_buf.resize(size, 0);
            src.read_exact(temp_buf).await?;
        } else {
            dest_buf.resize(size, 0);
            src.read_exact(dest_buf).await?;
        }

        if let Some(decrypt) = decryption {
            if compression != -1 {
                decrypt.decrypt(temp_buf);
            } else {
                decrypt.decrypt(dest_buf);
            }
        }

        if compression != -1 {
            super::compression::decompress(temp_buf, dest_buf)?;
        }
        temp_buf.clear();

        Ok::<_, IOError>(())
    }).await??;
    Ok(())
}

pub async fn encode_and_send_packet<W: AsyncWrite + Unpin + ?Sized>(dst: &mut W, write_buf: &[u8], temp_buf: &mut Vec<u8>,
                                                                    compression: i32, encryption: &mut Option<PacketEncryption>) -> IOResult<()> {
    temp_buf.clear();
    if compression >= 0 {
        super::compression::compress(write_buf, compression, temp_buf)?;
    } else {
        temp_buf.extend_from_slice(write_buf);
    }

    if let Some(encryption) = encryption {
        let mut varint_buf = [0u8; 3];
        let mut varint_writer = Cursor::new(varint_buf.as_mut_slice());
        let varint_len = VarInt(temp_buf.len() as i32).encode(&mut varint_writer, 3)?;

        encryption.encrypt(&mut varint_buf[..varint_len]);
        encryption.encrypt(temp_buf);

        dst.write_all(&varint_buf[..varint_len]).await?;
        dst.write_all(temp_buf).await?;
    } else {
        VarInt(temp_buf.len() as i32).encode_async(dst, 3).await?;
        dst.write_all(temp_buf).await?;
    }
    Ok(())
}