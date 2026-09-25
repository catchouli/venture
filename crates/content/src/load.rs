//! Loading and checking a content pack.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use crate::client::{ClientContent, parse_appearances, parse_items};
use crate::definitions::{
    AppearanceDef, COINS, ItemDef, MAX_ITEM_DESCRIPTION_CHARS, MAX_ITEM_NAME_CHARS, MAX_NPC_NAME_CHARS,
    MAX_WANDER_RADIUS, NewCharacterDef, NpcDef, Placement, ShopDef,
};

/// The file saying what new characters start with.
const NEW_CHARACTERS_FILE: &str = "new_characters.ron";
/// The largest an icon file may be, in bytes.
const MAX_ICON_BYTES: usize = 64 * 1024;
/// The first bytes of every PNG file.
const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// A problem with the content pack, and which file it's in.
#[derive(Clone, Debug, PartialEq)]
pub struct ContentError {
    /// The file, relative to the content folder.
    pub file: PathBuf,
    /// What's wrong, in plain words.
    pub message: String,
}

impl std::fmt::Display for ContentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.file.display(), self.message)
    }
}

/// A loaded, checked content pack.
#[derive(Clone, Debug, Default)]
pub struct ContentPack {
    /// Appearances, by id.
    pub appearances: BTreeMap<String, AppearanceDef>,
    /// Items, by id.
    pub items: BTreeMap<String, ItemDef>,
    /// What new characters start with.
    pub new_characters: NewCharacterDef,
    /// Shops, by id.
    pub shops: BTreeMap<String, ShopDef>,
    /// NPCs, in file order.
    pub npcs: Vec<NpcDef>,
    /// Script sources, by file name within `scripts/`.
    pub scripts: BTreeMap<String, String>,
    /// Dialogue sources, by file name within `dialogue/`.
    pub dialogue: BTreeMap<String, String>,
    /// The part clients download.
    pub client: ClientContent,
}

