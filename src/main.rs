extern crate core;

use crate::server::command::CommandSender;
use crate::server::ProxyServer;
use env_logger::{Builder, Target, WriteStyle};
use log::LevelFilter;
use rustyline::{DefaultEditor, ExternalPrinter};
use std::fmt::Arguments;
use std::io;
use std::io::Write;

pub mod auth;
pub mod chat;
pub mod haproxy;
pub mod plugin;
pub mod server;
pub mod util;
pub mod version;


#[cfg(windows)]
pub fn enable_virtual_terminal_processing() {
    use winapi_util::console::Console;

    if let Ok(mut term) = Console::stdout() {
        term.set_virtual_terminal_processing(true).ok();
    }
    if let Ok(mut term) = Console::stderr() {
        term.set_virtual_terminal_processing(true).ok();
    }
}

/********** pipe all the writes ***********/
struct SharedWriter {
    printer: Box<dyn ExternalPrinter + Send>,
}
impl Write for SharedWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        unreachable!("write");
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        let str = String::from_utf8_lossy(buf);
        self.printer
            .print(str.to_string())
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(())
    }

    fn write_fmt(&mut self, _: Arguments<'_>) -> io::Result<()> {
        unreachable!("write_fmt");
    }
}

fn main() {
    #[cfg(windows)]
    enable_virtual_terminal_processing();

    if std::env::var("RUST_LOG").is_err() {
        #[cfg(debug_assertions)]
        std::env::set_var("RUST_LOG", "debug");
        #[cfg(not(debug_assertions))]
        std::env::set_var("RUST_LOG", "info");
    }
    let mut rl = DefaultEditor::new().unwrap();
    let printer = rl.create_external_printer().unwrap();
    let target = Target::Pipe(Box::new(SharedWriter { printer: Box::new(printer) }));

    Builder::from_default_env()
        .write_style(WriteStyle::Always)
        .filter_module("rustyline", LevelFilter::Off)
        .filter_module("cranelift_codegen", LevelFilter::Off)
        .target(target)
        .try_init()
        .unwrap();

    server::run_server();

    while let Ok(line) = rl.readline("> ") {
        ProxyServer::instance().command_registry().execute(&CommandSender::Console, line.as_str());
    }
    //API.shutdown_proxy(None);
}
