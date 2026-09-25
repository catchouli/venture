//! Conversations with NPCs: the dialogue window a player sees when they talk
//! to an NPC. The only code that runs conversations.
//!
//! Each conversation is private to one player and runs here, on the server,
//! from the content's Yarn Spinner files (see `venture_content::dialogue`).
//! The client is only shown one "page" at a time (what the NPC says, and the
//! replies on offer) and sends back which reply was picked, so it never sees
//! branches the player didn't take. Several players can talk to the same NPC
//! at once, each in their own conversation.
//!
//! (What an NPC says out loud, for everyone nearby, is different: that's a
//! script calling `npc.say`, handled by [`crate::npcs`].)

use std::collections::HashMap;

use bevy::prelude::*;
use venture_content::dialogue::{CompiledDialogue, compile_dialogue};
use venture_protocol::{DialogueLine, EntityId, ServerMessage};
use yarnspinner::prelude::{Dialogue, DialogueEvent, OptionId};
use yarnspinner::runtime::{MemoryVariableStorage, StringTableTextProvider};

use crate::ServerSystems;
use crate::content::LoadedContent;
use crate::entities::{DisplayName, NetworkId, Player, Position};
use crate::link::Link;
use crate::npcs::{Npc, NpcInteraction};
use crate::requests::{DialogueChoice, DialogueRequest};

/// A conversation ends when the player gets further than this from the NPC, in metres.
const TALK_RANGE: f32 = 6.0;
/// The most steps (lines, commands, jumps) one page of dialogue may take,
/// so a conversation that loops without asking anything can't stall the server.
const MAX_STEPS_PER_PAGE: usize = 200;
/// The most lines shown on one page; longer runs are cut short.
const MAX_LINES_PER_PAGE: usize = 30;

/// Runs conversations.
pub struct DialoguePlugin;

impl Plugin for DialoguePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DialogueLibrary>().init_resource::<Conversations>().add_systems(
            FixedUpdate,
            (compile_when_content_changes, start_conversations, answer_choices, end_distant_conversations)
                .chain()
                .in_set(ServerSystems::Dialogue),
        );
    }
}

/// The content's dialogue, compiled.
#[derive(Resource, Default)]
struct DialogueLibrary(Option<CompiledDialogue>);

/// The conversations going on, by player entity.
#[derive(Resource, Default)]
struct Conversations(HashMap<Entity, Conversation>);

/// One player's conversation with an NPC.
struct Conversation {
    npc: Entity,
    dialogue: Dialogue,
    /// The replies on offer, in the order the player was shown them.
    options: Vec<OptionId>,
}

/// One page of a conversation: what's said until the player has to choose.
struct Page {
    lines: Vec<DialogueLine>,
    options: Vec<(OptionId, String)>,
}

fn compile_when_content_changes(content: Res<LoadedContent>, mut library: ResMut<DialogueLibrary>) {
    if !content.is_changed() {
        return;
    }
    // The pack was checked when it loaded, so this doesn't fail in practice.
    // Conversations already going on carry on with the dialogue they started with.
    match compile_dialogue(&content.pack.dialogue) {
        Ok(compiled) => library.0 = compiled,
        Err(problem) => error!("Couldn't compile the dialogue: {problem}"),
    }
}

/// Opens the NPC's conversation when a player talks to it (replacing any
/// conversation the player was having).
fn start_conversations(
    mut interactions: MessageReader<NpcInteraction>,
    library: Res<DialogueLibrary>,
    mut conversations: ResMut<Conversations>,
    mut link: ResMut<Link>,
    npcs: Query<(&Npc, &NetworkId)>,
    players: Query<(&Player, &DisplayName)>,
) {
    for interaction in interactions.read() {
        let Ok((npc, &NetworkId(npc_id))) = npcs.get(interaction.npc) else { continue };
        let Ok((player, name)) = players.get(interaction.player) else { continue };
        let (Some(node), Some(compiled)) = (&npc.definition.dialogue, &library.0) else { continue };

        let mut dialogue = new_dialogue(compiled, &name.0);
        if let Err(error) = dialogue.set_node(node.as_str()) {
            warn!("Couldn't start the conversation with NPC \"{}\": {error:?}", npc.definition.id);
            continue;
        }
        let conversation = Conversation { npc: interaction.npc, dialogue, options: Vec::new() };
        conversations.0.insert(interaction.player, conversation);
        show_next_page(interaction.player, player, npc_id, &mut conversations, &mut link);
    }
}

/// Carries on conversations with the reply each player picked.
fn answer_choices(
    mut requests: MessageReader<DialogueRequest>,
    mut conversations: ResMut<Conversations>,
    mut link: ResMut<Link>,
    npcs: Query<&NetworkId, With<Npc>>,
    players: Query<&Player>,
) {
    for request in requests.read() {
        let Ok(player) = players.get(request.player) else { continue };
        let Some(conversation) = conversations.0.get_mut(&request.player) else { continue };
        match request.choice {
            DialogueChoice::End => {
                conversations.0.remove(&request.player);
            }
            DialogueChoice::Option(index) => {
                // A choice that isn't on offer (a stale or made-up one) is ignored.
                let Some(&option) = conversation.options.get(index as usize) else { continue };
                if conversation.dialogue.set_selected_option(option).is_err() {
                    continue;
                }
                let Ok(&NetworkId(npc_id)) = npcs.get(conversation.npc) else { continue };
                show_next_page(request.player, player, npc_id, &mut conversations, &mut link);
            }
        }
    }
}

