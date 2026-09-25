//! Running content scripts (Rhai) safely.
//!
//! Scripts never touch the game directly. When a script calls, say,
//! `npc.say("Hello")`, that only queues a [`ScriptCommand`]. After the script
//! returns, [`crate::npcs`] checks each command and applies it. So scripts
//! can only do what the commands allow, and only in ways the engine accepts.
//!
//! The engine is locked down: no `eval`, no importing other files, and hard
//! limits on how much work a script may do per call and how big its strings,
//! arrays and maps may grow, so a buggy script can't stall the server.
//!
//! # The script API
//!
//! Hooks a script can define (see `venture_content::script_hooks`):
//! `on_spawn(npc)`, `on_interact(npc, player)`, `on_idle(npc)`.
//!
//! - `npc.name`, `npc.x`, `npc.y`, `npc.home_x`, `npc.home_y`: read-only.
//! - `npc.say(text)`: say something (shown above the NPC and in nearby players' chat).
//! - `npc.wander()`: walk to a random spot within the NPC's wander radius of home.
//! - `npc.move_to(x, y)`: walk to a point (kept within the wander radius).
//! - `player.name`, `player.x`, `player.y`: read-only.
//! - `random(n)`: a random whole number from 0 to n - 1.
//! - `chance(p)`: true with probability `p` (from 0.0 to 1.0).
//! - `print(text)`: write to the server log.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use rhai::{AST, CallFnOptions, Dynamic, Engine, FLOAT, INT, Scope};
use venture_content::ContentPack;

/// The most work (in Rhai operations) one hook call may do.
const MAX_OPERATIONS: u64 = 50_000;
/// The deepest function calls may nest.
const MAX_CALL_DEPTH: usize = 16;
/// The largest a string may grow, in bytes.
const MAX_STRING_SIZE: usize = 1_000;
/// The most items in an array or map.
const MAX_COLLECTION_SIZE: usize = 200;

/// Something a script asked for. Checked and applied by [`crate::npcs`].
#[derive(Clone, Debug, PartialEq)]
pub enum ScriptCommand {
    /// The NPC says something.
    Say {
        /// Which NPC.
        npc: Entity,
        /// What it says (not yet checked).
        text: String,
    },
    /// The NPC wanders somewhere near home.
    Wander {
        /// Which NPC.
        npc: Entity,
    },
    /// The NPC walks to a point.
    MoveTo {
        /// Which NPC.
        npc: Entity,
        /// Destination, in metres (not yet checked).
        x: f32,
        /// See `x`.
        y: f32,
    },
}

/// What a script sees of an NPC.
#[derive(Clone)]
pub struct NpcView {
    /// The NPC's entity (not visible to scripts; used to address commands).
    pub entity: Entity,
    /// Its name.
    pub name: String,
    /// Where it is.
    pub position: (f32, f32),
    /// Where it lives.
    pub home: (f32, f32),
}

/// What a script sees of a player.
#[derive(Clone)]
pub struct PlayerView {
    /// Their name.
    pub name: String,
    /// Where they are.
    pub position: (f32, f32),
}

/// The script engine and the compiled scripts.
#[derive(Resource)]
pub struct Scripts {
    engine: Engine,
    compiled: HashMap<String, AST>,
    queue: Arc<Mutex<Vec<ScriptCommand>>>,
}

impl Scripts {
    /// A locked-down engine with the script API registered.
    pub fn new() -> Self {
        let queue = Arc::new(Mutex::new(Vec::new()));
        let mut engine = Engine::new();
        engine
            .set_max_operations(MAX_OPERATIONS)
            .set_max_call_levels(MAX_CALL_DEPTH)
            .set_max_string_size(MAX_STRING_SIZE)
            .set_max_array_size(MAX_COLLECTION_SIZE)
            .set_max_map_size(MAX_COLLECTION_SIZE)
            .set_max_expr_depths(64, 32)
            .set_module_resolver(rhai::module_resolvers::DummyModuleResolver::new())
            .on_print(|text| info!("script: {text}"))
            .on_debug(|text, _, _| debug!("script: {text}"));
        engine.disable_symbol("eval");
        register_api(&mut engine, &queue);
        Self { engine, compiled: HashMap::new(), queue }
    }

    /// Compiles every script in a content pack, replacing the old ones.
    /// (The pack has already been checked, so this only fails on engine limits.)
    pub fn load(&mut self, pack: &ContentPack) -> Result<(), String> {
        let mut compiled = HashMap::new();
        for (name, source) in &pack.scripts {
            let ast = self.engine.compile(source).map_err(|error| format!("scripts/{name}: {error}"))?;
            compiled.insert(name.clone(), ast);
        }
        self.compiled = compiled;
        Ok(())
    }

    /// Calls `hook` in `script`, if the script defines it. Errors (including
    /// hitting the limits) are logged and otherwise ignored.
    pub fn call(&self, script: &str, hook: &str, args: impl rhai::FuncArgs) {
        let Some(ast) = self.compiled.get(script) else { return };
        let mut arguments = Vec::new();
        args.parse(&mut arguments);
        let defined =
            ast.iter_functions().any(|function| function.name == hook && function.params.len() == arguments.len());
        if !defined {
            return;
        }
        let options = CallFnOptions::new().eval_ast(false).rewind_scope(true);
        let result = self.engine.call_fn_with_options::<Dynamic>(options, &mut Scope::new(), ast, hook, arguments);
        if let Err(error) = result {
            warn!("Script {script} failed in {hook}: {error}");
        }
    }

