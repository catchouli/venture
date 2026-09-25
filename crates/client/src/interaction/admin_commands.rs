//! Admin commands typed in chat: `/setlevel 10`, `/reset`, `/setclass mage`,
//! `/setgender feminine`.
//!
//! They become [`AdminCommand`]s for the server, which carries them out only
//! if the player really is an admin. `/help` lists them for admins, marked
//! as admin commands.

use venture_protocol::{AdminCommand, BodyType};

/// Every admin command: its name, what follows it, and what it does.
pub const ADMIN_COMMANDS: [(&str, &str, &str); 4] = [
    ("setlevel", "LEVEL", "set your level"),
    ("reset", "", "start your character over at level 1"),
    ("setclass", "CLASS", "change class, getting your points back"),
    ("setgender", "BODY", "masculine, feminine or androgynous"),
];

/// Reads an admin command from its name and the words after it. Returns
/// `None` if there's no admin command with that name, and an explanation if
/// the words after it don't fit.
pub fn parse(name: &str, arguments: &[&str]) -> Option<Result<AdminCommand, String>> {
    let (name, usage, _) = ADMIN_COMMANDS.iter().find(|(command, ..)| command.eq_ignore_ascii_case(name))?;
    let usage = || format!("Write it as /{name} {usage}").trim_end().to_string() + ".";
    Some(match (*name, arguments) {
        ("setlevel", [level]) => level.parse().map(|level| AdminCommand::SetLevel { level }).map_err(|_| usage()),
        ("reset", []) => Ok(AdminCommand::Reset),
        ("setclass", [class]) => Ok(AdminCommand::SetClass { class: class.to_string() }),
        ("setgender", [body]) => BodyType::ALL
            .into_iter()
            .find(|option| option.name().eq_ignore_ascii_case(body))
            .map(|body| AdminCommand::SetBody { body })
            .ok_or_else(|| "Write it as /setgender masculine, feminine or androgynous.".to_string()),
        _ => Err(usage()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_commands_are_read() {
        assert_eq!(parse("setlevel", &["10"]), Some(Ok(AdminCommand::SetLevel { level: 10 })));
        assert_eq!(parse("RESET", &[]), Some(Ok(AdminCommand::Reset)));
        assert_eq!(parse("setclass", &["Mage"]), Some(Ok(AdminCommand::SetClass { class: "Mage".to_string() })));
        assert_eq!(parse("setlevel", &["lots"]), Some(Err("Write it as /setlevel LEVEL.".to_string())));
        assert_eq!(parse("reset", &["now"]), Some(Err("Write it as /reset.".to_string())));
        assert_eq!(parse("setgender", &["Feminine"]), Some(Ok(AdminCommand::SetBody { body: BodyType::Feminine })));
        assert!(parse("setgender", &["dragon"]).unwrap().is_err());
        assert_eq!(parse("fly", &[]), None);
    }
}
