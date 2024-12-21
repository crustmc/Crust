use openssl::symm::{Cipher, Crypter, Mode};

pub struct PacketEncryption {
    cipher: Crypter,
}

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
pub struct PacketDecryption {
    cipher: Crypter
}

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