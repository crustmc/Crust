use std::ffi::c_void;

use lowlevel::{LPluginApi, PlayerHandle};

pub mod lowlevel;

pub const PLUGIN_SDK_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct PluginApi {
    inner: LPluginApi,
}

impl PluginApi {
    pub fn shutdown_proxy(&self, reason: Option<&str>) -> ! {
        match reason {
            Some(reason) => (self.inner.shutdown_proxy)(reason.as_ptr() as *const u8, reason.len()),
            None => (self.inner.shutdown_proxy)(std::ptr::null(), 0),
        }
    }

    pub fn enumerate_players<F: FnMut() -> bool>(&self, mut callback: F) -> bool {
        let convert1: &mut dyn FnMut() -> bool = &mut callback;
        let convert2 = &convert1;
        let ud = convert2 as *const _ as *const c_void;

        extern "C" fn _callback(_player: PlayerHandle, user_data: *const c_void) -> bool {
            let callback = unsafe { &mut *(user_data as *mut &mut dyn FnMut() -> bool) };
            callback()
        }

        (self.inner.enumerate_players)(_callback, ud)
    }
}
