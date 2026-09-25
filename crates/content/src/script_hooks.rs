//! The functions ("hooks") a script can define, which the game calls when
//! things happen, and checking scripts before they're used.

/// Called when an NPC appears in the world (at startup, or when content reloads).
pub const ON_SPAWN: &str = "on_spawn";
/// Called when a player talks to an NPC: `on_interact(npc, player)`.
pub const ON_INTERACT: &str = "on_interact";
/// Called every few seconds while an NPC is standing still: `on_idle(npc)`.
pub const ON_IDLE: &str = "on_idle";

/// Every hook, with how many arguments it takes.
pub const HOOKS: [(&str, usize); 3] = [(ON_SPAWN, 1), (ON_INTERACT, 2), (ON_IDLE, 1)];

/// Checks a script compiles, and that every function whose name starts with
/// `on_` is a hook the game knows, with the right number of arguments (so a
/// misspelt hook is caught rather than silently never called).
pub fn check_script(source: &str) -> Result<(), String> {
    let engine = rhai::Engine::new_raw();
    let ast = engine.compile(source).map_err(|error| error.to_string())?;
    for function in ast.iter_functions() {
        if !function.name.starts_with("on_") {
            continue;
        }
        match HOOKS.iter().find(|(hook, _)| *hook == function.name) {
            None => {
                let known: Vec<&str> = HOOKS.iter().map(|(hook, _)| *hook).collect();
                return Err(format!("\"{}\" isn't a hook the game calls (hooks: {})", function.name, known.join(", ")));
            }
            Some((hook, arguments)) if function.params.len() != *arguments => {
                return Err(format!("{hook} takes {arguments} argument(s), not {}", function.params.len()));
            }
            Some(_) => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_scripts_pass() {
        assert!(check_script("fn on_interact(npc, player) { npc.say(\"Hello\"); } fn helper() {}").is_ok());
    }

    #[test]
    fn mistakes_are_caught() {
        assert!(check_script("fn on_interact(npc) {}").is_err(), "wrong argument count");
        assert!(check_script("fn on_interract(npc, player) {}").is_err(), "misspelt hook");
        assert!(check_script("fn on_idle(npc) { let x = ; }").is_err(), "syntax error");
    }
}
