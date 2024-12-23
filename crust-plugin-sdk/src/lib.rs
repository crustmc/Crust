use std::ffi::{c_void, CStr};

use lowlevel::{LPluginApi, LPluginMetadata, PlayerHandle};


#[cfg(feature = "lowlevel")]
pub mod lowlevel;
#[cfg(not(feature = "lowlevel"))]
mod lowlevel;

pub type PluginQueryMetadataFunc = extern "C" fn(&mut PluginMetadata) -> bool;
pub const PLUGIN_QUERY_METADATA_SYMBOL_NAME: &str = "CrustPlugin_QueryMetadata";
pub type PluginEntryPointFunc = extern "C" fn(&PluginApi) -> bool;
pub const PLUGIN_ENTRY_POINT_SYMBOL_NAME: &str = "CrustPlugin_EntryPoint";

pub const PLUGIN_SDK_VERSION: u32 = 1;

#[derive(Default)]
#[repr(C)]
pub struct PluginMetadata {
    inner: LPluginMetadata,
}

impl PluginMetadata {

    pub fn sdk_version(&self) -> u32 {
        self.inner.sdk_version
    }

    pub fn set_sdk_version(&mut self, sdk_version: u32) {
        self.inner.sdk_version = sdk_version;
    }

    pub unsafe fn name(&self) -> Option<Result<&str, std::str::Utf8Error>> {
        if self.inner.name.is_null() {
            None
        } else {
            Some(CStr::from_ptr(self.inner.name as *const i8).to_str())
        }
    }

    pub fn set_name(&mut self, name: &'static str) {
        self.inner.name = name.as_ptr() as *const u8;
    }

    pub unsafe fn version(&self) -> Option<Result<&str, std::str::Utf8Error>> {
        if self.inner.version.is_null() {
            None
        } else {
            Some(CStr::from_ptr(self.inner.version as *const i8).to_str())
        }
    }

    pub fn set_version(&mut self, version: &'static str) {
        self.inner.version = version.as_ptr() as *const u8;
    }

    pub unsafe fn description(&self) -> Option<Result<&str, std::str::Utf8Error>> {
        if self.inner.description.is_null() {
            None
        } else {
            Some(CStr::from_ptr(self.inner.description as *const i8).to_str())
        }
    }

    pub fn set_description(&mut self, description: &'static str) {
        self.inner.description = description.as_ptr() as *const u8;
    }

    pub fn authors_count(&self) -> usize {
        self.inner.authors_count
    }

    pub unsafe fn authors(&self) -> impl Iterator<Item = Result<&str, std::str::Utf8Error>> {
        (0..self.authors_count()).map(move |i| {
            CStr::from_ptr(*self.inner.authors.add(i) as *const i8).to_str()
        })
    }

    pub fn set_authors(&mut self, authors: &'static [&'static str]) {
        self.inner.authors_count = authors.len();
        self.inner.authors = authors.as_ptr() as *const *const u8;
    }
}

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
