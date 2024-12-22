use std::fmt::Display;
use serde::Serialize;

use super::ProxyServer;

pub fn get_status_response(client_version: i32) -> StatusResponse {
    StatusResponse {
        version: Version {
            name: format!("{} {}", crate::server::NAME, crate::version::SUPPORTED_VERSION_RANGE),
            protocol: if crate::version::is_supported(client_version) { client_version } else { -1 },
        },
        players: Players {
            max: ProxyServer::instance().config().max_players,
            online: ProxyServer::instance().player_count as i32,
            sample: None,
        },
        description: Some(ProxyServer::instance().config().motd.clone()),
        favicon: ProxyServer::instance().favicon.clone(),
    }
}

#[derive(Debug, Serialize)]
pub struct StatusResponse {
    pub version: Version,
    pub players: Players,
    pub description: Option<String>,
    pub favicon: Option<String>,
}

impl Display for StatusResponse {
    
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", serde_json::to_string(self).unwrap())
    }
}

#[derive(Debug, Serialize)]
pub struct Version {
    pub name: String,
    pub protocol: i32,
}

#[derive(Debug, Serialize)]
pub struct Players {
    pub max: i32,
    pub online: i32,
    pub sample: Option<Vec<Player>>,
}

#[derive(Debug, Serialize)]
pub struct Player {
    pub name: String,
    pub id: String,
}
