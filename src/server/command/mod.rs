use std::collections::HashMap;

use crate::{chat::Text, util::WeakHandle};

use super::{brigadier::Suggestions, ProxiedPlayer, ProxyServer};

pub(crate) mod core_impl;

pub type CommandExecutor = fn(sender: &CommandSender, name: &str, args: Vec<&str>);
pub type CommandTabCompleter = fn(sender: &CommandSender, name: &str, args: Vec<&str>, &mut Suggestions);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CommandArgType {
    /// Splits the command by space and passes the arguments as a vector
    #[default]
    TextSplitBySpace,
    /// Passes the full command string in ``args[0]`` and the full args string without the command name in ``args[1]``
    /// And you also have to calculate the start and length in the ``Suggestions`` struct when tab completing
    Args0ContainsEverything,
}

pub struct CommandInfo {
    pub names: Vec<String>,
    pub arg_type: CommandArgType,
    pub executor: CommandExecutor,
    pub tab_completer: Option<CommandTabCompleter>,
    pub permission: String,
    pub description: String,
}

impl CommandInfo {

    pub fn name(&self) -> &str {
        &self.names[0]
    }
}

pub trait CommandNames {

    fn names(self) -> impl Iterator<Item = String>;
}

impl<S: Into<String>, I: IntoIterator<Item = S>> CommandNames for I {

    fn names(self) -> impl Iterator<Item = String> {
        self.into_iter().map(|s| s.into())
    }
}

pub struct CommandRegistryBuilder {
    commands: Vec<CommandInfo>,
}

impl CommandRegistryBuilder {

    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    fn is_name_valid(name: &str) -> bool {
        name.chars().all(|c| c.is_alphanumeric() || c == '_')
    }

    pub fn add_core_command<N: CommandNames, P: Into<String>, D: Into<String>>(&mut self, names: N, arg_type: CommandArgType,
        executor: CommandExecutor, tab_completer: Option<CommandTabCompleter>, permission: P, description: D) {
        let mut names = names.names().collect::<Vec<String>>();
        for i in 0..names.len() {
            if !Self::is_name_valid(&names[i]) {
                panic!("Failed to register core command! {}th name contains invalid characters: {}", i, names[i]);
            }
            names.push(format!("crust:{}", names[i]));
        }
        self.commands.push(CommandInfo {
            names,
            arg_type,
            executor,
            tab_completer,
            permission: permission.into(),
            description: description.into(),
        });
    }

    pub fn core_command<N: CommandNames, P: Into<String>, D: Into<String>>(mut self, names: N, arg_type: CommandArgType,
        executor: CommandExecutor, tab_completer: Option<CommandTabCompleter>, permission: P, description: D) -> Self {
        self.add_core_command(names, arg_type, executor, tab_completer, permission, description);
        self
    }

    pub fn build(self) -> CommandRegistry {
        CommandRegistry::new(self.commands)
    } 
}

pub struct CommandRegistry {
    commands: Vec<CommandInfo>,
    commands_by_name: HashMap<String, usize>,
}

impl CommandRegistry {

    fn new(commands: Vec<CommandInfo>) -> Self {
        let mut commands_by_name = HashMap::new();
        for (index, command) in commands.iter().enumerate() {
            for name in command.names.iter() {
                commands_by_name.insert(name.clone(), index);
            }
        }
        Self {
            commands,
            commands_by_name,
        }
    }

    pub fn all_commands(&self) -> &[CommandInfo] {
        &self.commands
    }

    pub fn get_command_by_name(&self, name: &str) -> Option<&CommandInfo> {
        self.commands_by_name.get(name).map(|index| &self.commands[*index])
    }

    pub fn execute(&self, sender: &CommandSender, command: &str) -> bool {
        if command.is_empty() {
            return false;
        }
        let mut parts = command.splitn(2, ' ');
        let name = parts.next().unwrap();
        self.get_command_by_name(name).map_or(false, |info| {
            let args = parts.next().unwrap_or("");
            let args = match info.arg_type {
                CommandArgType::TextSplitBySpace => if args.is_empty() { vec![] } else { args.split_ascii_whitespace().collect::<Vec<&str>>() },
                CommandArgType::Args0ContainsEverything => vec![command, args],
            };
            (info.executor)(sender, name, args);
            true
        })
    }

    pub fn tab_complete(&self, sender: &CommandSender, command: &str) -> Option<Option<Suggestions>> {
        if command.is_empty() {
            return None;
        }
        let mut parts = command.splitn(2, ' ');
        let name = parts.next().unwrap();
        self.get_command_by_name(name).map_or(None, |info| {
            let mut suggestions = Suggestions { start: 0, length: 0, matches: Vec::new() };
            let args = parts.next().unwrap_or("");
            let args = match info.arg_type {
                CommandArgType::TextSplitBySpace => {
                    let mut splitted = if args.is_empty() { vec![] } else { args.split_ascii_whitespace().collect::<Vec<&str>>() };
                    if command.ends_with(" ") {
                        suggestions.start = command.len() as i32 + 1;
                        suggestions.length = 0;
                        splitted.push("");
                    } else {
                        let mut index = command.len();
                        let mut len = 0;
                        loop {
                            index -= 1;
                            len += 1;
                            if index == 0 || command.as_bytes()[index] == b' ' {
                                break;
                            }
                        }
                        suggestions.start = index as i32 + 2;
                        suggestions.length = len as i32;
                    }
                    splitted
                },
                CommandArgType::Args0ContainsEverything => vec![command, args],
            };
            if let Some(ref completer) = info.tab_completer {
                completer(sender, name, args, &mut suggestions);
                Some(Some(suggestions))
            } else {
                Some(None)
            }
        })
    }
}

pub enum CommandSender {
    Console,
    Player(WeakHandle<ProxiedPlayer>),
}

impl CommandSender {

    pub fn is_console(&self) -> bool {
        match self {
            CommandSender::Console => true,
            _ => false,
        }
    }

    pub fn is_player(&self) -> bool {
        match self {
            CommandSender::Player(_) => true,
            _ => false,
        }
    }

    pub fn as_player(&self) -> Option<WeakHandle<ProxiedPlayer>> {
        match self {
            CommandSender::Player(p) => Some(p.clone()),
            _ => None,
        }
    }

    pub fn send_message<T: Into<Text>>(&self, message: T) {
        ProxyServer::instance().block_on(self.send_message_async(message.into()));
    }

    pub async fn send_message_async(&self, message: Text) {
        match self {
            CommandSender::Console => log::info!("{}", message),
            CommandSender::Player(player) => {
                if let Some(player) = player.upgrade() {
                    let _ = player.send_message(message).await;
                }
            },
        }
    }

    pub fn has_permission(&self, _permission: &str) -> bool {
        match self {
            CommandSender::Console => true,
            CommandSender::Player(_) => true, // TODO: Implement permissions
        }
    }
}
