use aes::{cipher::{inout::InOutBuf, BlockDecryptMut, BlockEncryptMut, KeyIvInit}, Aes128};

pub struct PacketEncryption {
    cipher: cfb8::Encryptor<Aes128>,
}

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

pub struct PacketDecryption {
    cipher: cfb8::Decryptor<Aes128>,
}

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
