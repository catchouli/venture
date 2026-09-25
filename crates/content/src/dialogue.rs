//! Conversations with NPCs, written in [Yarn Spinner](https://yarnspinner.dev)
//! files (`dialogue/*.yarn`). All the files are compiled together, so an
//! NPC's `dialogue` can name a node in any of them, and nodes can jump
//! between files.
//!
//! Lines can use `{player_name()}`, the name of the player in the conversation.
//!
//! ```text
//! title: Guard
//! ---
//! Guard Aldren: Halt! Oh, a new face. Welcome to town, {player_name()}.
//! -> Where am I?
//!     Guard Aldren: This is the town square. The smithy's just east.
//! -> Goodbye.
//! ===
//! ```

use std::collections::{BTreeMap, BTreeSet};

use yarnspinner::core::Node;
use yarnspinner::prelude::{StringTable, YarnCompiler, YarnFile, YarnProgram};

/// Yarn Spinner's instruction codes that the loop check needs (see
/// `yarnspinner_core`'s `OpCode`, which isn't exported).
mod opcode {
    pub const JUMP_TO: i32 = 0;
    pub const JUMP: i32 = 1;
    pub const RUN_LINE: i32 = 2;
    pub const RUN_COMMAND: i32 = 3;
    pub const SHOW_OPTIONS: i32 = 5;
    pub const PUSH_STRING: i32 = 6;
    pub const JUMP_IF_FALSE: i32 = 10;
    pub const STOP: i32 = 15;
    pub const RUN_NODE: i32 = 16;
}

/// Every conversation, compiled and ready to run.
#[derive(Clone, Debug)]
pub struct CompiledDialogue {
    /// The compiled nodes.
    pub program: YarnProgram,
    /// The text of every line, by line id.
    pub strings: StringTable,
}

impl CompiledDialogue {
    /// Whether there's a node with this title.
    pub fn has_node(&self, title: &str) -> bool {
        self.program.nodes.contains_key(title)
    }
}

/// Compiles every dialogue file (file name to source). Returns `None` if
/// there are no files, and the problems if they don't compile or jump to
/// nodes that don't exist.
pub fn compile_dialogue(files: &BTreeMap<String, String>) -> Result<Option<CompiledDialogue>, String> {
    if files.is_empty() {
        return Ok(None);
    }
    let yarn_files = files.iter().map(|(name, source)| YarnFile { file_name: name.clone(), source: source.clone() });
    let compilation = YarnCompiler::new().add_files(yarn_files).compile().map_err(|error| error.to_string())?;
    let Some(program) = compilation.program else { return Ok(None) };
    let strings = compilation.string_table.into_iter().map(|(id, info)| (id, info.text)).collect();
    let dialogue = CompiledDialogue { program, strings };

    // The compiler doesn't check jumps, so a misspelt one would only show up
    // when a player reached it.
    let mut problems = Vec::new();
    for (file, source) in files {
        for (number, line) in source.lines().enumerate() {
            if let Some(target) = jump_target(line)
                && !dialogue.has_node(target)
            {
                problems.push(format!("{file}:{}: there's no node titled \"{target}\" to jump to", number + 1));
            }
        }
    }
    if problems.is_empty() {
        problems.extend(find_silent_loops(&dialogue.program));
    }
    if problems.is_empty() { Ok(Some(dialogue)) } else { Err(problems.join("\n")) }
}

/// Finds nodes that can jump round in a circle without saying anything or
/// asking anything, which would run forever. (A loop that says something
/// pauses each time round, and the server limits how far it runs.)
fn find_silent_loops(program: &YarnProgram) -> Vec<String> {
    let mut problems = Vec::new();
    // For each node, the nodes it can jump to without pausing.
    let mut jumps = BTreeMap::new();
    for (name, node) in &program.nodes {
        match silent_jumps(node) {
            Ok(targets) => {
                jumps.insert(name.as_str(), targets);
            }
            Err(problem) => problems.push(format!("node \"{name}\": {problem}")),
        }
    }
    for &start in jumps.keys() {
        // Can `start` get back to itself through silent jumps?
        let mut seen = BTreeSet::new();
        let mut to_visit: Vec<&str> = jumps[start].iter().map(String::as_str).collect();
        while let Some(name) = to_visit.pop() {
            if name == start {
                problems.push(format!(
                    "node \"{start}\" can jump back round to itself without saying anything, so it would never end"
                ));
                break;
            }
            if seen.insert(name) {
                to_visit.extend(jumps.get(name).into_iter().flatten().map(String::as_str));
            }
        }
    }
    problems
}

