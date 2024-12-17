use crate::chat::{ClickEvent, Style, Text, TextBuilder, TextColor};

use super::{packet_handler, ProxyServer, SlotId};


pub trait Command {
    fn name() -> String;
    async fn execute(player_id: SlotId, args: Vec<&str>);
}

pub struct CommandServer {}

impl Command for CommandServer {
    fn name() -> String {
        "server".to_owned()
    }

    async fn execute(player_id: SlotId, args: Vec<&str>) {
        if args.is_empty() {
            let servers = ProxyServer::instance().servers.read().await;
            let mut first = true;
            let mut builder = TextBuilder::new("Available servers: ").style(Style::default().with_color(TextColor::from_rgb(182, 255, 156)));
            for (_, info) in servers.all_servers() {
                if first {
                    first = false;
                } else {
                    builder.add_extra(", ");
                }
                let mut text = Text::new(info.label.as_str());
                text.click_event = Some(ClickEvent {
                    action: crate::chat::ClickAction::RunCommand,
                    value: format!("/server {}", info.label),
                });
                text.hover_event = Some(crate::chat::HoverEvent::ShowText(Box::new(Text::new("click to connect"))));
                builder.add_extra(text);
            }
            drop(servers);
            if let Some(player) = ProxyServer::instance().players().read().await.get(player_id) {
                player.send_message(builder.build()).await.ok();
            }
        } else {
            let server_name = args.first().unwrap().to_ascii_lowercase();
            let servers = ProxyServer::instance().servers.read().await;
            let server = servers.get_server_id_by_name(&server_name);
            if let Some(server_id) = server {
                drop(servers);
                packet_handler::switch_server_helper(player_id, server_id).await;
            } else {
                drop(servers);
                if let Some(player) = ProxyServer::instance().players().read().await.get(player_id) {
                    player.send_message(TextBuilder::new(format!("The server {} does not exist", server_name)).style(Style::default().with_color(TextColor::Red)).build()).await.ok();
                }
            }
        }
    }
}