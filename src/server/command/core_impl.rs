use crate::{chat::*, server::ProxyServer};

use super::{CommandRegistryBuilder, CommandSender};

pub fn register_all(builder: CommandRegistryBuilder) -> CommandRegistryBuilder {
    builder
        .core_command(["server"], Default::default(), server_command, None, "crust.command.server", "Switches the player to another server")
}

fn server_command(sender: &CommandSender, _name: &str, args: Vec<&str>) {
    if !sender.is_player() {
        sender.send_message(TextBuilder::new("This command can only be executed by a player").style(Style::empty().with_color(TextColor::Red)));
        return;
    }
    let player_id = sender.as_player().unwrap();
    if args.is_empty() {
        let servers = ProxyServer::instance().servers.blocking_read();
        let mut first = true;
        let mut builder = TextBuilder::new("Available servers: ")
            .style(Style::default().with_color(TextColor::from_rgb(182, 255, 156)));
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
        sender.send_message(builder);
    } else {
        let server_name = args.first().unwrap().to_ascii_lowercase();
        let servers = ProxyServer::instance().servers.blocking_read();
        let server = servers.get_server_id_by_name(&server_name);
        if let Some(server_id) = server {
            drop(servers);
            ProxyServer::instance().block_on(crate::server::packet_handler::switch_server_helper(player_id, server_id));
        } else {
            drop(servers);
            sender.send_message(TextBuilder::new(format!("The server {} does not exist", server_name))
                .style(Style::default().with_color(TextColor::Red)));
        }
    }
}
