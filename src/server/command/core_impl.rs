use crate::{chat::*, server::{brigadier::{Suggestion, Suggestions}, ProxyServer}};
use crate::server::ProxiedPlayer;
use crate::util::Handle;
use super::{CommandRegistryBuilder, CommandSender};

pub fn register_all(builder: CommandRegistryBuilder) -> CommandRegistryBuilder {
    builder
        .core_command(
            ["server"], Default::default(), server_command, Some(server_command_completer),
            "crust.command.server", "Switches the player to another server"
        )
        .core_command(
            ["gkick"], Default::default(), gkick_command, Some(gkick_command_completer),
            "crust.command.gkick", "Kick a player from the proxy"
        )
}

fn gkick_command(sender: &CommandSender, _name: &str, mut args: Vec<&str>) {
    if args.is_empty() {
        sender.send_message(TextBuilder::new("Usage: /gkick <player> [reason]").style(Style::empty().with_color(TextColor::Red)));
        return;
    }
    let player = ProxyServer::instance().get_player_by_name_blocking(args.first().unwrap());
    if let Some(player) = player {
        match player.upgrade() {
            None => {}
            Some(player) => {
                if args.len() > 1 {
                    args.remove(0);
                    let str = args.join(" ");
                    ProxyServer::instance().block_on( async move {
                        player.kick(Text::new(str)).await.ok();
                    });
                } else {
                    ProxyServer::instance().block_on(async move {
                        player.kick(TextBuilder::new("You have been kicked off the proxy").style(Style::empty().with_color(TextColor::Red)).build()).await.ok();
                    });
                }
                return;
            }
        }
    }
    sender.send_message(TextBuilder::new(format!("Player {} not found", args.first().unwrap())))
}


fn gkick_command_completer(_sender: &CommandSender, _name: &str, args: Vec<&str>, suggestions: &mut Suggestions) {
    if args.len() != 1 {
        return;
    }
    let filter = args.first().unwrap();
    let players = ProxyServer::instance().players().blocking_read();
    for (_, player) in players.iter() {
        if !player.profile.name.starts_with(filter) {
            continue;
        }
        suggestions.matches.push(Suggestion {
            text: player.profile.name.clone(),
            tooltip: None,
        });
    }
   
}


fn server_command(sender: &CommandSender, _name: &str, args: Vec<&str>) {
    if !sender.is_player() {
        sender.send_message(TextBuilder::new("This command can only be executed by a player").style(Style::empty().with_color(TextColor::Red)));
        return;
    }
    let player = sender.as_player().unwrap();
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
                action: ClickAction::RunCommand,
                value: format!("/server {}", info.label),
            });
            text.hover_event = Some(HoverEvent::ShowText(Box::new(Text::new("click to connect"))));
            builder.add_extra(text);
        }
        drop(servers);
        sender.send_message(builder);
    } else {
        let server_name = args.first().unwrap();
        let servers = ProxyServer::instance().servers.blocking_read();
        let server = servers.get_server_id_by_name(&server_name);
        if let Some(server_id) = server {
            drop(servers);
            ProxyServer::instance().block_on(crate::server::packet_handler::switch_server_helper(player, server_id));
        } else {
            drop(servers);
            sender.send_message(TextBuilder::new(format!("The server {} does not exist", server_name))
                .style(Style::default().with_color(TextColor::Red)));
        }
    }
}

fn server_command_completer(_sender: &CommandSender, _name: &str, args: Vec<&str>, suggestions: &mut Suggestions) {
    if args.len() != 1 {
        return;
    }
    let filter = args.first().unwrap();
    let servers = ProxyServer::instance().servers().blocking_read();
    for (_, info) in servers.all_servers() {
        if !info.label.starts_with(filter) {
            continue;
        }
        suggestions.matches.push(Suggestion {
            text: info.label.clone(),
            tooltip: None,
        });
    }
}
