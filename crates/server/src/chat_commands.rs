//! Chat commands: chat messages starting with `/` are commands for the
//! server rather than things to say. They're parsed here (by the gate) and
//! carried out by [`crate::gameplay`].

/// A command a player can type in chat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatCommand {
    /// `/town`: go back to the starting town's square.
    Town,
    /// `/help`: list the commands.
    Help,
}

impl ChatCommand {
    /// Every command, with a line of help for each.
    const ALL: [(ChatCommand, &'static str, &'static str); 2] =
        [(ChatCommand::Town, "town", "go back to the starting town"), (ChatCommand::Help, "help", "list the commands")];

    /// Reads a chat message as a command, if it starts with `/`. Returns
    /// `None` for ordinary chat, and an error message for unknown commands.
    pub fn parse(text: &str) -> Option<Result<ChatCommand, String>> {
        let name = text.strip_prefix('/')?.split_whitespace().next().unwrap_or_default().to_lowercase();
        let found = Self::ALL.iter().find(|(_, command_name, _)| *command_name == name).map(|(command, ..)| *command);
        Some(found.ok_or_else(|| format!("There's no /{name} command. Type /help to see the commands.")))
    }

    /// The help text listing every command.
    pub fn help() -> String {
        let lines: Vec<String> = Self::ALL.iter().map(|(_, name, help)| format!("/{name}: {help}")).collect();
        format!("Commands: {}", lines.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_recognised() {
        assert_eq!(ChatCommand::parse("/town"), Some(Ok(ChatCommand::Town)));
        assert_eq!(ChatCommand::parse("/TOWN please"), Some(Ok(ChatCommand::Town)));
        assert_eq!(ChatCommand::parse("hello /town"), None);
        assert!(matches!(ChatCommand::parse("/fly"), Some(Err(_))));
    }
}
