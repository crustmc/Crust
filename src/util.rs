use std::{
    io::{Read, Write},
    ops::{Deref, DerefMut},
    sync::{Arc, Weak},
};

use byteorder::{ReadBytesExt, WriteBytesExt};
use either::Either;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;

use crate::{
    chat::Text,
    server::{encryption::PacketDecryption, nbt::NbtType},
};

pub type DynError = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, DynError>;

pub type IOError = std::io::Error;
pub type IOErrorKind = std::io::ErrorKind;
pub type IOResult<T> = std::result::Result<T, IOError>;

#[macro_export]
macro_rules! hash_map {
    ($($key:expr => $val:expr),*) => {
        {
            let mut map = std::collections::HashMap::new();
            $(map.insert($key, $val);)*
            map
        }
    }
}




//pub fn uuid_from_str(str: &str) -> Result<Uuid> {
//    if str.contains('-') {
//        Ok(Uuid::parse_str(str)?.hyphenated())
//    } else {
//        
//    }
//}

pub fn generate_uuid(username: &str) -> Uuid {
    uuid::Builder::from_md5_bytes(
        md5::compute(format!("OfflinePlayer:{username}").as_bytes()).into(),
    )
    .into_uuid()
}

pub fn is_username_valid(username: &str) -> bool {
    !username.is_empty()
        && username.len() <= 16
        && username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub struct VarInt(pub i32);

impl VarInt {
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

    pub fn decode_simple<R: Read + ?Sized>(src: &mut R) -> IOResult<Self> {
        Self::decode(src, 5)
    }

    pub fn encode_simple<W: Write + ?Sized>(&self, dest: &mut W) -> IOResult<usize> {
        self.encode(dest, 5)
    }

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

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for VarInt {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> From<T> for VarInt
where
    T: Into<i32>,
{
    fn from(value: T) -> Self {
        VarInt(value.into())
    }
}

pub struct EncodingHelper(());

impl EncodingHelper {
    pub fn write_byte_array<W: Write + ?Sized>(dest: &mut W, data: &[u8]) -> IOResult<()> {
        let len = VarInt(data.len() as i32);
        len.encode(dest, 5)?;
        dest.write_all(data)?;
        Ok(())
    }

    pub fn read_byte_array<R: Read + ?Sized>(src: &mut R, max_length: usize) -> IOResult<Vec<u8>> {
        let len = VarInt::decode(src, 5)?.get() as usize;
        if len > max_length {
            return Err(IOError::new(IOErrorKind::InvalidData, "Byte array too big"));
        }
        let mut data = Self::need_read_uninit_vec(len);
        src.read_exact(&mut data)?;
        Ok(data)
    }

    pub fn write_string<W: Write + ?Sized>(dest: &mut W, data: &str) -> IOResult<()> {
        let data = data.as_bytes();
        VarInt(data.len() as i32).encode_simple(dest)?;
        dest.write_all(data)?;
        Ok(())
    }

    pub fn read_string<R: Read + ?Sized>(src: &mut R, max_length: usize) -> IOResult<String> {
        let len = VarInt::decode(src, 5)?.get() as usize;
        if len > max_length * 3 {
            return Err(IOError::new(IOErrorKind::InvalidData, "String too big"));
        }
        let mut data = Self::need_read_uninit_vec(len);
        src.read_exact(&mut data)?;
        String::from_utf8(data).map_err(|e| IOError::new(IOErrorKind::InvalidData, e))
    }

    pub fn read_text<R: Read + ?Sized>(src: &mut R, version: i32) -> IOResult<Text> {
        let nbt = crate::server::nbt::read_networking_nbt(src, version)?;
        if let Some(nbt_tag) = nbt.left() {
            if let Some(nbt_tag) = nbt_tag {
                let json = nbt_tag.to_json();
                return crate::chat::deserialize_json(&json)
                    .map_err(|err| IOError::new(IOErrorKind::InvalidData, err));
            }
        }
        Err(IOError::new(
            IOErrorKind::InvalidData,
            "Failed to parse text component: Invalid NBT data!",
        ))
    }

    pub fn write_text<W: Write + ?Sized>(dest: &mut W, version: i32, text: &Text) -> IOResult<()> {
        let json = crate::chat::serialize_json(text);
        let nbt = NbtType::from_json(&json)?;
        crate::server::nbt::write_networking_nbt(dest, version, &Either::Left(Some(nbt)))?;
        Ok(())
    }

    pub fn write_uuid<W: Write + ?Sized>(dest: &mut W, uuid: &Uuid) -> IOResult<()> {
        dest.write_all(uuid.as_bytes())?;
        Ok(())
    }

    pub fn read_uuid<R: Read + ?Sized>(src: &mut R) -> IOResult<Uuid> {
        let mut data = [0; 16];
        src.read_exact(&mut data)?;
        Ok(Uuid::from_bytes(data))
    }

    #[inline(always)]
    #[allow(clippy::uninit_vec)]
    pub fn need_read_uninit_vec(len: usize) -> Vec<u8> {
        let mut data = Vec::with_capacity(len);
        unsafe {
            data.set_len(len);
        }
        data
    }
}

pub struct Handle<T> {
    inner: Arc<T>,
}

impl<T> Handle<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner: Arc::new(inner),
        }
    }

    pub fn downgrade(&self) -> WeakHandle<T> {
        WeakHandle::new(Arc::downgrade(&self.inner))
    }

    #[allow(clippy::mut_from_ref)]
    pub fn get_mut(&self) -> &mut T {
        #[allow(invalid_reference_casting)]
        unsafe {
            &mut *core::mem::transmute::<*const T, *mut T>(self.inner.deref() as *const T)
        }
    }
}

#[allow(clippy::from_over_into)]
impl<T> Into<Arc<T>> for Handle<T> {
    fn into(self) -> Arc<T> {
        self.inner
    }
}

impl<T> From<Arc<T>> for Handle<T> {
    fn from(inner: Arc<T>) -> Self {
        Self { inner }
    }
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T> Deref for Handle<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> DerefMut for Handle<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.get_mut()
    }
}

pub struct WeakHandle<T> {
    inner: Weak<T>,
}

impl<T> WeakHandle<T> {
    pub fn new(inner: Weak<T>) -> Self {
        Self { inner }
    }

    pub fn upgrade(&self) -> Option<Handle<T>> {
        self.inner.upgrade().map(|inner| Handle { inner })
    }
}

impl<T> Clone for WeakHandle<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}
