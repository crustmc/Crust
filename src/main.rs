extern crate core;

use crate::plugin::api::API;
use crate::server::command::CommandSender;
use crate::server::ProxyServer;
use core::str;
use env_logger::{Builder, Target, WriteStyle};
use log::error;
use reedline::{
    ExternalPrinter, Prompt, PromptEditMode, PromptHistorySearch, PromptHistorySearchStatus,
    Reedline, Signal,
};
use std::borrow::Cow;
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

/********** pipe all the writes ***********/
struct SharedWriter {
    printer: Box<ExternalPrinter<String>>,
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

/**********    custom prompt    ***********/
struct CrustPrompt;

impl Prompt for CrustPrompt {
    fn render_prompt_left(&self) -> Cow<str> {
        Cow::Borrowed("")
    }

    fn render_prompt_right(&self) -> Cow<str> {
        Cow::Borrowed("")
    }

    fn render_prompt_indicator(&self, _: PromptEditMode) -> Cow<str> {
        "> ".into()
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<str> {
        Cow::Borrowed(":::")
    }

    fn render_prompt_history_search_indicator(
        &self,
        history_search: PromptHistorySearch,
    ) -> Cow<str> {
        let prefix = match history_search.status {
            PromptHistorySearchStatus::Passing => "",
            PromptHistorySearchStatus::Failing => "failing ",
        };
        Cow::Owned(format!(
            "({}reverse-search: {}) ",
            prefix, history_search.term
        ))
    }
}
fn main() {
    if std::env::var("RUST_LOG").is_err() {
        #[cfg(debug_assertions)]
        std::env::set_var("RUST_LOG", "debug");
        #[cfg(not(debug_assertions))]
        std::env::set_var("RUST_LOG", "info");
    }
    let printer = ExternalPrinter::default();

    Builder::from_default_env()
        .write_style(WriteStyle::Always)
        .target(Target::Pipe(Box::new(SharedWriter {
            printer: Box::new(printer.clone()),
        })))
        .try_init()
        .unwrap();

    server::run_server();
    let mut line_editor = Reedline::create().with_external_printer(printer);

    loop {
        if let Ok(sig) = line_editor.read_line(&CrustPrompt) {
            match sig {
                Signal::Success(input) => {
                    let executed = ProxyServer::instance()
                        .command_registry()
                        .execute(&CommandSender::Console, &input);
                    if !executed {
                        error!("Unknown command.");
                    }
                }
                Signal::CtrlD | Signal::CtrlC => {
                    API.shutdown_proxy(None);
                }
            }
            continue;
        }
        break;
    }
}
