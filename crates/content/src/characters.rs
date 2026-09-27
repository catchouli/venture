//! Character models: which 3D models make up a person (a body, hair, and
//! outfit parts on one shared skeleton) and the animations they play. Read
//! from `characters/characters.ron`, with the models (glTF) and their
//! textures beside it in `characters/`. See `docs/content-authoring.md`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use venture_protocol::{BodyType, Features};

/// Everything about character models.
///
/// ```ron
/// (
///     animations: "animations.glb",
///     clips: { "idle": "Idle_Loop", "run": "Jog_Fwd_Loop" },
///     bodies: (masculine: (model: "body_male.gltf", fits: Masculine, hair: "parted"), ...),
///     hair: { "long": "hair_long.gltf" },
///     parts: { "ranger_body": (masculine: "male_ranger_body.gltf", feminine: "female_ranger_body.gltf", covers: [Torso]) },
///     plain_clothes: ["peasant_body", "peasant_legs"],
///     outfits: { "Tunic": ["peasant_body", "peasant_arms", "peasant_legs", "peasant_feet"] },
///     hairstyles: ["parted", "long"],
///     beards: ["beard"],
///     hair_colours: [(0.32, 0.21, 0.12), (0.08, 0.06, 0.05)],
///     skin_tones: [(0.93, 0.80, 0.72), (0.48, 0.32, 0.22)],
/// )
/// ```
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct CharacterModels {
    /// The file holding the animations (on the shared skeleton).
    pub animations: String,
    /// The game's names for animations (such as `"run"`), and each one's name
    /// in the animations file. See [`CLIPS`] for the names the game uses.
    pub clips: BTreeMap<String, String>,
    /// The model for each body type.
    pub bodies: Bodies,
    /// Hairstyles: each one's model, by name.
    #[serde(default)]
    pub hair: BTreeMap<String, String>,
    /// Outfit parts (clothes and armour), by name. Items name the parts they
    /// put on the character.
    #[serde(default)]
    pub parts: BTreeMap<String, PartModel>,
    /// Parts worn where nothing else covers the body (so nobody walks about
    /// in their underclothes).
    #[serde(default)]
    pub plain_clothes: Vec<String>,
    /// The parts for each of the NPCs' outfit styles (`Tunic`, `Armour`, `Robe`).
    #[serde(default)]
    pub outfits: BTreeMap<String, Vec<String>>,
    /// The hairstyles players can choose (names from `hair`), in order.
    #[serde(default)]
    pub hairstyles: Vec<String>,
    /// The beards players can choose (names from `hair`), in order.
    #[serde(default)]
    pub beards: Vec<String>,
    /// The hair colours players can choose (sRGB, 0 to 1), in order.
    #[serde(default)]
    pub hair_colours: Vec<(f32, f32, f32)>,
    /// Creature models, by name: whole models on the characters' skeleton
    /// (so they share the animations), which creatures' looks can name.
    #[serde(default)]
    pub creatures: BTreeMap<String, CreatureModel>,
    /// The skin tones players can choose (sRGB, 0 to 1), in order. The skin
    /// textures are tinted to match (see [`BodyModel::skin_colour`]).
    #[serde(default)]
    pub skin_tones: Vec<(f32, f32, f32)>,
}

impl CharacterModels {
    /// A player's chosen features, with anything the content doesn't offer
    /// put back to the usual: unknown hairstyles and beards, and colours
    /// past the end of the palettes.
    pub fn allowed(&self, features: Features) -> Features {
        let Features { hair, beard, hair_colour, skin, .. } = &features;
        let hair = hair.clone().filter(|hair| self.hairstyles.contains(hair));
        let beard = beard.clone().filter(|beard| self.beards.contains(beard));
        let hair_colour = if (*hair_colour as usize) < self.hair_colours.len() { *hair_colour } else { 0 };
        let skin = if (*skin as usize) < self.skin_tones.len() { *skin } else { 0 };
        Features { hair, beard, hair_colour, skin, ..features.clamped() }
    }
}

/// The game's names for animations, with when each plays.
pub const CLIPS: [(&str, &str); 13] = [
    ("idle", "standing still"),
    ("walk", "moving slowly"),
    ("run", "running"),
    ("attack", "swinging a weapon"),
    ("punch", "attacking with no weapon"),
    ("cast", "casting a spell"),
    ("sit_down", "sitting down to rest"),
    ("sitting", "resting"),
    ("stand_up", "getting up from resting"),
    ("jump", "in the air"),
    ("wave", "waving"),
    ("hit", "being hit"),
    ("death", "dying"),
];

