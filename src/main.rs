use rustyline::{DefaultEditor, ExternalPrinter};
use std::io;
use std::io::Write;
use env_logger::{Builder, Target, WriteStyle};
use log::{error, info};
use crate::server::command::CommandSender;
use crate::server::ProxyServer;

pub mod auth;
pub mod chat;
pub mod server;
pub mod util;
pub mod version;

struct SharedWriter {
    printer: Box<dyn ExternalPrinter + Send>,
}

impl Write for SharedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.printer
            .print(String::from_utf8_lossy(buf).to_string())
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn main() {
    if std::env::var("RUST_LOG").is_err() {
        #[cfg(debug_assertions)]
        std::env::set_var("RUST_LOG", "debug");
        #[cfg(not(debug_assertions))]
        std::env::set_var("RUST_LOG", "info");
    }


    let mut editor = DefaultEditor::new().unwrap();
    let external_printer = editor.create_external_printer().unwrap();

    let shared_writer = SharedWriter {
        printer: Box::new(external_printer),
    };

    Builder::from_default_env()
        .target(Target::Pipe(Box::new(shared_writer)))
        .write_style(WriteStyle::Always)
        .filter_module("rustyline", log::LevelFilter::Off)
        .try_init()
        .unwrap();

    server::run_server();

    loop {
        match editor.readline("> ") {
            Ok(input) => {
                let executed = ProxyServer::instance().command_registry().execute(&CommandSender::Console, &input);
                if !executed {
                    error!("Unknown command: {}", input);
                }
            }
            Err(err) => {
                println!("Error reading console line: {:?}", err);
                break
            }
        }
    }
}
