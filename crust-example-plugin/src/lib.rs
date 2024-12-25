use crust_plugin_sdk::{PluginApi, PLUGIN_SDK_VERSION, lowlevel::{PluginMetadata, PtrWrapper}};

const fn get_plugin_metadata() -> PluginMetadata {
    let plugin_manifest = include_str!("../crust-plugin.json");
    PluginMetadata {
        sdk_version: PLUGIN_SDK_VERSION,
        manifest: PtrWrapper::new(plugin_manifest.as_bytes().as_ptr()),
        manifest_len: plugin_manifest.len(),
    }
}

static PLUGIN_METADATA: PluginMetadata = get_plugin_metadata();

static mut API_INSTANCE: Option<PluginApi> = None;

#[inline]
pub fn api() -> &'static PluginApi {
    unsafe {
        #[allow(static_mut_refs)]
        API_INSTANCE.as_ref().expect("API_INSTANCE is not initialized")
    }
}

#[no_mangle]
pub extern "C" fn CrustPlugin_QueryMetadata() -> *const PluginMetadata {
    &PLUGIN_METADATA as *const PluginMetadata
}

#[no_mangle]
pub extern "C" fn CrustPlugin_EntryPoint(plugin_api: &PluginApi) -> bool {
    #[allow(static_mut_refs)]
    unsafe { API_INSTANCE = Some(*plugin_api) };
    println!("Crust Example Plugin loaded!");
    std::thread::spawn(|| {
        loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            println!("Crust Example Plugin is running!");
            api().enumerate_players(|| {
                println!("hackacka");
                true
            });
        }
    });
    true
}