impl ContentPack {
    /// Loads a content pack folder, checking everything. Returns every problem
    /// found, or the pack if there were none.
    pub fn load(folder: &Path) -> Result<Self, Vec<ContentError>> {
        let mut errors = Vec::new();
        let mut pack = ContentPack::default();
        let mut client_files = Vec::new();

        for (relative, text) in read_files(folder, "appearances", "ron", &mut errors) {
            match parse_appearances(&text) {
                Ok(appearances) => {
                    for appearance in appearances {
                        check_appearance(&appearance, &relative, &mut errors);
                        if pack.appearances.contains_key(&appearance.id) {
                            errors.push(error(
                                &relative,
                                format!("there's already an appearance called \"{}\"", appearance.id),
                            ));
                        }
                        pack.appearances.insert(appearance.id.clone(), appearance);
                    }
                    client_files.push((to_slashes(&relative), text.into_bytes()));
                }
                Err(message) => errors.push(error(&relative, message)),
            }
        }

        let mut icons = HashSet::new();
        for (relative, bytes) in read_binary_files(folder, "icons", "png", &mut errors) {
            if !bytes.starts_with(&PNG_SIGNATURE) {
                errors.push(error(&relative, "this isn't a PNG image".to_string()));
            } else if bytes.len() > MAX_ICON_BYTES {
                errors.push(error(&relative, format!("icons must be at most {} KB", MAX_ICON_BYTES / 1024)));
            }
            icons.insert(file_name(&relative));
            client_files.push((to_slashes(&relative), bytes));
        }

        for (relative, text) in read_files(folder, "items", "ron", &mut errors) {
            match parse_items(&text) {
                Ok(items) => {
                    for item in items {
                        check_item(&item, &icons, &relative, &mut errors);
                        if pack.items.contains_key(&item.id) {
                            errors.push(error(&relative, format!("there's already an item called \"{}\"", item.id)));
                        }
                        pack.items.insert(item.id.clone(), item);
                    }
                    client_files.push((to_slashes(&relative), text.into_bytes()));
                }
                Err(message) => errors.push(error(&relative, message)),
            }
        }
        if !pack.items.is_empty() && !pack.items.contains_key(COINS) {
            errors.push(error(Path::new("items"), format!("there must be an item called \"{COINS}\" (money)")));
        }

        if let Ok(text) = std::fs::read_to_string(folder.join(NEW_CHARACTERS_FILE)) {
            let file = Path::new(NEW_CHARACTERS_FILE);
            match ron::from_str::<NewCharacterDef>(&text) {
                Ok(new_characters) => {
                    for stack in &new_characters.items {
                        if !pack.items.contains_key(&stack.item) {
                            errors.push(error(file, format!("there's no item called \"{}\"", stack.item)));
                        }
                    }
                    pack.new_characters = new_characters;
                }
                Err(message) => errors.push(error(file, message.to_string())),
            }
        }

        for (relative, text) in read_files(folder, "shops", "ron", &mut errors) {
            match ron::from_str::<Vec<ShopDef>>(&text) {
                Ok(shops) => {
                    for shop in shops {
                        check_shop(&shop, &pack, &relative, &mut errors);
                        if pack.shops.contains_key(&shop.id) {
                            errors.push(error(&relative, format!("there's already a shop called \"{}\"", shop.id)));
                        }
                        pack.shops.insert(shop.id.clone(), shop);
                    }
                }
                Err(message) => errors.push(error(&relative, message.to_string())),
            }
        }

        for (relative, text) in read_files(folder, "scripts", "rhai", &mut errors) {
            let name = file_name(&relative);
            #[cfg(feature = "scripts")]
            if let Err(message) = crate::script_hooks::check_script(&text) {
                errors.push(error(&relative, message));
            }
            pack.scripts.insert(name, text);
        }

        for (relative, text) in read_files(folder, "dialogue", "yarn", &mut errors) {
            pack.dialogue.insert(file_name(&relative), text);
        }
        // Compiled together, since nodes can jump between files. The compiler's
        // messages name the file.
        #[cfg(feature = "scripts")]
        let dialogue = crate::dialogue::compile_dialogue(&pack.dialogue).map_err(|message| {
            errors.push(error(Path::new("dialogue"), message));
        });
        #[cfg(feature = "scripts")]
        if let Ok(Some(dialogue)) = &dialogue {
            for (node, command) in dialogue.commands() {
                if let Err(message) = check_dialogue_command(&command, &pack) {
                    errors.push(error(Path::new("dialogue"), format!("node \"{node}\": {message}")));
                }
            }
        }

        let mut npc_ids = HashSet::new();
        for (relative, text) in read_files(folder, "npcs", "ron", &mut errors) {
            match ron::from_str::<Vec<NpcDef>>(&text) {
                Ok(npcs) => {
                    for npc in npcs {
                        check_npc(&npc, &pack, &relative, &mut errors);
                        #[cfg(feature = "scripts")]
                        // (If the dialogue doesn't compile, that's already reported.)
                        if let (Some(node), Ok(dialogue)) = (&npc.dialogue, &dialogue)
                            && !dialogue.as_ref().is_some_and(|dialogue| dialogue.has_node(node))
                        {
                            let message = format!("NPC \"{}\": there's no dialogue node titled \"{node}\"", npc.id);
                            errors.push(error(&relative, message));
                        }
                        if !npc_ids.insert(npc.id.clone()) {
                            errors.push(error(&relative, format!("there's already an NPC called \"{}\"", npc.id)));
                        }
                        pack.npcs.push(npc);
                    }
                }
                Err(message) => errors.push(error(&relative, message.to_string())),
            }
        }

        pack.client = ClientContent::new(client_files);
        if errors.is_empty() { Ok(pack) } else { Err(errors) }
    }
}

fn check_appearance(appearance: &AppearanceDef, file: &Path, errors: &mut Vec<ContentError>) {
    let colours = [
        ("skin", appearance.skin),
        ("hair", appearance.hair),
        ("tunic", appearance.tunic),
        ("trousers", appearance.trousers),
    ];
    for (part, (r, g, b)) in colours {
        if ![r, g, b].iter().all(|channel| (0.0..=1.0).contains(channel)) {
            errors.push(error(
                file,
                format!("appearance \"{}\": {part} colour values must be between 0 and 1", appearance.id),
            ));
        }
    }
}

fn check_item(item: &ItemDef, icons: &HashSet<String>, file: &Path, errors: &mut Vec<ContentError>) {
    let mut problem = |message: String| errors.push(error(file, format!("item \"{}\": {message}", item.id)));
    let name_length = item.name.trim().chars().count();
    if name_length == 0 || name_length > MAX_ITEM_NAME_CHARS {
        problem(format!("its name must be 1 to {MAX_ITEM_NAME_CHARS} characters"));
    }
    if item.description.chars().count() > MAX_ITEM_DESCRIPTION_CHARS {
        problem(format!("its description must be at most {MAX_ITEM_DESCRIPTION_CHARS} characters"));
    }
    if !icons.contains(&item.icon) {
        problem(format!("there's no icon called \"{}\" in icons/", item.icon));
    }
}

