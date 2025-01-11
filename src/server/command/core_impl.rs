use std::collections::HashMap;
use std::ops::Deref;
use super::{CommandRegistryBuilder, CommandSender};
use crate::{
    chat::*,
    server::{
        brigadier::{Suggestion, Suggestions},
        ProxyServer,
    },
};

pub fn register_all(builder: CommandRegistryBuilder) -> CommandRegistryBuilder {
    builder
        .core_command(
            ["server"],
            Default::default(),
            server_command,
            Some(server_command_completer),
            "crust.command.server",
            "Switches the player to another server",
        )
        .core_command(
            ["gkick"],
            Default::default(),
            gkick_command,
            Some(gkick_command_completer),
            "crust.command.gkick",
            "Kick a player from the proxy",
        )
        .core_command(
            ["end"],
            crate::server::command::CommandArgType::Args0ContainsEverything,
            end_command,
            None,
            "crust.command.end",
            "Shutdown the proxy",
        )
        .core_command(
            ["glist"],
            Default::default(),
            glist_command,
            None,
            "crust.command.glist",
            "List all players on the proxy",
        )
}

fn end_command(_sender: &CommandSender, _name: &str, args: Vec<&str>) {
    if args.get(1).unwrap().is_empty() {
        ProxyServer::instance().shutdown(None);
    } else {
        ProxyServer::instance().shutdown(Some(&*args.get(1).unwrap().replace("&", "§")));
    }
}

fn gkick_command(sender: &CommandSender, _name: &str, mut args: Vec<&str>) {
    if args.is_empty() {
        sender.send_message(
            TextBuilder::new("Usage: /gkick <player> [reason]")
                .style(Style::empty().with_color(TextColor::Red)),
        );
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
                    ProxyServer::instance().block_on(async move {
                        player.kick(Text::new(str)).await.ok();
                    });
                } else {
                    ProxyServer::instance().block_on(async move {
                        player
                            .kick(
                                TextBuilder::new("You have been kicked off the proxy")
                                    .style(Style::empty().with_color(TextColor::Red))
                                    .build(),
                            )
                            .await
                            .ok();
                    });
                }
                return;
            }
        }
    }
    sender.send_message(TextBuilder::new(format!(
        "Player {} not found",
        args.first().unwrap()
    )))
}

fn gkick_command_completer(
    _sender: &CommandSender,
    _name: &str,
    args: Vec<&str>,
    suggestions: &mut Suggestions,
) {
    if args.len() != 1 {
        return;
    }
    let filter = args.first().unwrap();
    let players = ProxyServer::instance().players().blocking_read();
    for (_, player) in players.iter() {
        if !player.name.starts_with(filter) {
            continue;
        }
        suggestions.matches.push(Suggestion {
            text: player.name.clone(),
            tooltip: None,
        });
    }
}

fn glist_command(sender: &CommandSender, _name: &str, args: Vec<&str>) {
    let mut amt = 0usize;
    let mut map = HashMap::new();
    let players = ProxyServer::instance().players.blocking_read();
    let servers = ProxyServer::instance().servers.blocking_read();
    for (_, player) in players.iter() {
        amt += 1;
        if let Some(server_id) = player.current_server {
            if let Some(server) = servers.servers.get(server_id) {
                if !map.contains_key(&server.label) {
                    let vec = vec!(player.name.clone());
                    map.insert(server.label.clone(), vec);
                } else {
                    let mut values = map.get_mut(&server.label).unwrap();
                    values.push(player.name.clone());
                }
            }
        }
    }
    drop(players);
    drop(servers);
    
    let style = Style::default().with_color(TextColor::from_rgb(182, 255, 156));
    sender.send_message(TextBuilder::new(format!("There are currently {} players on the proxy", amt)).style(style.clone()));
    for (name, player_names) in map {
        sender.send_message(TextBuilder::new(format!("{} ({}): {}", name, player_names.len(), player_names.join(", "))).style(style.clone()));
    }
}

fn server_command(sender: &CommandSender, _name: &str, args: Vec<&str>) {
    if !sender.is_player() {
        sender.send_message(
            TextBuilder::new("This command can only be executed by a player")
                .style(Style::empty().with_color(TextColor::Red)),
        );
        return;
    }
    let style = Style::default().with_color(TextColor::from_rgb(182, 255, 156));
    let player = sender.as_player().unwrap();
    if args.is_empty() {
        let servers = ProxyServer::instance().servers.blocking_read();
        let mut first = true;
        if let Some(player) = player.upgrade() {
            let current = player.current_server;
            if let Some(server) = current {
                if let Some(server) = servers.servers.get(server) {
                    sender.send_message(TextBuilder::new(format!("You are currenrly connected to {}", server.label )).style(style.clone()).build());
                }
            }
        }
        
        let mut builder = TextBuilder::new("Available servers: ").style(style);
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
            text.hover_event = Some(HoverEvent::ShowText(Box::new(Text::new(
                "click to connect",
            ))));
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
            ProxyServer::instance().block_on(crate::server::packet_handler::switch_server_helper(
                player, server_id,
            ));
        } else {
            drop(servers);
            sender.send_message(
                TextBuilder::new(format!("The server {} does not exist", server_name))
                    .style(Style::default().with_color(TextColor::Red)),
            );
        }
    }
}

fn server_command_completer(
    _sender: &CommandSender,
    _name: &str,
    args: Vec<&str>,
    suggestions: &mut Suggestions,
) {
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
