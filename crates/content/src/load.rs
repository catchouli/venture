//! Loading and checking a content pack.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use crate::characters::{CharacterModels, check_character_models, parse_character_models};
use crate::client::{
    ClientContent, parse_abilities, parse_appearances, parse_game_sounds, parse_items, parse_skills, parse_theme,
};
use crate::definitions::MonsterDef;
use crate::definitions::{
    AbilityDef, AbilityEffect, AppearanceDef, ClassDef, GameSounds, ItemDef, ItemEffect, MAX_HIT_DIE,
    MAX_ITEM_DESCRIPTION_CHARS, MAX_ITEM_NAME_CHARS, MAX_NPC_NAME_CHARS, MAX_WANDER_RADIUS, NewCharacterDef, NpcDef,
    QuestDef, RespawnPoint, ShopDef, SkillDef, SpawnerDef, THEME_PIECES, ThemeDef,
};
use crate::layout::{UiNode, UiStyles, check_layout, check_styles, parse_layout, parse_styles};
use crate::sky::{SKY_CYCLE_FILE, SKY_FILES, SkyLight, parse_sky, parse_sky_cycle};
use venture_protocol::EquipSlot;
use venture_rpg::Healing;
use venture_rpg::MAX_LEVEL;

/// The file saying what new characters start with.
const NEW_CHARACTERS_FILE: &str = "new_characters.ron";
/// The file listing where players wake up after dying.
const RESPAWN_POINTS_FILE: &str = "respawn_points.ron";
/// The file naming the game's own sounds (hits, misses, levelling up).
const SOUNDS_FILE: &str = "sounds.ron";
/// The largest a sound file may be, in bytes.
const MAX_SOUND_BYTES: usize = 512 * 1024;
/// The interface's theme, in `ui/`.
const THEME_FILE: &str = "ui/theme.ron";
/// Character models and animations.
const CHARACTERS_FILE: &str = "characters/characters.ron";
/// The largest a model file (or its texture) may be, in bytes.
const MAX_MODEL_FILE_BYTES: usize = 8 << 20;
/// The interface's text styles and colours.
const STYLES_FILE: &str = "ui/styles.ron";
/// The largest an interface image may be, in bytes.
const MAX_UI_IMAGE_BYTES: usize = 1024 * 1024;
/// The largest a font may be, in bytes.
const MAX_FONT_BYTES: usize = 4 * 1024 * 1024;
/// Ground and surface textures: a folder of pictures for each.
const TEXTURES_FOLDER: &str = "textures";
/// The pictures each texture has (see `docs/textures.md`).
pub const TEXTURE_PICTURES: [&str; 3] = ["colour.jpg", "normal.jpg", "surface.jpg"];
/// Skies: a folder of files for each (see [`crate::sky`]).
const SKIES_FOLDER: &str = "skies";
/// The largest a sky file may be, in bytes.
const MAX_SKY_FILE_BYTES: usize = 4 << 20;
/// The largest a texture picture may be, in bytes.
const MAX_TEXTURE_BYTES: usize = 4 << 20;
/// The first bytes of every JPEG file.
const JPEG_SIGNATURE: [u8; 3] = [0xFF, 0xD8, 0xFF];
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
    /// Quests, by id.
    pub quests: BTreeMap<String, QuestDef>,
    /// Abilities, by id.
    pub abilities: BTreeMap<String, AbilityDef>,
    /// The game's own sounds.
    pub sounds: GameSounds,
    /// Skills (gathering and making), in the order they're shown.
    pub skills: Vec<SkillDef>,
    /// The interface's look.
    pub theme: ThemeDef,
    /// The interface's text styles, colours and bar colours.
    pub ui_styles: UiStyles,
    /// Character models, if the pack has them.
    pub character_models: Option<CharacterModels>,
    /// The interface's layouts, by name (the file name without `.ron`).
    pub layouts: BTreeMap<String, UiNode>,
    /// Monsters, by id.
    pub monsters: BTreeMap<String, MonsterDef>,
    /// Spawners, in file order.
    pub spawners: Vec<SpawnerDef>,
    /// Where players wake up after dying.
    pub respawn_points: Vec<RespawnPoint>,
    /// Classes, in the order players are offered them (the first is the default).
    pub classes: Vec<ClassDef>,
    /// NPCs, in file order.
    pub npcs: Vec<NpcDef>,
    /// Script sources, by file name within `scripts/`.
    pub scripts: BTreeMap<String, String>,
    /// Dialogue sources, by file name within `dialogue/`.
    pub dialogue: BTreeMap<String, String>,
    /// Textures (by name: folders in `textures/`).
    pub textures: BTreeSet<String>,
    /// Skies (by name: folders in `skies/`).
    pub skies: BTreeSet<String>,
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

        let mut sounds = HashSet::new();
        for (relative, bytes) in read_binary_files(folder, "sounds", "wav", &mut errors) {
            if !(bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE".as_slice())) {
                errors.push(error(&relative, "this isn't a WAV sound".to_string()));
            } else if bytes.len() > MAX_SOUND_BYTES {
                errors.push(error(&relative, format!("sounds must be at most {} KB", MAX_SOUND_BYTES / 1024)));
            }
            sounds.insert(file_name(&relative));
            client_files.push((to_slashes(&relative), bytes));
        }
        if let Ok(text) = std::fs::read_to_string(folder.join(SOUNDS_FILE)) {
            let file = Path::new(SOUNDS_FILE);
            match parse_game_sounds(&text) {
                Ok(game_sounds) => {
                    for sound in game_sounds.files().filter(|sound| !sounds.contains(*sound)) {
                        errors.push(error(file, format!("there's no sound called \"{sound}\" in sounds/")));
                    }
                    pack.sounds = game_sounds;
                    client_files.push((SOUNDS_FILE.to_string(), text.into_bytes()));
                }
                Err(message) => errors.push(error(file, message)),
            }
        }

        let mut ui_files = HashSet::new();
        for (relative, bytes) in read_binary_files(folder, "ui", "png", &mut errors) {
            if !bytes.starts_with(&PNG_SIGNATURE) {
                errors.push(error(&relative, "this isn't a PNG image".to_string()));
            } else if bytes.len() > MAX_UI_IMAGE_BYTES {
                errors.push(error(
                    &relative,
                    format!("interface images must be at most {} KB", MAX_UI_IMAGE_BYTES / 1024),
                ));
            }
            ui_files.insert(file_name(&relative));
            client_files.push((to_slashes(&relative), bytes));
        }
        for extension in ["ttf", "otf"] {
            for (relative, bytes) in read_binary_files(folder, "ui", extension, &mut errors) {
                if bytes.len() > MAX_FONT_BYTES {
                    errors.push(error(&relative, format!("fonts must be at most {} KB", MAX_FONT_BYTES / 1024)));
                }
                ui_files.insert(file_name(&relative));
                client_files.push((to_slashes(&relative), bytes));
            }
        }
        if let Ok(text) = std::fs::read_to_string(folder.join(THEME_FILE)) {
            let file = Path::new(THEME_FILE);
            match parse_theme(&text) {
                Ok(theme) => {
                    check_theme(&theme, &ui_files, file, &mut errors);
                    pack.theme = theme;
                    client_files.push((THEME_FILE.to_string(), text.into_bytes()));
                }
                Err(message) => errors.push(error(file, message)),
            }
        }

        let ui_file_names: Vec<String> = ui_files.iter().cloned().collect();
        if let Ok(text) = std::fs::read_to_string(folder.join(STYLES_FILE)) {
            let file = Path::new(STYLES_FILE);
            match parse_styles(&text) {
                Ok(styles) => {
                    errors
                        .extend(check_styles(&styles, &ui_file_names).into_iter().map(|message| error(file, message)));
                    pack.ui_styles = styles;
                    client_files.push((STYLES_FILE.to_string(), text.into_bytes()));
                }
                Err(message) => errors.push(error(file, message)),
            }
        }
        let piece_names: Vec<&str> = THEME_PIECES.iter().map(|(name, _)| *name).collect();
        for (relative, text) in read_files(folder, "ui/layouts", "ron", &mut errors) {
            let name = file_name(&relative).trim_end_matches(".ron").to_string();
            match parse_layout(&text) {
                Ok(root) => {
                    let problems = check_layout(&name, &root, &pack.ui_styles, &piece_names, &ui_file_names);
                    errors.extend(problems.into_iter().map(|message| error(&relative, message)));
                    pack.layouts.insert(name, root);
                    client_files.push((to_slashes(&relative), text.into_bytes()));
                }
                Err(message) => errors.push(error(&relative, message)),
            }
        }

        for (relative, text) in read_files(folder, "skills", "ron", &mut errors) {
            match parse_skills(&text) {
                Ok(skills) => {
                    for skill in skills {
                        check_skill(&skill, &icons, &relative, &mut errors);
                        if pack.skills.iter().any(|other| other.id == skill.id) {
                            errors.push(error(&relative, format!("there's already a skill called \"{}\"", skill.id)));
                        }
                        pack.skills.push(skill);
                    }
                    client_files.push((to_slashes(&relative), text.into_bytes()));
                }
                Err(message) => errors.push(error(&relative, message)),
            }
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

        for (relative, text) in read_files(folder, "abilities", "ron", &mut errors) {
            match parse_abilities(&text) {
                Ok(abilities) => {
                    for ability in abilities {
                        check_ability(&ability, &icons, &sounds, &relative, &mut errors);
                        if pack.abilities.contains_key(&ability.id) {
                            errors.push(error(
                                &relative,
                                format!("there's already an ability called \"{}\"", ability.id),
                            ));
                        }
                        pack.abilities.insert(ability.id.clone(), ability);
                    }
                    client_files.push((to_slashes(&relative), text.into_bytes()));
                }
                Err(message) => errors.push(error(&relative, message)),
            }
        }

        for (relative, text) in read_files(folder, "classes", "ron", &mut errors) {
            match ron::from_str::<Vec<ClassDef>>(&text) {
                Ok(classes) => {
                    for class in classes {
                        check_class(&class, &pack, &relative, &mut errors);
                        if pack.classes.iter().any(|other| other.id == class.id) {
                            errors.push(error(&relative, format!("there's already a class called \"{}\"", class.id)));
                        }
                        pack.classes.push(class);
                    }
                }
                Err(message) => errors.push(error(&relative, message.to_string())),
            }
        }
        if !pack.classes.is_empty() && !pack.classes.iter().any(|class| class.starting) {
            errors.push(error(Path::new("classes"), "no class is offered at the start (starting: true)".to_string()));
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
                    for ability in &new_characters.abilities {
                        if !pack.abilities.contains_key(ability) {
                            errors.push(error(file, format!("there's no ability called \"{ability}\"")));
                        }
                    }
                    pack.new_characters = new_characters;
                }
                Err(message) => errors.push(error(file, message.to_string())),
            }
        }

        for (relative, text) in read_files(folder, "monsters", "ron", &mut errors) {
            match ron::from_str::<Vec<MonsterDef>>(&text) {
                Ok(monsters) => {
                    for monster in monsters {
                        check_monster(&monster, &relative, &mut errors);
                        if pack.monsters.contains_key(&monster.id) {
                            errors
                                .push(error(&relative, format!("there's already a monster called \"{}\"", monster.id)));
                        }
                        pack.monsters.insert(monster.id.clone(), monster);
                    }
                }
                Err(message) => errors.push(error(&relative, message.to_string())),
            }
        }
        for (relative, text) in read_files(folder, "spawners", "ron", &mut errors) {
            match ron::from_str::<Vec<SpawnerDef>>(&text) {
                Ok(spawners) => {
                    for spawner in spawners {
                        check_spawner(&spawner, &pack, &relative, &mut errors);
                        if pack.spawners.iter().any(|other| other.id == spawner.id) {
                            errors
                                .push(error(&relative, format!("there's already a spawner called \"{}\"", spawner.id)));
                        }
                        pack.spawners.push(spawner);
                    }
                }
                Err(message) => errors.push(error(&relative, message.to_string())),
            }
        }
        if let Ok(text) = std::fs::read_to_string(folder.join(RESPAWN_POINTS_FILE)) {
            let file = Path::new(RESPAWN_POINTS_FILE);
            match ron::from_str::<Vec<RespawnPoint>>(&text) {
                Ok(points) => {
                    for point in &points {
                        if !point.at.0.is_finite() || !point.at.1.is_finite() {
                            errors.push(error(file, format!("\"{}\": its position must be a real number", point.name)));
                        }
                    }
                    pack.respawn_points = points;
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

        for (relative, text) in read_files(folder, "quests", "ron", &mut errors) {
            match ron::from_str::<Vec<QuestDef>>(&text) {
                Ok(quests) => {
                    for quest in quests {
                        check_quest(&quest, &relative, &mut errors);
                        if pack.quests.contains_key(&quest.id) {
                            errors.push(error(&relative, format!("there's already a quest called \"{}\"", quest.id)));
                        }
                        pack.quests.insert(quest.id.clone(), quest);
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

        let mut model_files = Vec::new();
        for extension in ["gltf", "bin", "glb", "png"] {
            for (relative, bytes) in read_binary_files(folder, "characters", extension, &mut errors) {
                if bytes.len() > MAX_MODEL_FILE_BYTES {
                    errors.push(error(
                        &relative,
                        format!("model files must be at most {} MB", MAX_MODEL_FILE_BYTES >> 20),
                    ));
                }
                model_files.push(file_name(&relative));
                client_files.push((to_slashes(&relative), bytes));
            }
        }
        let mut texture_folders: Vec<PathBuf> = std::fs::read_dir(folder.join(TEXTURES_FOLDER))
            .into_iter()
            .flatten()
            .filter_map(|entry| Some(entry.ok()?.path()))
            .filter(|path| path.is_dir())
            .collect();
        texture_folders.sort();
        for texture in texture_folders {
            let name = file_name(&texture);
            let subfolder = format!("{TEXTURES_FOLDER}/{name}");
            let pictures = read_binary_files(folder, &subfolder, "jpg", &mut errors);
            for wanted in TEXTURE_PICTURES {
                if !pictures.iter().any(|(relative, _)| file_name(relative) == wanted) {
                    errors.push(error(Path::new(&subfolder), format!("texture \"{name}\" has no {wanted}")));
                }
            }
            for (relative, bytes) in pictures {
                if !bytes.starts_with(&JPEG_SIGNATURE) {
                    errors.push(error(&relative, "this isn't a JPEG picture".to_string()));
                } else if bytes.len() > MAX_TEXTURE_BYTES {
                    errors.push(error(
                        &relative,
                        format!("texture pictures must be at most {} MB", MAX_TEXTURE_BYTES >> 20),
                    ));
                }
                client_files.push((to_slashes(&relative), bytes));
            }
            pack.textures.insert(name);
        }

        let mut sky_folders: Vec<PathBuf> = std::fs::read_dir(folder.join(SKIES_FOLDER))
            .into_iter()
            .flatten()
            .filter_map(|entry| Some(entry.ok()?.path()))
            .filter(|path| path.is_dir())
            .collect();
        sky_folders.sort();
        for sky in sky_folders {
            let name = file_name(&sky);
            let subfolder = Path::new(SKIES_FOLDER).join(&name);
            for wanted in SKY_FILES {
                let relative = subfolder.join(wanted);
                let bytes = match std::fs::read(folder.join(&relative)) {
                    Ok(bytes) => bytes,
                    Err(_) => {
                        errors.push(error(&subfolder, format!("sky \"{name}\" has no {wanted}")));
                        continue;
                    }
                };
                let problem = match wanted {
                    _ if bytes.len() > MAX_SKY_FILE_BYTES => {
                        Some(format!("sky files must be at most {} MB", MAX_SKY_FILE_BYTES >> 20))
                    }
                    "sky.jpg" if !bytes.starts_with(&JPEG_SIGNATURE) => Some("this isn't a JPEG picture".to_string()),
                    "light.bin" => SkyLight::from_bytes(&bytes).err(),
                    "sky.ron" => parse_sky(&String::from_utf8_lossy(&bytes)).err(),
                    _ => None,
                };
                if let Some(problem) = problem {
                    errors.push(error(&relative, problem));
                }
                client_files.push((to_slashes(&relative), bytes));
            }
            pack.skies.insert(name);
        }
        let cycle_path = Path::new(SKY_CYCLE_FILE);
        if let Ok(bytes) = std::fs::read(folder.join(cycle_path)) {
            match parse_sky_cycle(&String::from_utf8_lossy(&bytes)) {
                Ok(cycle) => {
                    for keyframe in cycle.keyframes.iter().filter(|keyframe| !pack.skies.contains(&keyframe.sky)) {
                        errors.push(error(cycle_path, format!("there's no sky called \"{}\"", keyframe.sky)));
                    }
                }
                Err(problem) => errors.push(error(cycle_path, problem)),
            }
            client_files.push((SKY_CYCLE_FILE.to_string(), bytes));
        }

        if let Ok(text) = std::fs::read_to_string(folder.join(CHARACTERS_FILE)) {
            let file = Path::new(CHARACTERS_FILE);
            match parse_character_models(&text) {
                Ok(models) => {
                    errors.extend(check_character_models(&models, &model_files).into_iter().map(|m| error(file, m)));
                    pack.character_models = Some(models);
                    client_files.push((CHARACTERS_FILE.to_string(), text.into_bytes()));
                }
                Err(message) => errors.push(error(file, message)),
            }
        }
        check_model_names(&pack, &mut errors);

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
    match item.on_use {
        ItemEffect::Nothing => {}
        _ if item.equip.is_some() => problem("it can't be both worn and used".to_string()),
        ItemEffect::Heal(Healing::Flat(0)) => problem("it must heal at least 1".to_string()),
        ItemEffect::Heal(Healing::Percent(percent)) if !(percent > 0.0 && percent <= 100.0) => {
            problem("it must heal more than 0% and at most 100%".to_string());
        }
        ItemEffect::Heal(_) => {}
    }
    if let Some(equip) = &item.equip {
        if equip.slots.is_empty() {
            problem("it must fill at least one equipment slot".to_string());
        }
        let wielded = equip.slots.contains(&EquipSlot::DominantHand);
        match &equip.weapon {
            Some(weapon) if !wielded => {
                problem("only items in the DominantHand slot can have weapon stats".to_string());
            }
            Some(weapon) if !(weapon.speed > 0.0 && weapon.speed.is_finite()) => {
                problem("its weapon speed must be more than 0".to_string());
            }
            None if wielded => problem("items in the DominantHand slot need weapon stats".to_string()),
            _ => {}
        }
    }
}

fn check_ability(
    ability: &AbilityDef,
    icons: &HashSet<String>,
    sounds: &HashSet<String>,
    file: &Path,
    errors: &mut Vec<ContentError>,
) {
    let mut problem = |message: String| errors.push(error(file, format!("ability \"{}\": {message}", ability.id)));
    if ability.name.trim().is_empty() {
        problem("it needs a name".to_string());
    }
    if !icons.contains(&ability.icon) {
        problem(format!("there's no icon called \"{}\" in icons/", ability.icon));
    }
    if !(ability.cooldown >= 0.0 && ability.cooldown.is_finite() && ability.range >= 0.0 && ability.range.is_finite()) {
        problem("its cooldown and range must be 0 or more".to_string());
    }
    if let Some(sound) = ability.sound.as_ref().filter(|sound| !sounds.contains(*sound)) {
        problem(format!("there's no sound called \"{sound}\" in sounds/"));
    }
    if let AbilityEffect::Bolt { speed, .. } = ability.effect
        && !(speed > 0.0 && speed.is_finite())
    {
        problem("its bolt's speed must be more than 0".to_string());
    }
    if ability.needs_target() && ability.reach() <= 0.0 {
        problem("it's used on a target, so it needs a range".to_string());
    }
    let (r, g, b) = ability.visual.colour;
    if ![r, g, b, ability.visual.size].iter().all(|value| value.is_finite() && *value >= 0.0) {
        problem("its visual's colour and size must be 0 or more".to_string());
    }
}

fn check_skill(skill: &SkillDef, icons: &HashSet<String>, file: &Path, errors: &mut Vec<ContentError>) {
    let mut problem = |message: String| errors.push(error(file, format!("skill \"{}\": {message}", skill.id)));
    if skill.name.trim().is_empty() {
        problem("it needs a name".to_string());
    }
    if !icons.contains(&skill.icon) {
        problem(format!("there's no icon called \"{}\" in icons/", skill.icon));
    }
}

fn check_theme(theme: &ThemeDef, files: &HashSet<String>, file: &Path, errors: &mut Vec<ContentError>) {
    let mut problem = |message: String| errors.push(error(file, message));
    for (name, piece) in &theme.pieces {
        if !THEME_PIECES.iter().any(|(known, _)| known == name) {
            let known: Vec<_> = THEME_PIECES.iter().map(|(known, _)| *known).collect();
            problem(format!("there's no piece called \"{name}\" (the pieces are {})", known.join(", ")));
        }
        if !files.contains(&piece.image) {
            problem(format!("piece \"{name}\": there's no image called \"{}\" in ui/", piece.image));
        }
        if !piece.border.sides().iter().all(|side| side.is_finite() && *side >= 0.0) {
            problem(format!("piece \"{name}\": its border must be 0 or more"));
        }
    }
    for (tab, icon) in &theme.tab_icons {
        if !files.contains(icon) {
            problem(format!("tab icon \"{tab}\": there's no image called \"{icon}\" in ui/"));
        }
    }
    if let Some(font) = theme.font.as_ref().filter(|font| !files.contains(*font)) {
        problem(format!("there's no font called \"{font}\" in ui/"));
    }
}

/// Items' and appearances' outfit parts, hairstyles and equipment must exist.
fn check_model_names(pack: &ContentPack, errors: &mut Vec<ContentError>) {
    let models = pack.character_models.clone().unwrap_or_default();
    let has_part = |part: &String| models.parts.contains_key(part);
    for item in pack.items.values() {
        let parts = item.equip.iter().flat_map(|equip| &equip.parts);
        for part in parts.filter(|part| !has_part(part)) {
            let message = format!("item \"{}\": there's no outfit part called \"{part}\"", item.id);
            errors.push(error(Path::new("items"), message));
        }
    }
    for monster in pack.monsters.values() {
        if let Some(model) = monster.look.model.as_ref().filter(|model| !models.monsters.contains_key(*model)) {
            let message = format!("monster \"{}\": there's no monster model called \"{model}\"", monster.id);
            errors.push(error(Path::new("monsters"), message));
        }
    }
    for appearance in pack.appearances.values() {
        for part in appearance.parts.iter().filter(|part| !has_part(part)) {
            let message = format!("appearance \"{}\": there's no outfit part called \"{part}\"", appearance.id);
            errors.push(error(Path::new("appearances"), message));
        }
        for item in &appearance.equipment {
            let message = match pack.items.get(item) {
                None => format!("appearance \"{}\": there's no item called \"{item}\"", appearance.id),
                Some(def) if def.equip.is_none() => {
                    format!("appearance \"{}\": \"{item}\" isn't something that can be worn", appearance.id)
                }
                Some(_) => continue,
            };
            errors.push(error(Path::new("appearances"), message));
        }
        let hairs = appearance.hairstyle.iter().chain(&appearance.beard);
        for hair in hairs.filter(|hair| !models.hair.contains_key(*hair)) {
            let message = format!("appearance \"{}\": there's no hair called \"{hair}\"", appearance.id);
            errors.push(error(Path::new("appearances"), message));
        }
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

fn check_class(class: &ClassDef, pack: &ContentPack, file: &Path, errors: &mut Vec<ContentError>) {
    let mut problem = |message: String| errors.push(error(file, format!("class \"{}\": {message}", class.id)));
    if class.name.trim().is_empty() {
        problem("it needs a name".to_string());
    }
    if !(1..=MAX_HIT_DIE).contains(&class.rules.hit_die) {
        problem(format!("its hit_die must be from 1 to {MAX_HIT_DIE}"));
    }
    if !class.stats.is_valid() {
        problem("its stats must each be from 1 to 99".to_string());
    }
    let mut filled = HashSet::new();
    for item in &class.starting_equipment {
        match pack.items.get(item).and_then(|def| def.equip.as_ref()) {
            None => problem(format!("its starting equipment \"{item}\" isn't an item that can be equipped")),
            Some(equip) => {
                for slot in &equip.slots {
                    if !filled.insert(*slot) {
                        problem(format!("two of its starting items go in the {} slot", slot.name()));
                    }
                }
            }
        }
    }
    let class_abilities = class.abilities.iter().map(|offer| &offer.ability);
    for ability in class.starting_abilities.iter().chain(class_abilities) {
        if !pack.abilities.contains_key(ability) {
            problem(format!("there's no ability called \"{ability}\""));
        }
    }
    for offer in &class.abilities {
        if !(1..=MAX_LEVEL).contains(&offer.level) {
            problem(format!("{} must unlock at a level from 1 to {MAX_LEVEL}", offer.ability));
        }
    }
}

fn check_monster(monster: &MonsterDef, file: &Path, errors: &mut Vec<ContentError>) {
    let mut problem = |message: String| errors.push(error(file, format!("monster \"{}\": {message}", monster.id)));
    if monster.name.trim().is_empty() {
        problem("it needs a name".to_string());
    }
    if monster.health == 0 {
        problem("its health must be at least 1".to_string());
    }
    if !(1..=MAX_LEVEL).contains(&monster.level) {
        problem(format!("its level must be from 1 to {MAX_LEVEL}"));
    }
    if !(monster.attack.speed > 0.0 && monster.attack.speed.is_finite()) {
        problem("its attack speed must be more than 0".to_string());
    }
    let radii = [monster.aggro_radius, monster.wander_radius];
    if !radii.iter().all(|radius| (0.0..=MAX_WANDER_RADIUS).contains(radius)) {
        problem(format!("its aggro_radius and wander_radius must be from 0 to {MAX_WANDER_RADIUS} metres"));
    }
}

fn check_spawner(spawner: &SpawnerDef, pack: &ContentPack, file: &Path, errors: &mut Vec<ContentError>) {
    let mut problem = |message: String| errors.push(error(file, format!("spawner \"{}\": {message}", spawner.id)));
    if !pack.monsters.contains_key(&spawner.monster) {
        problem(format!("there's no monster called \"{}\"", spawner.monster));
    }
    if !(0.0..=MAX_WANDER_RADIUS).contains(&spawner.radius) {
        problem(format!("its radius must be from 0 to {MAX_WANDER_RADIUS} metres"));
    }
    if !(spawner.respawn_seconds > 0.0 && spawner.respawn_seconds.is_finite()) {
        problem("respawn_seconds must be more than 0".to_string());
    }
    if !spawner.at.0.is_finite() || !spawner.at.1.is_finite() {
        problem("its position must be a real number".to_string());
    }
}

fn check_quest(quest: &QuestDef, file: &Path, errors: &mut Vec<ContentError>) {
    let mut problem = |message: String| errors.push(error(file, format!("quest \"{}\": {message}", quest.id)));
    if quest.name.trim().is_empty() {
        problem("it needs a name".to_string());
    }
    if quest.stages.is_empty() {
        problem("it needs at least one stage".to_string());
    }
    let mut stages = HashSet::new();
    for stage in &quest.stages {
        if !stages.insert(stage.id.as_str()) {
            problem(format!("there's more than one stage called \"{}\"", stage.id));
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
        DialogueCommand::GiveMiasma { .. } | DialogueCommand::TakeMiasma { .. } => {}
        DialogueCommand::SetQuest { quest, stage } => match pack.quests.get(&quest) {
            None => return Err(format!("<<{text}>>: there's no quest called \"{quest}\"")),
            Some(def) if def.stage_index(&stage).is_none() => {
                return Err(format!("<<{text}>>: quest \"{quest}\" has no stage called \"{stage}\""));
            }
            Some(_) => {}
        },
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
    let (x, y) = npc.at;
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
        let npcs = r#"[(id: "tom", name: "Old Tom", appearance: "villager", at: (3.0, 4.0), wander_radius: 5.0)]"#;
        let folder = TestPack::new("good", &[("appearances/people.ron", APPEARANCE), ("npcs/town.ron", npcs)]);
        let pack = ContentPack::load(&folder.0).unwrap();
        assert_eq!(pack.npcs.len(), 1);
        assert_eq!(pack.client.manifest.files.len(), 1, "only appearances go to clients");
    }

    #[test]
    fn themes_name_known_pieces_and_pictures_that_exist() {
        let theme = r#"(pieces: {"window": (image: "window.png", border: 12), "windwo": (image: "window.png"),
            "button": (image: "missing.png", border: (4, 4, 3, 3))}, tab_icons: {"bag": "bag.png"})"#;
        let folder = TestPack::new("theme", &[("ui/theme.ron", theme), ("ui/window.png", "not a picture")]);
        let messages: Vec<String> = ContentPack::load(&folder.0).unwrap_err().iter().map(ToString::to_string).collect();
        assert_eq!(messages.len(), 4, "{messages:?}");
        assert_eq!(messages[0], "ui/window.png: this isn't a PNG image");
        assert!(messages[1].contains("piece \"button\": there's no image called \"missing.png\""), "{messages:?}");
        assert!(messages[2].contains("there's no piece called \"windwo\""), "{messages:?}");
        assert!(messages[3].contains("tab icon \"bag\": there's no image called \"bag.png\""), "{messages:?}");
    }

    #[test]
    fn items_need_icons() {
        let items = r#"[(id: "bread", name: "Bread", description: "A loaf.", icon: "bread.png", value: 4)]"#;
        let folder = TestPack::new("items", &[("items/food.ron", items), ("icons/loaf.png", "not a picture")]);
        let messages: Vec<String> = ContentPack::load(&folder.0).unwrap_err().iter().map(ToString::to_string).collect();
        assert_eq!(
            messages,
            [
                "icons/loaf.png: this isn't a PNG image",
                "items/food.ron: item \"bread\": there's no icon called \"bread.png\" in icons/",
            ]
        );
    }

    #[test]
    fn every_problem_is_reported_with_its_file() {
        let npcs = r#"[
            (id: "tom", name: "", appearance: "nobody", at: (1.0, 1.0), script: Some("missing.rhai")),
            (id: "tom", name: "Tom again", appearance: "villager", at: (1.0, 1.0)),
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
        let npcs = r#"[(id: "tom", name: "Tom", appearance: "villager", at: (1.0, 1.0), dialogue: Some("Tom"))]"#;
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
    fn some_class_must_be_offered_at_the_start() {
        let class = |id: &str, starting: bool| {
            format!(
                "(id: \"{id}\", name: \"{id}\", description: \"\", starting: {starting}, \
                 rules: (hit_die: 8, mana_per_level: 1, casting_stat: Wisdom), \
                 stats: (strength: 10, dexterity: 10, constitution: 10, intelligence: 10, wisdom: 10))"
            )
        };
        let hidden = format!("[{}]", class("knight", false));
        let folder = TestPack::new("no-starting-class", &[("classes/all.ron", &hidden)]);
        let errors = ContentPack::load(&folder.0).unwrap_err();
        assert!(errors.iter().any(|error| error.message.contains("no class is offered")), "{errors:?}");
        let both = format!("[{}, {}]", class("squire", true), class("knight", false));
        let folder = TestPack::new("one-starting-class", &[("classes/all.ron", &both)]);
        let pack = ContentPack::load(&folder.0).unwrap();
        assert_eq!(pack.classes.iter().filter(|class| class.starting).count(), 1);
    }

    #[test]
    fn textures_need_all_three_pictures_as_jpegs() {
        let folder = TestPack::new(
            "textures",
            &[("textures/moss/colour.jpg", "not a picture"), ("textures/moss/normal.jpg", "nor this")],
        );
        let errors = ContentPack::load(&folder.0).unwrap_err();
        let messages: Vec<&str> = errors.iter().map(|error| error.message.as_str()).collect();
        assert!(messages.contains(&"texture \"moss\" has no surface.jpg"), "{messages:?}");
        assert!(messages.iter().filter(|message| **message == "this isn't a JPEG picture").count() == 2);
    }

    #[test]
    fn syntax_errors_name_the_file() {
        let folder = TestPack::new("syntax", &[("appearances/broken.ron", "[(id: \"x\", skin: ")]);
        let errors = ContentPack::load(&folder.0).unwrap_err();
        assert_eq!(errors[0].file, Path::new("appearances/broken.ron"));
    }
}