/// A creature's model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct CreatureModel {
    /// Its model (on the characters' skeleton).
    pub model: String,
    /// Its size (1 as modelled).
    #[serde(default = "one")]
    pub scale: f32,
}

/// The body model for each body type.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct Bodies {
    /// For masculine characters.
    pub masculine: BodyModel,
    /// For feminine characters.
    pub feminine: BodyModel,
    /// For androgynous characters.
    pub androgynous: BodyModel,
}

impl Bodies {
    /// The model for a body type.
    pub fn of(&self, body: BodyType) -> &BodyModel {
        match body {
            BodyType::Masculine => &self.masculine,
            BodyType::Feminine => &self.feminine,
            BodyType::Androgynous => &self.androgynous,
        }
    }
}

/// A body.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct BodyModel {
    /// Its model.
    pub model: String,
    /// Which outfit parts fit it: parts made for masculine or feminine bodies.
    pub fits: PartFit,
    /// The hairstyle it has unless one is chosen.
    pub hair: String,
    /// The colour of the skin in its texture (sRGB, 0 to 1), which is tinted
    /// to make other skin tones.
    #[serde(default = "texture_skin")]
    pub skin_colour: (f32, f32, f32),
}

/// Which bodies an outfit part was made for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum PartFit {
    /// Parts made for masculine bodies.
    #[default]
    Masculine,
    /// Parts made for feminine bodies.
    Feminine,
}

/// An outfit part: its model for each kind of body (either can be left out,
/// and the other is used), and the parts of the body it covers (hidden under
/// it, so they don't poke through).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct PartModel {
    /// Its model for masculine bodies.
    #[serde(default)]
    pub masculine: Option<String>,
    /// Its model for feminine bodies.
    #[serde(default)]
    pub feminine: Option<String>,
    /// The parts of the body it hides.
    #[serde(default)]
    pub covers: Vec<BodyRegion>,
    /// Whether it hides the hair (a hood or helmet).
    #[serde(default)]
    pub hides_hair: bool,
    /// For things held or strapped on (weapons, shields) rather than worn:
    /// the bone it's attached to, and how it sits there.
    #[serde(default)]
    pub held: Option<Held>,
}

/// How a held thing is attached to the skeleton.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct Held {
    /// The bone it's attached to (such as `"hand_r"`).
    #[cfg_attr(feature = "reflect", reflect(@crate::Refers::Bone))]
    pub bone: String,
    /// Where it sits, from the bone (metres, in the bone's own directions).
    #[serde(default)]
    pub position: (f32, f32, f32),
    /// How it's turned from the bone (degrees about X, then Y, then Z).
    #[serde(default)]
    pub rotation: (f32, f32, f32),
    /// Its size (1 as modelled).
    #[serde(default = "one")]
    pub scale: f32,
}

fn texture_skin() -> (f32, f32, f32) {
    (0.66, 0.47, 0.33)
}

fn one() -> f32 {
    1.0
}

impl PartModel {
    /// Its model for bodies it fits (or the other one, if it has only that).
    pub fn model_for(&self, fit: PartFit) -> Option<&String> {
        let (wanted, other) = match fit {
            PartFit::Masculine => (&self.masculine, &self.feminine),
            PartFit::Feminine => (&self.feminine, &self.masculine),
        };
        wanted.as_ref().or(other.as_ref())
    }
}

/// A part of the body clothes can cover.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum BodyRegion {
    /// Chest, back and belly.
    Torso,
    /// Upper and lower arms.
    Arms,
    /// Hands.
    Hands,
    /// Hips.
    Hips,
    /// Thighs and shins.
    Legs,
    /// Feet.
    Feet,
}

impl BodyRegion {
    /// The skeleton's bones in this part of the body. (A part of the body
    /// mesh is hidden when all its corners belong mostly to hidden bones.)
    pub fn bones(self) -> &'static [&'static str] {
        match self {
            BodyRegion::Torso => &["spine_01", "spine_02", "spine_03", "clavicle_l", "clavicle_r"],
            BodyRegion::Arms => &["upperarm_l", "upperarm_r", "lowerarm_l", "lowerarm_r"],
            BodyRegion::Hands => &[
                "hand_l",
                "hand_r",
                "index_01_l",
                "index_02_l",
                "index_03_l",
                "middle_01_l",
                "middle_02_l",
                "middle_03_l",
                "pinky_01_l",
                "pinky_02_l",
                "pinky_03_l",
                "ring_01_l",
                "ring_02_l",
                "ring_03_l",
                "thumb_01_l",
                "thumb_02_l",
                "thumb_03_l",
                "index_01_r",
                "index_02_r",
                "index_03_r",
                "middle_01_r",
                "middle_02_r",
                "middle_03_r",
                "pinky_01_r",
                "pinky_02_r",
                "pinky_03_r",
                "ring_01_r",
                "ring_02_r",
                "ring_03_r",
                "thumb_01_r",
                "thumb_02_r",
                "thumb_03_r",
            ],
            BodyRegion::Hips => &["pelvis"],
            BodyRegion::Legs => &["thigh_l", "thigh_r", "calf_l", "calf_r"],
            BodyRegion::Feet => &["foot_l", "foot_r", "ball_l", "ball_r"],
        }
    }
}

