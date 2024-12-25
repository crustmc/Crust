#[cfg(not(target_os = "windows"))]
use openssl::symm::{Cipher, Crypter, Mode};
#[cfg(not(target_os = "windows"))]
pub struct PacketEncryption {
    cipher: Crypter,
}
#[cfg(not(target_os = "windows"))]
impl PacketEncryption {
    pub fn new(key: &[u8; 16]) -> Self {
        Self {
            cipher: Crypter::new(
                Cipher::aes_128_cfb8(),
                Mode::Encrypt,
                key,
                Some(key),
            )
            .unwrap(),
        }
    }
    #[warn(clippy::transmute_ptr_to_ref)]
    pub fn encrypt(&mut self, data: &mut [u8]) {
        unsafe {
            self.cipher.update(
                core::mem::transmute::<_, &mut [u8]>(data as *mut [u8]),
                data,
            ).unwrap();
        }
    }
}
#[cfg(not(target_os = "windows"))]
pub struct PacketDecryption {
    cipher: Crypter
}
#[cfg(not(target_os = "windows"))]
impl PacketDecryption {
    
    pub fn new(key: &[u8; 16]) -> Self {
        Self {
            cipher: Crypter::new(
                Cipher::aes_128_cfb8(),
                Mode::Decrypt,
                key,
                Some(key),
            ).unwrap(),
        }
    }

    #[warn(clippy::transmute_ptr_to_ref)]
    pub fn decrypt(&mut self, data: &mut [u8]) {
        unsafe {
            self.cipher.update(
                core::mem::transmute::<_, &mut [u8]>(data as *mut [u8]),
                data,
            ).unwrap();
        }
    }
}


#[cfg(target_os = "windows")]
use aes::{cipher::{inout::InOutBuf, BlockDecryptMut, BlockEncryptMut, KeyIvInit}, Aes128};
#[cfg(target_os = "windows")]
pub struct PacketEncryption {
    cipher: cfb8::Encryptor<Aes128>,
}
#[cfg(target_os = "windows")]
impl PacketEncryption {
    pub fn new(key: &[u8; 16]) -> Self {
        Self {
            cipher: cfb8::Encryptor::new(key.into(), key.into()),
        }
    }
    pub fn encrypt(&mut self, data: &mut [u8]) {
        let (in_out, _) = InOutBuf::from(data).into_chunks();
        self.cipher.encrypt_blocks_inout_mut(in_out);
    }
}
#[cfg(target_os = "windows")]
pub struct PacketDecryption {
    cipher: cfb8::Decryptor<Aes128>,
}
#[cfg(target_os = "windows")]
impl PacketDecryption {
    pub fn new(key: &[u8; 16]) -> Self {
        Self {
            cipher: cfb8::Decryptor::new(key.into(), key.into()),
        }
    }

    pub fn decrypt(&mut self, data: &mut [u8]) {
        let (in_out, _) = InOutBuf::from(data).into_chunks();
        self.cipher.decrypt_blocks_inout_mut(in_out);
    }
}