/// Ends conversations when the player walks away, or the NPC or player is gone.
fn end_distant_conversations(
    mut conversations: ResMut<Conversations>,
    mut link: ResMut<Link>,
    positions: Query<&Position>,
    players: Query<&Player>,
) {
    conversations.0.retain(|&player_entity, conversation| {
        let Ok(player) = players.get(player_entity) else { return false };
        let in_range = match (positions.get(player_entity), positions.get(conversation.npc)) {
            (Ok(&Position(a)), Ok(&Position(b))) => (a.0 - b.0).hypot(a.1 - b.1) <= TALK_RANGE,
            _ => false,
        };
        if !in_range {
            link.send(player.connection, &ServerMessage::DialogueEnded);
        }
        in_range
    });
}

/// Runs a player's conversation to the next choice (or its end) and shows
/// them the page. A finished conversation is forgotten.
fn show_next_page(
    player_entity: Entity,
    player: &Player,
    npc: EntityId,
    conversations: &mut Conversations,
    link: &mut Link,
) {
    let Some(conversation) = conversations.0.get_mut(&player_entity) else { return };
    let page = match next_page(&mut conversation.dialogue) {
        Ok(page) => page,
        Err(problem) => {
            warn!("A conversation went wrong, so it was ended: {problem}");
            Page { lines: Vec::new(), options: Vec::new() }
        }
    };
    conversation.options = page.options.iter().map(|(id, _)| *id).collect();
    if page.options.is_empty() {
        conversations.0.remove(&player_entity);
        if page.lines.is_empty() {
            link.send(player.connection, &ServerMessage::DialogueEnded);
            return;
        }
    }
    let options = page.options.into_iter().map(|(_, text)| text).collect();
    link.send(player.connection, &ServerMessage::Dialogue { npc, lines: page.lines, options });
}

/// A fresh conversation over the compiled dialogue, with the player called
/// `player_name`. Variables last as long as the conversation.
fn new_dialogue(compiled: &CompiledDialogue, player_name: &str) -> Dialogue {
    let mut text = StringTableTextProvider::new();
    text.extend_base_language(compiled.strings.clone());
    let mut dialogue = Dialogue::new(Box::new(MemoryVariableStorage::new()), Box::new(text));
    let player_name = player_name.to_string();
    dialogue.library_mut().add_function("player_name", move || player_name.clone());
    dialogue.add_program(compiled.program.clone());
    dialogue
}

/// Runs the dialogue until the player has a choice to make or it's over.
/// Options the dialogue marks unavailable aren't offered.
fn next_page(dialogue: &mut Dialogue) -> Result<Page, String> {
    let mut page = Page { lines: Vec::new(), options: Vec::new() };
    for _ in 0..MAX_STEPS_PER_PAGE {
        for event in dialogue.continue_().map_err(|error| format!("{error:?}"))? {
            match event {
                DialogueEvent::Line(line) if page.lines.len() < MAX_LINES_PER_PAGE => {
                    let speaker = line.character_name().map(str::to_string);
                    page.lines.push(DialogueLine { speaker, text: line.text_without_character_name() });
                }
                DialogueEvent::Options(options) => {
                    page.options = options
                        .into_iter()
                        .filter(|option| option.is_available)
                        .map(|option| (option.id, option.line.text_without_character_name()))
                        .collect();
                    return Ok(page);
                }
                DialogueEvent::DialogueComplete => return Ok(page),
                DialogueEvent::Command(command) => debug!("Dialogue command ignored: {}", command.name),
                _ => {}
            }
        }
    }
    Err(format!("it ran {MAX_STEPS_PER_PAGE} steps without asking the player anything"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn dialogue(source: &str) -> Dialogue {
        let files = BTreeMap::from([("test.yarn".to_string(), source.to_string())]);
        let compiled = compile_dialogue(&files).unwrap().unwrap();
        let mut dialogue = new_dialogue(&compiled, "Alice");
        dialogue.set_node("Start").unwrap();
        dialogue
    }

    #[test]
    fn a_page_runs_to_the_next_choice() {
        let mut dialogue = dialogue(
            "title: Start\n---\nTom: Hello, {player_name()}.\nNice day.\n-> Hi!\n    Tom: Hi yourself.\n-> Bye.\n===\n",
        );
        let page = next_page(&mut dialogue).unwrap();
        assert_eq!(
            page.lines,
            vec![
                DialogueLine { speaker: Some("Tom".to_string()), text: "Hello, Alice.".to_string() },
                DialogueLine { speaker: None, text: "Nice day.".to_string() },
            ]
        );
        let options: Vec<&str> = page.options.iter().map(|(_, text)| text.as_str()).collect();
        assert_eq!(options, ["Hi!", "Bye."]);

        dialogue.set_selected_option(page.options[0].0).unwrap();
        let page = next_page(&mut dialogue).unwrap();
        assert_eq!(page.lines[0].text, "Hi yourself.");
        assert!(page.options.is_empty(), "the conversation is over");
    }

    #[test]
    fn unavailable_options_are_not_offered() {
        let mut dialogue = dialogue("title: Start\n---\n-> Secret <<if false>>\n-> Open\n===\n");
        let page = next_page(&mut dialogue).unwrap();
        let options: Vec<&str> = page.options.iter().map(|(_, text)| text.as_str()).collect();
        assert_eq!(options, ["Open"]);
    }

    #[test]
    fn endless_conversations_are_cut_short() {
        // (Loops that say nothing are refused when the content loads.)
        let mut dialogue = dialogue("title: Start\n---\nAnd again.\n<<jump Start>>\n===\n");
        assert!(next_page(&mut dialogue).is_err());
    }
}
