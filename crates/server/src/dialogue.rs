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
//! Dialogue can affect the game through commands (`<<give_item bread>>`, see
//! `venture_content::dialogue_commands`). While a page runs, commands work on
//! a copy of the player's state ([`PlayerState`]), which the dialogue's
//! functions (`item_count`) read too, so a condition sees what a command just
//! did. When the page is done, the copy is written back.
//!
//! (What an NPC says out loud, for everyone nearby, is different: that's a
//! script calling `npc.say`, handled by [`crate::npcs`].)

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use venture_content::ItemDef;
use venture_content::dialogue::{CompiledDialogue, compile_dialogue};
use venture_content::dialogue_commands::DialogueCommand;
use venture_protocol::{DialogueLine, ServerMessage};
use venture_simulation::TALK_RANGE;
use yarnspinner::prelude::{Dialogue, DialogueEvent, OptionId};
use yarnspinner::runtime::{MemoryVariableStorage, StringTableTextProvider};

use crate::ServerSystems;
use crate::content::LoadedContent;
use crate::entities::{DisplayName, NetworkId, Player, Position};
use crate::inventory::{Change, Inventory, describe_items};
use crate::link::Link;
use crate::npcs::{Npc, NpcInteraction};
use crate::requests::{DialogueChoice, DialogueRequest};
use crate::shops::OpenShop;

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
    /// The player's state while a page runs, shared with the dialogue's functions.
    state: Arc<Mutex<PlayerState>>,
}

/// What a conversation can see and change of the player.
#[derive(Clone, Debug, PartialEq)]
struct PlayerState {
    inventory: Inventory,
}

/// One page of a conversation: what's said until the player has to choose.
struct Page {
    lines: Vec<DialogueLine>,
    options: Vec<(OptionId, String)>,
}

/// What a page's commands did besides changing the player's state.
#[derive(Default)]
struct Effects {
    /// Messages for the player ("You receive: Bread.").
    notices: Vec<String>,
    /// A shop to open when the page is done.
    open_shop: Option<String>,
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
    npcs: Query<&Npc>,
    mut pages: Pages,
) {
    for interaction in interactions.read() {
        let Ok(npc) = npcs.get(interaction.npc) else { continue };
        let Ok((_, name, inventory)) = pages.players.get(interaction.player) else { continue };
        let (Some(node), Some(compiled)) = (&npc.definition.dialogue, &library.0) else { continue };

        let state = Arc::new(Mutex::new(PlayerState { inventory: inventory.clone() }));
        let mut dialogue = new_dialogue(compiled, &name.0, &state);
        if let Err(error) = dialogue.set_node(node.as_str()) {
            warn!("Couldn't start the conversation with NPC \"{}\": {error:?}", npc.definition.id);
            continue;
        }
        let conversation = Conversation { npc: interaction.npc, dialogue, options: Vec::new(), state };
        pages.conversations.0.insert(interaction.player, conversation);
        pages.show_next(interaction.player);
    }
}

