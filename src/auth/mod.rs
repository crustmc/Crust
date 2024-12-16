use std::net::IpAddr;

use serde::{Deserialize, Serialize};

use crate::{server::ProxyServer, util::{IOError, IOErrorKind, IOResult}};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameProfile {
    pub id: String,
    pub name: String,
    pub properties: Vec<Property>,
}

impl GameProfile {

    pub fn clone_without_properties(&self) -> Self {
        Self {
            id: self.id.clone(),
            name: self.name.clone(),
            properties: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Property {
    pub name: String,
    pub value: String,
    pub signature: Option<String>,
}

pub async fn has_joined(name: &str, server_id: &str, secret_key: &[u8; 16], ip: Option<IpAddr>) -> IOResult<Option<GameProfile>> {
    let name = urlencoding::encode(name);
    let server_id = server_hash(server_id, secret_key);
    let server_id = urlencoding::encode(&server_id);
    let ip = ip.map(|ip| urlencoding::encode(&ip.to_string()).into_owned());
    let url = match ip {
        Some(ip) => format!("https://sessionserver.mojang.com/session/minecraft/hasJoined?username={}&serverId={}&ip={}", name, server_id, ip),
        None => format!("https://sessionserver.mojang.com/session/minecraft/hasJoined?username={}&serverId={}", name, server_id),
    };
    let response = reqwest::get(url).await
        .map_err(|e| IOError::new(IOErrorKind::Other, format!("Failed to send HTTP request: {}", e)))?;

    if response.status().is_success() {
        let profile = response.bytes().await
            .map_err(|e| IOError::new(IOErrorKind::Other, format!("Failed to read response body: {}", e)))?;
        let profile = serde_json::from_slice(&profile)
            .map_err(|e| IOError::new(IOErrorKind::Other, format!("Failed to parse response body: {}", e)))?;
        return Ok(Some(profile));
    }
    Ok(None)
}

fn server_hash(server_id: &str, secret_key: &[u8; 16]) -> String {
    use digest::Digest;
    use rsa::pkcs8::EncodePublicKey;
    let mut hasher = sha1::Sha1::new();
    hasher.update(server_id.as_bytes());
    hasher.update(secret_key);
    hasher.update(ProxyServer::instance().rsa_public_key().to_public_key_der().unwrap());

    let hash = hasher.finalize();
    let hash_bigint = num_bigint::BigInt::from_signed_bytes_be(&hash);
    hash_bigint.to_str_radix(16)
}