    /// The commands scripts have queued since last time.
    pub fn take_commands(&self) -> Vec<ScriptCommand> {
        std::mem::take(&mut *self.queue.lock().expect("the script queue isn't poisoned"))
    }
}

impl Default for Scripts {
    fn default() -> Self {
        Self::new()
    }
}

/// Registers the types and functions scripts may use.
fn register_api(engine: &mut Engine, queue: &Arc<Mutex<Vec<ScriptCommand>>>) {
    engine
        .register_type_with_name::<NpcView>("Npc")
        .register_get("name", |npc: &mut NpcView| npc.name.clone())
        .register_get("x", |npc: &mut NpcView| npc.position.0 as FLOAT)
        .register_get("y", |npc: &mut NpcView| npc.position.1 as FLOAT)
        .register_get("home_x", |npc: &mut NpcView| npc.home.0 as FLOAT)
        .register_get("home_y", |npc: &mut NpcView| npc.home.1 as FLOAT)
        .register_type_with_name::<PlayerView>("Player")
        .register_get("name", |player: &mut PlayerView| player.name.clone())
        .register_get("x", |player: &mut PlayerView| player.position.0 as FLOAT)
        .register_get("y", |player: &mut PlayerView| player.position.1 as FLOAT);

    let push = |queue: &Arc<Mutex<Vec<ScriptCommand>>>| {
        let queue = queue.clone();
        move |command: ScriptCommand| queue.lock().expect("the script queue isn't poisoned").push(command)
    };
    let say = push(queue);
    engine.register_fn("say", move |npc: &mut NpcView, text: &str| {
        say(ScriptCommand::Say { npc: npc.entity, text: text.to_string() })
    });
    let wander = push(queue);
    engine.register_fn("wander", move |npc: &mut NpcView| wander(ScriptCommand::Wander { npc: npc.entity }));
    let move_to = push(queue);
    engine.register_fn("move_to", move |npc: &mut NpcView, x: FLOAT, y: FLOAT| {
        move_to(ScriptCommand::MoveTo { npc: npc.entity, x: x as f32, y: y as f32 })
    });
    let move_to_whole = push(queue);
    engine.register_fn("move_to", move |npc: &mut NpcView, x: INT, y: INT| {
        move_to_whole(ScriptCommand::MoveTo { npc: npc.entity, x: x as f32, y: y as f32 })
    });

    engine.register_fn("random", |n: INT| if n <= 0 { 0 } else { fastrand::i64(0..n) });
    engine.register_fn("chance", |probability: FLOAT| fastrand::f64() < probability);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scripts_with(source: &str) -> Scripts {
        let mut scripts = Scripts::new();
        let mut pack = ContentPack::default();
        pack.scripts.insert("test.rhai".to_string(), source.to_string());
        scripts.load(&pack).unwrap();
        scripts
    }

    fn npc() -> NpcView {
        NpcView { entity: Entity::PLACEHOLDER, name: "Tom".to_string(), position: (1.0, 2.0), home: (1.0, 2.0) }
    }

    #[test]
    fn scripts_queue_commands_instead_of_acting() {
        let scripts =
            scripts_with(r#"fn on_interact(npc, player) { npc.say("Hello, " + player.name + "!"); npc.wander(); }"#);
        let player = PlayerView { name: "Alice".to_string(), position: (0.0, 0.0) };
        scripts.call("test.rhai", "on_interact", (npc(), player));
        assert_eq!(
            scripts.take_commands(),
            vec![
                ScriptCommand::Say { npc: Entity::PLACEHOLDER, text: "Hello, Alice!".to_string() },
                ScriptCommand::Wander { npc: Entity::PLACEHOLDER },
            ]
        );
        assert!(scripts.take_commands().is_empty());
    }

    #[test]
    fn runaway_scripts_are_stopped() {
        let scripts = scripts_with("fn on_idle(npc) { loop { } }");
        scripts.call("test.rhai", "on_idle", (npc(),));
        assert!(scripts.take_commands().is_empty());
    }

    #[test]
    fn eval_and_imports_are_disabled() {
        let mut scripts = Scripts::new();
        let mut pack = ContentPack::default();
        pack.scripts.insert("evil.rhai".to_string(), r#"fn on_idle(npc) { eval("1 + 1"); }"#.to_string());
        assert!(scripts.load(&pack).is_err(), "eval is a disabled keyword");
        pack.scripts.insert("evil.rhai".to_string(), r#"import "other" as other;"#.to_string());
        let scripts = {
            scripts.load(&pack).unwrap();
            scripts
        };
        scripts.call("evil.rhai", "on_idle", (npc(),));
        assert!(scripts.take_commands().is_empty());
    }

    #[test]
    fn missing_hooks_are_skipped() {
        let scripts = scripts_with("fn on_idle(npc) { npc.say(\"hi\"); }");
        scripts.call("test.rhai", "on_interact", (npc(), PlayerView { name: String::new(), position: (0.0, 0.0) }));
        assert!(scripts.take_commands().is_empty());
    }
}