/// The nodes a node can jump to before it pauses (to say something, run a
/// command or offer options) or ends.
fn silent_jumps(node: &Node) -> Result<BTreeSet<String>, String> {
    let instructions = &node.instructions;
    let label = |instruction: usize| -> Option<usize> {
        let name = String::try_from(instructions[instruction].operands.first()?.clone()).ok()?;
        node.labels.get(&name).map(|&position| position as usize)
    };
    let mut targets = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut to_visit = vec![0];
    while let Some(position) = to_visit.pop() {
        if position >= instructions.len() || !seen.insert(position) {
            continue;
        }
        match instructions[position].opcode {
            opcode::RUN_LINE | opcode::RUN_COMMAND | opcode::SHOW_OPTIONS | opcode::STOP => {}
            opcode::JUMP_TO => to_visit.extend(label(position)),
            opcode::JUMP_IF_FALSE => to_visit.extend([position + 1].into_iter().chain(label(position))),
            // A jump to a label chosen at run time: it could be any of them.
            opcode::JUMP => to_visit.extend(node.labels.values().map(|&position| position as usize)),
            opcode::RUN_NODE => {
                let pushed = position.checked_sub(1).map(|previous| &instructions[previous]);
                let target = pushed
                    .filter(|instruction| instruction.opcode == opcode::PUSH_STRING)
                    .and_then(|instruction| String::try_from(instruction.operands.first()?.clone()).ok());
                match target {
                    Some(target) => {
                        targets.insert(target);
                    }
                    None => return Err("jumps to a node chosen while running, which isn't supported".to_string()),
                }
            }
            _ => to_visit.push(position + 1),
        }
    }
    Ok(targets)
}

/// The node a line jumps to, if it's a plain `<<jump Name>>`.
fn jump_target(line: &str) -> Option<&str> {
    let target = line.trim().strip_prefix("<<jump ")?.strip_suffix(">>")?.trim();
    (!target.contains('{')).then_some(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialogue_compiles_and_knows_its_nodes() {
        let files = BTreeMap::from([("a.yarn".to_string(), "title: Start\n---\nHello!\n===\n".to_string())]);
        let dialogue = compile_dialogue(&files).unwrap().unwrap();
        assert!(dialogue.has_node("Start"));
        assert!(!dialogue.has_node("Missing"));
    }

    #[test]
    fn mistakes_are_reported() {
        let files = BTreeMap::from([("a.yarn".to_string(), "title: Start\n---\n<<if true>>\nHi\n===\n".to_string())]);
        assert!(compile_dialogue(&files).is_err());
        let files = BTreeMap::from([("a.yarn".to_string(), "title: Start\n---\n<<jump Nowhere>>\n===\n".to_string())]);
        assert_eq!(compile_dialogue(&files).unwrap_err(), "a.yarn:3: there's no node titled \"Nowhere\" to jump to");
        assert!(compile_dialogue(&BTreeMap::new()).unwrap().is_none());
    }

    #[test]
    fn silent_loops_are_caught() {
        let silent = "title: A\n---\n<<if true>>\n<<jump B>>\n<<endif>>\nHello.\n===\ntitle: B\n---\n<<jump A>>\n===\n";
        let files = BTreeMap::from([("a.yarn".to_string(), silent.to_string())]);
        let problems = compile_dialogue(&files).unwrap_err();
        assert!(problems.contains("node \"A\" can jump back round"), "{problems}");

        // Saying something on the way round is fine: it pauses each time.
        let talkative = "title: A\n---\nHello again.\n<<jump A>>\n===\n";
        let files = BTreeMap::from([("a.yarn".to_string(), talkative.to_string())]);
        assert!(compile_dialogue(&files).is_ok());
    }
}
