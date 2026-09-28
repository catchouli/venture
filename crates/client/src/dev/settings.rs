//! Development options read from environment variables, for checking the game
//! without a person at the keyboard (see `docs/building-and-running.md`).
//!
//! - `VENTURE_CAMERA=x,y,yaw,pitch,distance`: start new characters at `(x, y)`
//!   metres (in `--local` mode), with the camera at that rotation, tilt
//!   (radians) and distance (metres).
//! - `VENTURE_MOVE_TO=x,y`: once loaded, run to `(x, y)` as if it were clicked.
//! - `VENTURE_SCREENSHOT=path`: see [`crate::dev::screenshot`].
//! - `VENTURE_PROFILE=name`: use a separate login token (so a separate
//!   character), for running several clients on one computer.
//! - `VENTURE_AUTOMATED=1`: nobody is at the keyboard, so take the suggested
//!   name without asking (implied by `VENTURE_SCREENSHOT`).
//! - `VENTURE_CLASS=mage`: the class automated runs make new characters as
//!   (the first class if not set).
//! - `VENTURE_NAME=name`: the name automated runs make new characters with
//!   (the server's suggestion if not set).
//! - `VENTURE_BODY=feminine`: the body type automated runs make new
//!   characters with (androgynous if not set).
//! - `VENTURE_LOOK=height=5,build=-3,head=2,hair=long,beard=beard,hair_colour=3,skin=5`:
//!   the features automated runs make new characters with (any left out are
//!   the usual), and the character creation screen starts with.
//! - `VENTURE_SHOW_CREATION=1`: show the character creation screen even in an
//!   automated run (and take the screenshot there).
//! - `VENTURE_SAY=text`: say this in chat once in the game (several messages
//!   can be separated with `|`).
//! - `VENTURE_SAY_DELAY=seconds`: wait this long in the game before saying
//!   them (to answer something another client does first).
//! - `VENTURE_MAP_ZOOM=3.0`: how zoomed in the map (`VENTURE_OPEN=map`)
//!   opens, in metres a pixel.
//! - `VENTURE_USE=air_blast`: use these abilities (ids, separated with `|`) once
//!   in the game, on the selected target (after `VENTURE_SELECT_NEAREST` has
//!   picked one, if set).
//! - `VENTURE_OPEN=bag,journal`: open these windows once in the game.
//! - `VENTURE_CHOOSE=1,2`: pick these replies (numbered from 1), one per
//!   dialogue page, as conversations offer them.
//! - `VENTURE_SELECT_NEAREST=1`: select the character (player or NPC) nearest yours once one is in sight.
//! - `VENTURE_FISH=x,y`: once loaded, take the first fishing rod in the bag
//!   in hand (if none is), and cast at `(x, y)`.
//! - `VENTURE_FISH_AUTO=1`: and strike at the bite, and reel while the fish
//!   rests (to see a fight).
//! - `VENTURE_TRAP=1`: once loaded, set the first fishing trap in the bag in
//!   the nearest water.
//!
//! Environment variables don't exist on the web, so there these are all unset.

use bevy::prelude::*;
use venture_navigation::Point;

/// Development options.
#[derive(Resource, Default, Debug)]
pub struct DevSettings {
    /// Where new characters start, instead of the world's spawn point (`--local` mode only).
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub start_position: Option<Point>,
    /// The camera's starting `(yaw, pitch, distance)`.
    pub camera_view: Option<(f32, f32, f32)>,
    /// Where the player runs once the world has loaded.
    pub move_to: Option<Point>,
    /// Which login token to use, if not the usual one.
    pub profile: Option<String>,
    /// Whether nobody is at the keyboard (a screenshot run), so prompts should
    /// take their defaults.
    pub automated: bool,
    /// Messages to say in chat once in the game.
    pub say: Vec<String>,
    /// How long to wait (in seconds, once in the game) before saying them.
    pub say_delay: f32,
    /// How zoomed in the map opens (metres a pixel), if not the usual.
    pub map_zoom: Option<f32>,
    /// Abilities to use once in the game.
    pub use_abilities: Vec<String>,
    /// Whether to select the character (player or NPC) nearest yours once one is in sight.
    pub select_nearest: bool,
    /// Windows to open once in the game (`bag`, `journal`).
    pub open: Vec<String>,
    /// Dialogue replies to pick, in order (numbered from 1).
    pub choose: Vec<u32>,
    /// The class automated runs make new characters as.
    pub class: Option<String>,
    /// The name automated runs make new characters with.
    pub name: Option<String>,
    /// The body type automated runs make new characters with.
    pub body: Option<venture_protocol::BodyType>,
    /// The features automated runs make new characters with.
    pub features: venture_protocol::Features,
    /// Whether to show character creation even in an automated run.
    pub show_creation: bool,
    /// Where to cast a fishing line once loaded.
    pub fish_at: Option<Point>,
    /// Whether to play the fish once it bites.
    pub fish_auto: bool,
    /// Whether to set a trap once loaded.
    pub trap: bool,
}

