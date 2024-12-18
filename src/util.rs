use std::{
    io::{Read, Write},
    ops::{Deref, DerefMut},
};

use byteorder::{ReadBytesExt, WriteBytesExt};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;

use crate::server::encryption::PacketDecryption;

pub type DynError = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, DynError>;

pub type IOError = std::io::Error;
pub type IOErrorKind = std::io::ErrorKind;
pub type IOResult<T> = std::result::Result<T, IOError>;

#[inline]
pub fn generate_uuid(username: &str) -> Uuid {
    uuid::Builder::from_md5_bytes(
        md5::compute(format!("OfflinePlayer:{username}").as_bytes()).into(),
    )
    .into_uuid()
}

#[inline]
pub fn is_username_valid(username: &str) -> bool {
    !username.is_empty()
        && username.len() <= 16
        && username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub struct VarInt(pub i32);

impl VarInt {
    #[inline]
    pub fn get(&self) -> i32 {
        self.0
    }

    pub fn decode<R: Read + ?Sized>(src: &mut R, max_bytes: u32) -> IOResult<Self> {
        let mut out = 0u32;
        let mut bytes = 0;
        loop {
            let b = src.read_u8()? as u32;
            out |= (b & 0x7F) << (bytes * 7);
            bytes += 1;
            if bytes > max_bytes {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "VarInt too big",
                ));
            }
            if (b & 0x80) != 0x80 {
                break;
            }
        }
        Ok(Self(out as i32))
    }

    #[inline]
    pub fn decode_simple<R: Read + ?Sized>(src: &mut R) -> IOResult<Self> {
        Self::decode(src, 5)
    }

    #[inline]
    pub fn encode_simple<W: Write + ?Sized>(&self, dest: &mut W) -> IOResult<usize> {
        self.encode(dest, 5)
    }

    #[inline]
    pub fn encode<W: Write + ?Sized>(&self, dest: &mut W, max_bytes: u32) -> IOResult<usize> {
        let max_bytes = max_bytes as usize;
        let mut value = self.0 as u32;
        let mut part;
        let mut bytes = 0;
        loop {
            part = value & 0x7F;
            value >>= 7;
            if value != 0 {
                part |= 0x80;
            }
            dest.write_u8(part as u8)?;
            bytes += 1;
            if bytes > max_bytes {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "VarInt too big",
                ));
            }
            if value == 0 {
                break;
            }
        }
        Ok(bytes)
    }

    #[inline]
    pub async fn decode_async<R: AsyncRead + Unpin + ?Sized>(
        src: &mut R,
        max_bytes: u32,
    ) -> IOResult<Self> {
        let mut out = 0u32;
        let mut bytes = 0;
        loop {
            let b = src.read_u8().await? as u32;
            out |= (b & 0x7F) << (bytes * 7);
            bytes += 1;
            if bytes > max_bytes {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "VarInt too big",
                ));
            }
            if (b & 0x80) != 0x80 {
                break;
            }
        }
        Ok(Self(out as i32))
    }

    #[inline]
    pub async fn decode_encrypted_async<R: AsyncRead + Unpin + ?Sized>(
        src: &mut R,
        max_bytes: u32,
        decrypt: &mut PacketDecryption,
    ) -> IOResult<Self> {
        let mut out = 0u32;
        let mut bytes = 0;
        let mut buf = [0u8];
        loop {
            src.read_exact(&mut buf).await?;
            decrypt.decrypt(&mut buf);
            let b = buf[0] as u32;
            out |= (b & 0x7F) << (bytes * 7);
            bytes += 1;
            if bytes > max_bytes {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "VarInt too big",
                ));
            }
            if (b & 0x80) != 0x80 {
                break;
            }
        }
        Ok(Self(out as i32))
    }

    #[inline]
    pub async fn encode_async<W: AsyncWrite + Unpin + ?Sized>(
        &self,
        dest: &mut W,
        max_bytes: u32,
    ) -> IOResult<usize> {
        let mut value = self.0 as u32;
        let mut part;
        let mut bytes = 0;
        loop {
            part = value & 0x7F;
            value >>= 7;
            if value != 0 {
                part |= 0x80;
            }
            dest.write_u8(part as u8).await?;
            bytes += 1;
            if bytes > max_bytes {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "VarInt too big",
                ));
            }
            if value == 0 {
                break;
            }
        }
        Ok(bytes as usize)
    }

    #[inline]
    pub fn get_size(v: i32) -> usize {
        let v = v as u32;
        if (v & 0xFFFFFF80) == 0 {
            return 1;
        }
        if (v & 0xFFFFC000) == 0 {
            return 2;
        }
        if (v & 0xFFE00000) == 0 {
            return 3;
        }
        if (v & 0xF0000000) == 0 {
            return 4;
        }
        5
    }
}

impl Deref for VarInt {
    type Target = i32;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for VarInt {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> From<T> for VarInt
where
    T: Into<i32>,
{
    #[inline]
    fn from(value: T) -> Self {
        VarInt(value.into())
    }
}

pub struct EncodingHelper(());

impl EncodingHelper {
    
    #[inline]
    pub fn write_byte_array<W: Write + ?Sized>(dest: &mut W, data: &[u8]) -> IOResult<()> {
        let len = VarInt(data.len() as i32);
        len.encode(dest, 5)?;
        dest.write_all(data)?;
        Ok(())
    }
    
    #[inline]
    pub fn read_byte_array<R: Read + ?Sized>(src: &mut R, max_length: usize) -> IOResult<Vec<u8>> {
        let len = VarInt::decode(src, 5)?.get() as usize;
        if len > max_length {
            return Err(IOError::new(IOErrorKind::InvalidData, "Byte array too big"));
        }
        let mut data = vec![0; len];
        src.read_exact(&mut data)?;
        Ok(data)
    }
    
    #[inline]
    pub fn write_string<W: Write + ?Sized>(dest: &mut W, data: &str) -> IOResult<()> {
        let data = data.as_bytes();
        VarInt(data.len() as i32).encode_simple(dest)?;
        dest.write_all(data)?;
        Ok(())
    }
    
    #[inline]
    pub fn read_string<R: Read + ?Sized>(src: &mut R, max_length: usize) -> IOResult<String> {
        let len = VarInt::decode(src, 5)?.get() as usize;
        if len > max_length * 3 {
            return Err(IOError::new(IOErrorKind::InvalidData, "String too big"));
        }
        let mut data = vec![0; len];
        src.read_exact(&mut data)?;
        String::from_utf8(data).map_err(|e| IOError::new(IOErrorKind::InvalidData, e))
    }
    
    #[inline]
    pub fn write_uuid<W: Write + ?Sized>(dest: &mut W, uuid: &Uuid) -> IOResult<()> {
        dest.write_all(uuid.as_bytes())?;
        Ok(())
    }
    
    #[inline]
    pub fn read_uuid<R: Read + ?Sized>(src: &mut R) -> IOResult<Uuid> {
        let mut data = [0; 16];
        src.read_exact(&mut data)?;
        Ok(Uuid::from_bytes(data))
    }
}
