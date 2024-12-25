use std::ffi::c_void;

use paste::paste;

use crate::PluginApi;

#[repr(C)]
#[derive(Debug, Clone)]
pub struct PluginMetadata {
    pub sdk_version: u32,
    pub manifest: PtrWrapper<u8>,
    pub manifest_len: usize,
}

#[repr(C)]
#[derive(Debug, Clone)]
pub struct PtrWrapper<T>(*const T);

unsafe impl<T> Send for PtrWrapper<T> {}
unsafe impl<T> Sync for PtrWrapper<T> {}

impl<T> PtrWrapper<T> {
    
    pub const fn new(ptr: *const T) -> Self {
        Self(ptr)
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