impl DevSettings {
    /// Reads the settings from the environment.
    pub fn from_environment() -> Self {
        let camera = numbers("VENTURE_CAMERA").filter(|n| n.len() == 5);
        let move_to = numbers("VENTURE_MOVE_TO").filter(|n| n.len() == 2);
        Self {
            start_position: camera.as_ref().map(|n| (n[0], n[1])),
            camera_view: camera.as_ref().map(|n| (n[2], n[3], n[4])),
            move_to: move_to.map(|n| (n[0], n[1])),
            profile: std::env::var("VENTURE_PROFILE").ok().filter(|name| name.chars().all(char::is_alphanumeric)),
            automated: std::env::var_os("VENTURE_SCREENSHOT").is_some()
                || std::env::var_os("VENTURE_AUTOMATED").is_some(),
            say: std::env::var("VENTURE_SAY")
                .map(|say| say.split('|').map(str::to_string).collect())
                .unwrap_or_default(),
            say_delay: std::env::var("VENTURE_SAY_DELAY").ok().and_then(|delay| delay.parse().ok()).unwrap_or(0.0),
            map_zoom: std::env::var("VENTURE_MAP_ZOOM").ok().and_then(|zoom| zoom.parse().ok()),
            use_abilities: std::env::var("VENTURE_USE")
                .map(|abilities| abilities.split('|').map(str::to_string).collect())
                .unwrap_or_default(),
            select_nearest: std::env::var_os("VENTURE_SELECT_NEAREST").is_some(),
            open: std::env::var("VENTURE_OPEN")
                .map(|open| open.split(',').map(|name| name.trim().to_string()).collect())
                .unwrap_or_default(),
            class: std::env::var("VENTURE_CLASS").ok(),
            name: std::env::var("VENTURE_NAME").ok(),
            body: std::env::var("VENTURE_BODY").ok().and_then(|wanted| {
                venture_protocol::BodyType::ALL.into_iter().find(|body| body.name().eq_ignore_ascii_case(&wanted))
            }),
            features: std::env::var("VENTURE_LOOK").map(|look| features(&look)).unwrap_or_default(),
            show_creation: std::env::var_os("VENTURE_SHOW_CREATION").is_some(),
            choose: numbers("VENTURE_CHOOSE").unwrap_or_default().into_iter().map(|n| n as u32).collect(),
            fish_at: numbers("VENTURE_FISH").filter(|n| n.len() == 2).map(|n| (n[0], n[1])),
            fish_auto: std::env::var_os("VENTURE_FISH_AUTO").is_some(),
            trap: std::env::var_os("VENTURE_TRAP").is_some(),
        }
    }
}

/// Features from `name=value` pairs, separated with commas.
fn features(text: &str) -> venture_protocol::Features {
    let mut features = venture_protocol::Features::default();
    for (name, value) in text.split(',').filter_map(|pair| pair.split_once('=')) {
        let (name, value) = (name.trim(), value.trim());
        let number = value.parse().unwrap_or(0);
        match name {
            "height" => features.height = number,
            "build" => features.build = number,
            "head" => features.head = number,
            "hair" => features.hair = Some(value.to_string()),
            "beard" => features.beard = Some(value.to_string()),
            "hair_colour" => features.hair_colour = number.max(0) as u8,
            "skin" => features.skin = number.max(0) as u8,
            _ => {}
        }
    }
    features.clamped()
}

/// A comma-separated list of numbers from an environment variable.
fn numbers(variable: &str) -> Option<Vec<f32>> {
    let value = std::env::var(variable).ok()?;
    value.split(',').map(|part| part.trim().parse().ok()).collect()
}
