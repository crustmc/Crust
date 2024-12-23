use crust_plugin_sdk::{PluginApi, PluginMetadata, PLUGIN_SDK_VERSION};

pub const PLUGIN_NAME: &str = "Crust Example Plugin\0";
pub const PLUGIN_VERSION: &str = "0.1.0\0";
pub const PLUGIN_AUTHORS: &[&str] = &["Your Name\0"];
pub const PLUGIN_DESCRIPTION: &str = "A simple example plugin for Crust\0";

static mut API_INSTANCE: Option<PluginApi> = None;

#[inline]
pub fn api() -> &'static PluginApi {
    unsafe {
        #[allow(static_mut_refs)]
        API_INSTANCE.as_ref().expect("API_INSTANCE is not initialized")
    }
}

#[no_mangle]
pub extern "C" fn CrustPlugin_QueryMetadata(metadata: &mut PluginMetadata) -> bool {
    metadata.set_sdk_version(PLUGIN_SDK_VERSION);
    metadata.set_name(PLUGIN_NAME);
    metadata.set_version(PLUGIN_VERSION);
    metadata.set_authors(PLUGIN_AUTHORS);
    metadata.set_description(PLUGIN_DESCRIPTION);
    true
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
