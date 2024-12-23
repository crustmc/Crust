use std::ffi::c_void;

use paste::paste;

use crate::PluginApi;

#[repr(C)]
pub struct LPluginMetadata {
    pub sdk_version: u32,
    pub name: *const u8,
    pub version: *const u8,
    pub description: *const u8,
    pub authors: *const *const u8,
    pub authors_count: usize,
}

impl Default for LPluginMetadata {
    fn default() -> Self {
        Self {
            sdk_version: 0,
            name: std::ptr::null(),
            version: std::ptr::null(),
            description: std::ptr::null(),
            authors: std::ptr::null(),
            authors_count: 0,
        }
    }
}

pub type PlayerHandle = *const c_void;

/// Return `true` to continue enumeration, `false` to stop.
pub type EnumeratePlayersCallback = extern "C" fn(player: PlayerHandle, user_data: *const c_void) -> bool;

macro_rules! define_plugin_api {
    ($( ($fn_name:ident, $typedef_name:ident, fn ( $($arg_name:ident: $arg_typ:ty),* ) -> $ret_ty:ty ) )*) => {
        paste! {
            $(
                pub type [<APIFn $typedef_name>] = extern "C" fn($($arg_typ),*) -> $ret_ty;
            )*

            #[derive(Debug, Clone, Copy)]
            #[repr(C)]
            pub struct LPluginApi {
                $(pub $fn_name: [<APIFn $typedef_name>],)*
            }
        }
    };
}

define_plugin_api! {
    // SDK Version 1
    (shutdown_proxy, ShutdownProxy, fn(reason: *const u8, reason_len: usize) -> !)
    (enumerate_players, EnumeratePlayers, fn(callback: EnumeratePlayersCallback, user_data: *const c_void) -> bool)
}

impl LPluginApi {

    #[allow(dead_code)]
    pub const fn into_plugin_api(self) -> PluginApi {
        PluginApi {
            inner: self,
        }
    }
}