/// The bones things can be held by.
pub const BONES: [&str; 8] = ["hand_r", "hand_l", "lowerarm_r", "lowerarm_l", "Head", "spine_03", "pelvis", "root"];

/// Reads `characters/characters.ron`.
pub fn parse_character_models(text: &str) -> Result<CharacterModels, String> {
    ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .from_str(text)
        .map_err(|error| error.to_string())
}

/// Every problem with the character models: files that aren't in
/// `characters/`, and names that don't exist.
pub fn check_character_models(models: &CharacterModels, files: &[String]) -> Vec<String> {
    let mut missing = Vec::new();
    let mut file = |what: String, name: &str| {
        if !files.iter().any(|file| file == name) {
            missing.push(format!("{what}: there's no file called \"{name}\" in characters/"));
        }
    };
    file("animations".to_string(), &models.animations);
    for body in [BodyType::Masculine, BodyType::Feminine, BodyType::Androgynous] {
        let model = models.bodies.of(body);
        file(format!("{body:?} body"), &model.model);
    }
    for (name, hair) in &models.hair {
        file(format!("hair \"{name}\""), hair);
    }
    for (name, creature) in &models.creatures {
        file(format!("creature \"{name}\""), &creature.model);
    }
    let mut part_problems = Vec::new();
    for (name, part) in &models.parts {
        for model in [&part.masculine, &part.feminine].into_iter().flatten() {
            file(format!("part \"{name}\""), model);
        }
        if part.masculine.is_none() && part.feminine.is_none() {
            part_problems.push(format!("part \"{name}\": it needs a model"));
        }
        if let Some(held) = part.held.as_ref().filter(|held| !BONES.contains(&held.bone.as_str())) {
            part_problems.push(format!("part \"{name}\": the skeleton has no bone called \"{}\"", held.bone));
        }
    }
    let mut problems = missing;
    problems.extend(part_problems);
    for body in [BodyType::Masculine, BodyType::Feminine, BodyType::Androgynous] {
        let hair = &models.bodies.of(body).hair;
        if !models.hair.contains_key(hair) {
            problems.push(format!("{body:?} body: there's no hairstyle called \"{hair}\""));
        }
    }
    for hair in models.hairstyles.iter().chain(&models.beards).filter(|hair| !models.hair.contains_key(*hair)) {
        problems.push(format!("there's no hair called \"{hair}\""));
    }
    let parts = models.plain_clothes.iter().chain(models.outfits.values().flatten());
    for part in parts.filter(|part| !models.parts.contains_key(*part)) {
        problems.push(format!("there's no part called \"{part}\""));
    }
    for (name, _) in CLIPS.iter().filter(|(name, _)| !models.clips.contains_key(*name)) {
        problems.push(format!("clips: the game needs an animation for \"{name}\""));
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_offered_features_are_allowed() {
        let models = CharacterModels {
            hairstyles: vec!["long".to_string()],
            hair_colours: vec![(0.1, 0.1, 0.1); 2],
            skin_tones: vec![(0.5, 0.4, 0.3)],
            ..CharacterModels::default()
        };
        let chosen = Features {
            height: 100,
            build: -3,
            head: -100,
            hair: Some("long".to_string()),
            beard: Some("long".to_string()),
            hair_colour: 1,
            skin: 1,
        };
        let allowed = models.allowed(chosen);
        assert_eq!((allowed.height, allowed.build, allowed.head), (10, -3, -10));
        assert_eq!(allowed.hair.as_deref(), Some("long"));
        assert_eq!(allowed.beard, None);
        assert_eq!((allowed.hair_colour, allowed.skin), (1, 0));
    }

    #[test]
    fn parts_fall_back_to_the_other_body() {
        let only_masculine = PartModel { masculine: Some("m.gltf".to_string()), ..PartModel::default() };
        assert_eq!(only_masculine.model_for(PartFit::Feminine), Some(&"m.gltf".to_string()));
        let both = PartModel { feminine: Some("f.gltf".to_string()), ..only_masculine };
        assert_eq!(both.model_for(PartFit::Feminine), Some(&"f.gltf".to_string()));
    }
}
