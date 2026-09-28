//! Admin commands typed in chat: `/setlevel 10`, `/reset`, `/setclass mage`,
//! `/setgender feminine`, `/time 19:30`, `/timescale 10`, `/give worm 20`,
//! `/setskill fishing 20`, `/tp Wynra`, `/tp 2340 2176`, `/setspeed 5`.
//!
//! They become [`AdminCommand`]s for the server, which carries them out only
//! if the player really is an admin. `/help` lists them for admins, marked
//! as admin commands.

use venture_protocol::{AdminCommand, BodyType, TeleportTo};

/// Every admin command: its name, what follows it, and what it does.
pub const ADMIN_COMMANDS: [(&str, &str, &str); 10] = [
    ("setlevel", "LEVEL", "set your level"),
    ("reset", "", "start your character over at level 1"),
    ("setclass", "CLASS", "change class, getting your points back"),
    ("setgender", "BODY", "masculine, feminine or androgynous"),
    ("time", "HOUR", "set the time of day for everyone (19.5, or 19:30)"),
    ("timescale", "SPEED", "how fast the days go for everyone (1 usual, 10 ten times, 0 stopped)"),
    ("give", "ITEM [COUNT]", "put items in your bag (an item's id or name)"),
    ("setskill", "SKILL LEVEL", "set your level in a skill"),
    ("tp", "[NAME] PLAYER, or [NAME] X Y", "teleport (you, or NAME) to a player or a place (the map shows X Y)"),
    ("setspeed", "SPEED", "run faster or slower (1 usual, 5 five times)"),
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
        ("time", [time]) => parse_hour(time).map(|hour| AdminCommand::SetTime { hour }).ok_or_else(usage),
        ("timescale", [scale]) => scale
            .parse::<f32>()
            .ok()
            .filter(|scale| scale.is_finite() && *scale >= 0.0)
            .map(|scale| AdminCommand::SetTimescale { scale })
            .ok_or_else(usage),
        ("give", [item]) => Ok(AdminCommand::Give { item: item.to_string(), count: 1 }),
        ("give", [item, count]) => {
            count.parse().map(|count| AdminCommand::Give { item: item.to_string(), count }).map_err(|_| usage())
        }
        ("setskill", [skill, level]) => {
            level.parse().map(|level| AdminCommand::SetSkill { skill: skill.to_string(), level }).map_err(|_| usage())
        }
        ("tp", arguments) => teleport(arguments).ok_or_else(usage),
        ("setspeed", [scale]) => scale
            .parse::<f32>()
            .ok()
            .filter(|scale| scale.is_finite() && *scale > 0.0)
            .map(|scale| AdminCommand::SetSpeed { scale })
            .ok_or_else(usage),
        _ => Err(usage()),
    })
}

/// `/tp`'s words: a player, two numbers (a place), or either after the name
/// of who to move.
fn teleport(arguments: &[&str]) -> Option<AdminCommand> {
    // (Copied from the map, a place may have a comma: `2340, 2176`.)
    let number = |word: &str| word.trim_end_matches(',').parse::<f32>().ok().filter(|n| n.is_finite());
    let place = |x: &str, y: &str| Some(TeleportTo::Place { x: number(x)?, y: number(y)? });
    let (who, to) = match arguments {
        [player] => (None, TeleportTo::Player(player.to_string())),
        [x, y] if place(x, y).is_some() => (None, place(x, y)?),
        [who, player] => (Some(who), TeleportTo::Player(player.to_string())),
        [who, x, y] => (Some(who), place(x, y)?),
        _ => return None,
    };
    Some(AdminCommand::Teleport { who: who.map(|who| who.to_string()), to })
}

/// A time of day, as hours (`19.5`) or hours and minutes (`19:30`).
fn parse_hour(time: &str) -> Option<f32> {
    let hour = match time.split_once(':') {
        Some((hours, minutes)) => hours.parse::<f32>().ok()? + minutes.parse::<f32>().ok()? / 60.0,
        None => time.parse().ok()?,
    };
    (0.0..24.0).contains(&hour).then_some(hour)
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
        assert_eq!(parse("time", &["19:30"]), Some(Ok(AdminCommand::SetTime { hour: 19.5 })));
        assert_eq!(parse("time", &["6"]), Some(Ok(AdminCommand::SetTime { hour: 6.0 })));
        assert!(parse("time", &["25"]).unwrap().is_err());
        assert_eq!(parse("timescale", &["1.5"]), Some(Ok(AdminCommand::SetTimescale { scale: 1.5 })));
        assert_eq!(parse("timescale", &["0"]), Some(Ok(AdminCommand::SetTimescale { scale: 0.0 })));
        assert!(parse("timescale", &["-2"]).unwrap().is_err());
        assert_eq!(
            parse("give", &["worm", "20"]),
            Some(Ok(AdminCommand::Give { item: "worm".to_string(), count: 20 }))
        );
        assert_eq!(parse("give", &["worm"]), Some(Ok(AdminCommand::Give { item: "worm".to_string(), count: 1 })));
        assert_eq!(
            parse("setskill", &["fishing", "20"]),
            Some(Ok(AdminCommand::SetSkill { skill: "fishing".to_string(), level: 20 }))
        );
        let to_player = |name: &str| TeleportTo::Player(name.to_string());
        assert_eq!(parse("tp", &["Wynra"]), Some(Ok(AdminCommand::Teleport { who: None, to: to_player("Wynra") })));
        assert_eq!(
            parse("tp", &["Wynra", "Tam"]),
            Some(Ok(AdminCommand::Teleport { who: Some("Wynra".to_string()), to: to_player("Tam") }))
        );
        let place = TeleportTo::Place { x: 2340.0, y: 2176.5 };
        assert_eq!(
            parse("tp", &["2340,", "2176.5"]),
            Some(Ok(AdminCommand::Teleport { who: None, to: place.clone() }))
        );
        assert_eq!(
            parse("tp", &["Tam", "2340", "2176.5"]),
            Some(Ok(AdminCommand::Teleport { who: Some("Tam".to_string()), to: place }))
        );
        assert!(parse("tp", &[]).unwrap().is_err());
        assert!(parse("tp", &["Tam", "north", "2"]).unwrap().is_err());
        assert_eq!(parse("setspeed", &["5"]), Some(Ok(AdminCommand::SetSpeed { scale: 5.0 })));
        assert!(parse("setspeed", &["0"]).unwrap().is_err());
        assert_eq!(parse("fly", &[]), None);
    }
}
