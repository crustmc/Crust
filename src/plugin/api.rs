use std::ffi::c_void;

use crust_plugin_sdk::{lowlevel::{EnumeratePlayersCallback, LPluginApi}, PluginApi};
use crate::chat::Text;
use crate::server::ProxyServer;

pub static API: PluginApi = LPluginApi {
    shutdown_proxy,
    enumerate_players,
}.into_plugin_api();

extern "C" fn shutdown_proxy(reason: *const u8, reason_len: usize) -> ! {
    if reason.is_null() {
        log::info!("Shutting down...");
        ProxyServer::instance().block_on( async {
            for (_, player) in ProxyServer::instance().players().read().await.iter() {
                player.kick(Text::new("Crust shutdown")).await.ok();
            }
        });
    } else {
        let reason = unsafe { std::str::from_utf8(std::slice::from_raw_parts(reason, reason_len)).unwrap() };
        log::info!("Shutting down: {}", reason);
        ProxyServer::instance().block_on( async {
            for (_, player) in ProxyServer::instance().players().read().await.iter() {
                player.kick(Text::new(reason)).await.ok();
            }
        });
    }

    std::process::exit(0);
}

extern "C" fn enumerate_players(callback: EnumeratePlayersCallback, user_data: *const c_void) -> bool {
    let players = ProxyServer::instance().players().blocking_read();
    players.values().all(|_| callback(std::ptr::null(), user_data))
}
