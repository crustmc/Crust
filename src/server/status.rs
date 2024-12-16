use serde::Serialize;

use super::ProxyServer;

pub fn get_status_response(client_version: i32) -> StatusResponse {
    StatusResponse {
        version: Version {
            name: "Bertycord 1.20.2 - 1.21.4".to_string(),
            protocol: if crate::version::is_supported(client_version) { client_version } else { -1 },
        },
        players: Players {
            max: 100,
            online: 0,
            sample: None,
        },
        description: Some("Hello, world!".to_string()),
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

impl ToString for StatusResponse {

    fn to_string(&self) -> String {
        serde_json::to_string(self).unwrap()
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