fn check_shop(shop: &ShopDef, pack: &ContentPack, file: &Path, errors: &mut Vec<ContentError>) {
    let mut problem = |message: String| errors.push(error(file, format!("shop \"{}\": {message}", shop.id)));
    if shop.name.trim().is_empty() {
        problem("it needs a name".to_string());
    }
    for stock in &shop.stock {
        if !pack.items.contains_key(&stock.item) {
            problem(format!("there's no item called \"{}\"", stock.item));
        }
        if stock.price == 0 {
            problem(format!("{} needs a price of at least 1 coin", stock.item));
        }
    }
}

/// Checks a dialogue command exists and refers to things that do.
#[cfg(feature = "scripts")]
fn check_dialogue_command(text: &str, pack: &ContentPack) -> Result<(), String> {
    use crate::dialogue_commands::DialogueCommand;
    match DialogueCommand::parse(text)? {
        DialogueCommand::GiveItem { item, .. } | DialogueCommand::TakeItem { item, .. } => {
            if !pack.items.contains_key(&item) {
                return Err(format!("<<{text}>>: there's no item called \"{item}\""));
            }
        }
        DialogueCommand::OpenShop { shop } => {
            if !pack.shops.contains_key(&shop) {
                return Err(format!("<<{text}>>: there's no shop called \"{shop}\""));
            }
        }
    }
    Ok(())
}

fn check_npc(npc: &NpcDef, pack: &ContentPack, file: &Path, errors: &mut Vec<ContentError>) {
    let mut problem = |message: String| errors.push(error(file, format!("NPC \"{}\": {message}", npc.id)));
    let name_length = npc.name.trim().chars().count();
    if name_length == 0 || name_length > MAX_NPC_NAME_CHARS {
        problem(format!("its name must be 1 to {MAX_NPC_NAME_CHARS} characters"));
    }
    if !pack.appearances.contains_key(&npc.appearance) {
        problem(format!("there's no appearance called \"{}\"", npc.appearance));
    }
    if let Some(script) = &npc.script
        && !pack.scripts.contains_key(script)
    {
        problem(format!("there's no script called \"{script}\" in scripts/"));
    }
    if !(0.0..=MAX_WANDER_RADIUS).contains(&npc.wander_radius) {
        problem(format!("wander_radius must be between 0 and {MAX_WANDER_RADIUS} metres"));
    }
    let (x, y) = match npc.at {
        Placement::StartingSettlement { offset } | Placement::Settlement { offset, .. } => offset,
        Placement::Position(position) => position,
    };
    if !x.is_finite() || !y.is_finite() {
        problem("its position must be a real number".to_string());
    }
}

/// Reads every text file with the given extension in a subfolder (see
/// [`read_binary_files`]).
fn read_files(
    folder: &Path,
    subfolder: &str,
    extension: &str,
    errors: &mut Vec<ContentError>,
) -> Vec<(PathBuf, String)> {
    read_binary_files(folder, subfolder, extension, errors)
        .into_iter()
        .filter_map(|(relative, bytes)| match String::from_utf8(bytes) {
            Ok(text) => Some((relative, text)),
            Err(_) => {
                errors.push(error(&relative, "it isn't UTF-8 text".to_string()));
                None
            }
        })
        .collect()
}

/// Reads every file with the given extension in a subfolder (sorted by
/// name, so loading is repeatable), returning paths relative to the pack.
/// A missing subfolder is fine: it just has no files.
fn read_binary_files(
    folder: &Path,
    subfolder: &str,
    extension: &str,
    errors: &mut Vec<ContentError>,
) -> Vec<(PathBuf, Vec<u8>)> {
    let Ok(entries) = std::fs::read_dir(folder.join(subfolder)) else { return Vec::new() };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == extension))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let relative = path.strip_prefix(folder).unwrap_or(&path).to_path_buf();
            match std::fs::read(&path) {
                Ok(bytes) => Some((relative, bytes)),
                Err(problem) => {
                    errors.push(error(&relative, format!("can't read it: {problem}")));
                    None
                }
            }
        })
        .collect()
}

fn error(file: &Path, message: String) -> ContentError {
    ContentError { file: file.to_path_buf(), message }
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
}

