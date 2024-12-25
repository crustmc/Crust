use std::path::Path;

use api::PluginMetadata;
use rsa::pkcs8::der::zeroize::Zeroize;
use serde::Deserialize;
use wasmer::{Imports, Instance, Module, Store, TypedFunction, WasmPtr};

pub mod api;

pub const MIN_SUPPORTED_SDK_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize)]
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
                if extension == "wasm" {
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
        let mut store = Store::default();
        let module = Module::from_file(&store, path)?;

        let instance = Instance::new(&mut store, &module, &Imports::default())?;

        let query_metadata: TypedFunction<(), WasmPtr<u8>> = instance.exports.get_typed_function(&store, "CrustPlugin_QueryMetadata")
            .map_err(|e| format!("Failed to get symbol 'CrustPlugin_QueryMetadata': {}", e))?;
        let entry_point: TypedFunction<WasmPtr<u8>, i8> = instance.exports.get_typed_function(&store, "CrustPlugin_EntryPoint")
            .map_err(|e| format!("Failed to get symbol 'CrustPlugin_EntryPoint': {}", e))?;

        let memory = instance.exports.get_memory("memory").map_err(|e| format!("Failed to get memory: {}", e))?;
        if memory.view(&store).size().0 < 1 {
            memory.grow(&mut store, 1).map_err(|e| format!("Failed to grow memory: {}", e))?;
        }

        println!("Querying metadata for plugin: {}", path.display());
        let metadata_ptr = query_metadata.call(&mut store).map_err(|e| format!("Failed to call 'CrustPlugin_QueryMetadata': {}", e))?;
        if metadata_ptr.is_null() {
            return Err("Plugin rejected metadata query".into());
        }

        let mem_view = memory.view(&store);
        let start = metadata_ptr.offset() as u64;
        let metadata_bytes = mem_view.copy_range_to_vec(start..start + std::mem::size_of::<PluginMetadata>() as u64)
            .map_err(|e| format!("Failed to copy metadata bytes: {}", e))?;
        let metadata = unsafe { &*(metadata_bytes.as_ptr() as *const PluginMetadata) };
        let sdk_version = metadata.sdk_version;
        if sdk_version < MIN_SUPPORTED_SDK_VERSION {
            return Err(format!("Failed to load plugin '{}': SDK version {} is not supported, minimum supported version is {}", path.display(), sdk_version, MIN_SUPPORTED_SDK_VERSION).into());
        }
        let start = metadata.manifest.offset() as u64;
        let len = metadata.manifest_len.offset() as u64;
        let manifest = mem_view.copy_range_to_vec(start..start + len)
            .map_err(|e| format!("Failed to copy manifest bytes: {}", e))?;
        let manifest = std::str::from_utf8(&manifest).map_err(|e| format!("Failed to parse manifest UTF-8 bytes: {}", e))?;
        let manifest = serde_json::from_str::<PluginInfo>(manifest)
            .map_err(|e| format!("Failed to parse manifest JSON: {}", e))?;

        // if !entry_point(&API) {
        //     return Err(format!("Failed to initialize plugin '{}'", name).into());
        // }

        Ok(Plugin {
            info: manifest,
        })
    }
}

pub struct Plugin {
    info: PluginInfo,
}