/// Carries on conversations with the reply each player picked.
fn answer_choices(mut requests: MessageReader<DialogueRequest>, mut pages: Pages) {
    for request in requests.read() {
        let Some(conversation) = pages.conversations.0.get_mut(&request.player) else { continue };
        match request.choice {
            DialogueChoice::End => {
                pages.conversations.0.remove(&request.player);
            }
            DialogueChoice::Option(index) => {
                // A choice that isn't on offer (a stale or made-up one) is ignored.
                let Some(&option) = conversation.options.get(index as usize) else { continue };
                if conversation.dialogue.set_selected_option(option).is_ok() {
                    pages.show_next(request.player);
                }
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

/// Everything needed to run a page of a conversation and show it.
#[derive(SystemParam)]
struct Pages<'w, 's> {
    conversations: ResMut<'w, Conversations>,
    content: Res<'w, LoadedContent>,
    link: ResMut<'w, Link>,
    open_shops: MessageWriter<'w, OpenShop>,
    players: Query<'w, 's, (&'static Player, &'static DisplayName, &'static mut Inventory)>,
    npc_ids: Query<'w, 's, &'static NetworkId, With<Npc>>,
}

impl Pages<'_, '_> {
    /// Runs a player's conversation to the next choice (or its end), applies
    /// what its commands did, and shows the player the page. A finished
    /// conversation is forgotten.
    fn show_next(&mut self, player_entity: Entity) {
        let Some(conversation) = self.conversations.0.get_mut(&player_entity) else { return };
        let Ok((player, _, mut inventory)) = self.players.get_mut(player_entity) else { return };
        let Ok(&NetworkId(npc_id)) = self.npc_ids.get(conversation.npc) else { return };
        let connection = player.connection;

        // Run the page against a fresh copy of the player's state...
        let state = &conversation.state;
        *lock(state) = PlayerState { inventory: inventory.clone() };
        let mut effects = Effects::default();
        let items = &self.content.pack.items;
        let page = next_page(&mut conversation.dialogue, &mut |command| {
            run_command(command, &mut lock(state), items, &mut effects);
        });
        let page = page.unwrap_or_else(|problem| {
            warn!("A conversation went wrong, so it was ended: {problem}");
            Page { lines: Vec::new(), options: Vec::new() }
        });

        // ...then keep what its commands did.
        let changed = lock(state).inventory.clone();
        if changed != *inventory {
            *inventory = changed;
        }
        for text in effects.notices {
            self.link.send(connection, &ServerMessage::Notice { text });
        }
        let npc = conversation.npc;
        conversation.options = page.options.iter().map(|(id, _)| *id).collect();

        // Opening a shop ends the conversation.
        if let Some(shop) = effects.open_shop {
            self.conversations.0.remove(&player_entity);
            self.link.send(connection, &ServerMessage::DialogueEnded);
            self.open_shops.write(OpenShop { player: player_entity, npc, shop });
            return;
        }
        if page.options.is_empty() {
            self.conversations.0.remove(&player_entity);
            if page.lines.is_empty() {
                self.link.send(connection, &ServerMessage::DialogueEnded);
                return;
            }
        }
        let options = page.options.into_iter().map(|(_, text)| text).collect();
        self.link.send(connection, &ServerMessage::Dialogue { npc: npc_id, lines: page.lines, options });
    }
}

fn lock(state: &Mutex<PlayerState>) -> std::sync::MutexGuard<'_, PlayerState> {
    state.lock().expect("a conversation's state isn't poisoned")
}

/// A fresh conversation over the compiled dialogue, with the player called
/// `player_name`, whose state the dialogue's functions read from `state`.
/// Yarn variables last as long as the conversation.
fn new_dialogue(compiled: &CompiledDialogue, player_name: &str, state: &Arc<Mutex<PlayerState>>) -> Dialogue {
    let mut text = StringTableTextProvider::new();
    text.extend_base_language(compiled.strings.clone());
    let mut dialogue = Dialogue::new(Box::new(MemoryVariableStorage::new()), Box::new(text));
    // The functions listed in `venture_content::dialogue_commands::DIALOGUE_FUNCTIONS`.
    let player_name = player_name.to_string();
    let state = state.clone();
    dialogue
        .library_mut()
        .add_function("player_name", move || player_name.clone())
        .add_function("item_count", move |item: String| lock(&state).inventory.count(&item) as f32);
    dialogue.add_program(compiled.program.clone());
    dialogue
}

/// Runs the dialogue until the player has a choice to make or it's over,
/// calling `on_command` with each command's text as it's reached. Options
/// the dialogue marks unavailable aren't offered.
fn next_page(dialogue: &mut Dialogue, on_command: &mut dyn FnMut(&str)) -> Result<Page, String> {
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
                DialogueEvent::Command(command) => on_command(&command.raw),
                _ => {}
            }
        }
    }
    Err(format!("it ran {MAX_STEPS_PER_PAGE} steps without asking the player anything"))
}

/// Carries out one dialogue command. (Commands were checked when the content
/// loaded, so problems here are logged rather than shown to the player,
/// except for a full bag.)
fn run_command(text: &str, state: &mut PlayerState, items: &BTreeMap<String, ItemDef>, effects: &mut Effects) {
    let command = match DialogueCommand::parse(text) {
        Ok(command) => command,
        Err(problem) => {
            warn!("Dialogue: {problem}");
            return;
        }
    };
    match command {
        DialogueCommand::GiveItem { item, count } => {
            let given = describe_items(&item, count, items);
            match state.inventory.apply(&Change::give(&item, count), items) {
                Ok(()) => effects.notices.push(format!("You receive: {given}.")),
                Err(_) => effects.notices.push(format!("You have no room in your bag for {given}.")),
            }
        }
        DialogueCommand::TakeItem { item, count } => match state.inventory.apply(&Change::take(&item, count), items) {
            Ok(()) => effects.notices.push(format!("You hand over: {}.", describe_items(&item, count, items))),
            Err(_) => warn!("Dialogue: <<{text}>> ran when the player didn't have them (check with item_count first)."),
        },
        DialogueCommand::OpenShop { shop } => effects.open_shop = Some(shop),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> BTreeMap<String, ItemDef> {
        let item = ItemDef {
            id: "bread".to_string(),
            name: "Bread".to_string(),
            description: String::new(),
            icon: String::new(),
            stackable: false,
            value: 4,
        };
        BTreeMap::from([("bread".to_string(), item)])
    }

    /// A conversation at node "Start", with an empty bag.
    fn dialogue(source: &str) -> (Dialogue, Arc<Mutex<PlayerState>>) {
        let files = BTreeMap::from([("test.yarn".to_string(), source.to_string())]);
        let compiled = compile_dialogue(&files).unwrap().unwrap();
        let state = Arc::new(Mutex::new(PlayerState { inventory: Inventory::empty() }));
        let mut dialogue = new_dialogue(&compiled, "Alice", &state);
        dialogue.set_node("Start").unwrap();
        (dialogue, state)
    }

    /// Runs a page, carrying out its commands.
    fn run_page(dialogue: &mut Dialogue, state: &Mutex<PlayerState>, effects: &mut Effects) -> Result<Page, String> {
        next_page(dialogue, &mut |command| run_command(command, &mut lock(state), &items(), effects))
    }

    #[test]
    fn a_page_runs_to_the_next_choice() {
        let (mut dialogue, state) = dialogue(
            "title: Start\n---\nTom: Hello, {player_name()}.\nNice day.\n-> Hi!\n    Tom: Hi yourself.\n-> Bye.\n===\n",
        );
        let page = run_page(&mut dialogue, &state, &mut Effects::default()).unwrap();
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
        let page = run_page(&mut dialogue, &state, &mut Effects::default()).unwrap();
        assert_eq!(page.lines[0].text, "Hi yourself.");
        assert!(page.options.is_empty(), "the conversation is over");
    }

    #[test]
    fn unavailable_options_are_not_offered() {
        let (mut dialogue, state) = dialogue("title: Start\n---\n-> Secret <<if false>>\n-> Open\n===\n");
        let page = run_page(&mut dialogue, &state, &mut Effects::default()).unwrap();
        let options: Vec<&str> = page.options.iter().map(|(_, text)| text.as_str()).collect();
        assert_eq!(options, ["Open"]);
    }

    #[test]
    fn commands_change_the_state_that_conditions_see() {
        let source = "title: Start\n---\n<<give_item bread 2>>\n<<if item_count(\"bread\") == 2>>\nTwo loaves.\n<<endif>>\n<<take_item bread>>\n<<open_shop bakery>>\n===\n";
        let (mut dialogue, state) = dialogue(source);
        let mut effects = Effects::default();
        let page = run_page(&mut dialogue, &state, &mut effects).unwrap();
        assert_eq!(page.lines[0].text, "Two loaves.");
        assert_eq!(lock(&state).inventory.count("bread"), 1);
        assert_eq!(effects.notices, ["You receive: Bread (2).", "You hand over: Bread."]);
        assert_eq!(effects.open_shop.as_deref(), Some("bakery"));
    }

    #[test]
    fn endless_conversations_are_cut_short() {
        // (Loops that say nothing are refused when the content loads.)
        let (mut dialogue, state) = dialogue("title: Start\n---\nAnd again.\n<<jump Start>>\n===\n");
        assert!(run_page(&mut dialogue, &state, &mut Effects::default()).is_err());
    }
}
