use std::path::Path;

use api::API;
use crust_plugin_sdk::{PluginEntryPointFunc, PluginMetadata, PluginQueryMetadataFunc, PLUGIN_ENTRY_POINT_SYMBOL_NAME, PLUGIN_QUERY_METADATA_SYMBOL_NAME};
use libloading::Library;

pub mod api;

pub const MIN_SUPPORTED_SDK_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub authors: Vec<String>,
    pub description: String,
}

pub struct PluginManager {
    plugins: Vec<Plugin>,
}

static mut PLUGIN_MANAGER: Option<PluginManager> = None;

impl PluginManager {

    #[inline]
    pub fn instance() -> &'static PluginManager {
        unsafe {
            #[allow(static_mut_refs)]
            PLUGIN_MANAGER.as_ref().expect("PluginManager is not initialized")
        }
    }

    pub fn load_plugins() -> bool {
        unsafe {
            PLUGIN_MANAGER = Some(PluginManager {
                plugins: Vec::new(),
            });
        }
        let plugins_dir = Path::new("plugins");
        if !plugins_dir.exists() {
            if let Err(e) = std::fs::create_dir("plugins") {
                log::error!("Failed to create plugins directory: {}", e);
            }
        }
        let rd = match plugins_dir.read_dir() {
            Ok(r) => r,
            Err(e) => {
                log::error!("Failed to read plugins directory: {}", e);
                return false;
            },
        };
        for entry in rd {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    log::error!("Failed to read entry in plugins directory, skipping this entry: {}", e);
                    continue;
                },
            };
            let path = entry.path();
            if let Some(extension) = path.extension() {
                if extension == std::env::consts::DLL_EXTENSION {
                    unsafe {
                        #[allow(static_mut_refs)]
                        let pm = PLUGIN_MANAGER.as_mut().unwrap();
                        let res = Self::load_plugin(path.as_ref());
                        let plugin = match res {
                            Ok(p) => p,
                            Err(e) => {
                                log::error!("FATAL: Failed to load plugin '{}': {}", path.display(), e);
                                return false;
                            },
                        };
                        log::info!("Loaded plugin: {}", plugin.info.name);
                        pm.plugins.push(plugin);
                    }
                }
            }
        }
        true
    }

    unsafe fn load_plugin(path: &Path) -> Result<Plugin, Box<dyn std::error::Error>> {
        let library = Library::new(path)?;
        let query_metadata = library.get::<PluginQueryMetadataFunc>(PLUGIN_QUERY_METADATA_SYMBOL_NAME.as_bytes())
            .map_err(|e| format!("Failed to get symbol '{}': {}", PLUGIN_QUERY_METADATA_SYMBOL_NAME, e))?;
        let entry_point = library.get::<PluginEntryPointFunc>(PLUGIN_ENTRY_POINT_SYMBOL_NAME.as_bytes())
            .map_err(|e| format!("Failed to get symbol '{}': {}", PLUGIN_ENTRY_POINT_SYMBOL_NAME, e))?;

        let mut metadata = PluginMetadata::default();
        if !query_metadata(&mut metadata) {
            return Err("Plugin rejected metadata query".into());
        }

        let name = match metadata.name() {
            Some(Ok(n)) => n.to_string(),
            _ => return Err("Plugin didn't provide a valid name".into()),
        };
        let version = match metadata.version() {
            Some(Ok(v)) => v.to_string(),
            _ => return Err("Plugin didn't provide a valid version".into()),
        };
        let authors = match metadata.authors().map(|a| a.map(|a| a.to_string())).collect::<Result<Vec<String>, _>>()
            .map_err(|e| format!("Failed to get authors: {}", e)) {
            Ok(a) => a,
            Err(e) => return Err(e.into()),
        };
        let description = match metadata.description() {
            Some(Ok(d)) => d.to_string(),
            _ => return Err("Plugin didn't provide a valid description".into()),
        };

        if !entry_point(&API) {
            return Err(format!("Failed to initialize plugin '{}'", name).into());
        }

        let plugin = Plugin {
            _library: library,
            info: PluginInfo {
                name,
                version,
                authors,
                description,
            },
        };

        log::info!("PluginInfo: {:#?}", plugin.info);

        Ok(plugin)
    }
}

pub struct Plugin {
    _library: Library,
    info: PluginInfo,
}