fn to_slashes(path: &Path) -> String {
    path.components().map(|part| part.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A content folder in the temporary directory, deleted afterwards.
    struct TestPack(PathBuf);

    impl TestPack {
        fn new(name: &str, files: &[(&str, &str)]) -> Self {
            let folder = std::env::temp_dir().join(format!("venture-content-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&folder);
            for (path, text) in files {
                let path = folder.join(path);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, text).unwrap();
            }
            Self(folder)
        }
    }

    impl Drop for TestPack {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const APPEARANCE: &str = r#"[(id: "villager", skin: (0.8, 0.6, 0.5), hair: (0.3, 0.2, 0.1), tunic: (0.3, 0.4, 0.2), trousers: (0.3, 0.3, 0.2))]"#;

    #[test]
    fn a_good_pack_loads() {
        let npcs = r#"[(id: "tom", name: "Old Tom", appearance: "villager", at: StartingSettlement(offset: (3.0, 4.0)), wander_radius: 5.0)]"#;
        let folder = TestPack::new("good", &[("appearances/people.ron", APPEARANCE), ("npcs/town.ron", npcs)]);
        let pack = ContentPack::load(&folder.0).unwrap();
        assert_eq!(pack.npcs.len(), 1);
        assert_eq!(pack.client.manifest.files.len(), 1, "only appearances go to clients");
    }

    #[test]
    fn items_need_icons_and_coins() {
        let items = r#"[(id: "bread", name: "Bread", description: "A loaf.", icon: "bread.png", value: 4)]"#;
        let folder = TestPack::new("items", &[("items/food.ron", items), ("icons/loaf.png", "not a picture")]);
        let messages: Vec<String> = ContentPack::load(&folder.0).unwrap_err().iter().map(ToString::to_string).collect();
        assert_eq!(
            messages,
            [
                "icons/loaf.png: this isn't a PNG image",
                "items/food.ron: item \"bread\": there's no icon called \"bread.png\" in icons/",
                "items: there must be an item called \"coins\" (money)",
            ]
        );
    }

    #[test]
    fn every_problem_is_reported_with_its_file() {
        let npcs = r#"[
            (id: "tom", name: "", appearance: "nobody", at: Position((1.0, 1.0)), script: Some("missing.rhai")),
            (id: "tom", name: "Tom again", appearance: "villager", at: Position((1.0, 1.0))),
        ]"#;
        let folder = TestPack::new("bad", &[("appearances/people.ron", APPEARANCE), ("npcs/town.ron", npcs)]);
        let errors = ContentPack::load(&folder.0).unwrap_err();
        let messages: Vec<String> = errors.iter().map(ToString::to_string).collect();
        assert_eq!(errors.len(), 4, "{messages:#?}");
        assert!(errors.iter().all(|error| error.file == Path::new("npcs/town.ron")));
    }

    #[cfg(feature = "scripts")]
    #[test]
    fn npcs_must_name_dialogue_that_exists() {
        let npcs =
            r#"[(id: "tom", name: "Tom", appearance: "villager", at: Position((1.0, 1.0)), dialogue: Some("Tom"))]"#;
        let dialogue = "title: Tim\n---\nHello.\n===\n";
        let files = [("appearances/people.ron", APPEARANCE), ("npcs/town.ron", npcs), ("dialogue/town.yarn", dialogue)];
        let folder = TestPack::new("dialogue", &files);
        let errors = ContentPack::load(&folder.0).unwrap_err();
        assert_eq!(errors[0].to_string(), "npcs/town.ron: NPC \"tom\": there's no dialogue node titled \"Tom\"");
    }

    #[cfg(feature = "scripts")]
    #[test]
    fn dialogue_commands_must_refer_to_real_things() {
        let dialogue = "title: A\n---\n<<open_shop nowhere>>\n<<give_item unicorn 2>>\n<<dance>>\n===\n";
        let folder =
            TestPack::new("commands", &[("appearances/people.ron", APPEARANCE), ("dialogue/a.yarn", dialogue)]);
        let messages: Vec<String> = ContentPack::load(&folder.0).unwrap_err().iter().map(ToString::to_string).collect();
        assert_eq!(messages.len(), 3, "{messages:#?}");
        assert!(messages[0].contains("no shop called \"nowhere\""));
        assert!(messages[1].contains("no item called \"unicorn\""));
        assert!(messages[2].contains("isn't a command the game knows"));
    }

    #[test]
    fn syntax_errors_name_the_file() {
        let folder = TestPack::new("syntax", &[("appearances/broken.ron", "[(id: \"x\", skin: ")]);
        let errors = ContentPack::load(&folder.0).unwrap_err();
        assert_eq!(errors[0].file, Path::new("appearances/broken.ron"));
    }
}
