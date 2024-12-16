
pub mod auth;
pub mod chat;
pub mod server;
pub mod util;
pub mod version;

fn main() {
    if std::env::var("RUST_LOG").is_err() {
        #[cfg(debug_assertions)]
        std::env::set_var("RUST_LOG", "debug");
        #[cfg(not(debug_assertions))]
        std::env::set_var("RUST_LOG", "info");
    }
    env_logger::init();
    server::run_server();
}
