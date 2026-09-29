//! The server's settings, read from a RON file (`server.ron` by default).
//!
//! Every setting has a default, so the file only needs the ones you want to
//! change, and a missing file means "all defaults". Unknown settings are an
//! error, so a typo is reported rather than silently ignored.

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use venture_simulation::{DayCycle, MovementRules, Rules};

/// All the server's settings.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerConfig {
    /// Where the server listens.
    pub network: NetworkConfig,
    /// Where the server's files are.
    pub paths: PathsConfig,
    /// Rules for players.
    pub player: PlayerConfig,
    /// Rules for parties.
    pub party: PartyConfig,
    /// Rules for fishing.
    pub fishing: FishingConfig,
    /// Rules for woodcutting (felling trees and cutting them up).
    pub woodcutting: WoodcuttingConfig,
    /// How the days go: the length of a day, sunrise and sunset, and how
    /// high the sun gets (see `venture_simulation::DayCycle`).
    pub day: DayCycle,
    /// How characters move: gravity, jumps and slopes (see
    /// `venture_simulation::MovementRules`).
    pub movement: MovementRules,
    /// The names of characters with admin powers (ignoring case). Names are
    /// unique, and each belongs to one login token.
    pub admins: Vec<String>,
}

/// Where the server listens.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NetworkConfig {
    /// The port for the game, the web client and the world files.
    pub port: u16,
    /// The address to listen on. `0.0.0.0` accepts connections from the local network.
    pub bind: IpAddr,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self { port: 8080, bind: IpAddr::V4(Ipv4Addr::UNSPECIFIED) }
    }
}

/// Where the server's files are.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PathsConfig {
    /// The baked world folder.
    pub world: PathBuf,
    /// Where player profiles are saved.
    pub profiles: PathBuf,
    /// The built web client.
    pub client: PathBuf,
    /// The content pack (NPCs, appearances, scripts).
    pub content: PathBuf,
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            world: "world".into(),
            profiles: "server_data/profiles".into(),
            client: "crates/client/dist".into(),
            content: "content".into(),
        }
    }
}

/// Rules for players.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlayerConfig {
    /// The furthest (in a straight line, in metres) one click can send a
    /// character, and the furthest a character keeps following someone.
    pub max_move_distance: f32,
    /// How long (in seconds) a player waits after using an item (eating
    /// bread, say) before they can use another.
    pub item_cooldown: f32,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self { max_move_distance: Rules::default().max_move_distance, item_cooldown: 1.0 }
    }
}

/// Rules for parties (groups of players; see `crate::parties`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PartyConfig {
    /// The most players a party can have.
    pub max_members: u32,
    /// How long (in seconds) an invited player has to accept or decline.
    pub invite_seconds: f32,
    /// How near (in metres) a member must be to a monster when a party
    /// member defeats it to share its experience.
    pub share_distance: f32,
}

impl Default for PartyConfig {
    fn default() -> Self {
        Self { max_members: 8, invite_seconds: 15.0, share_distance: 60.0 }
    }
}

/// Rules for fishing (see `crate::fishing`; the rest are in `venture_rpg::fishing`
/// and the content).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FishingConfig {
    /// How much later than the strike window (in seconds) a strike still
    /// counts, allowing for the time it takes to reach the server.
    pub strike_allowance: f32,
    /// How long (in seconds) a line waits when nothing here takes the bait,
    /// before the player is told so.
    pub patience: f32,
    /// How much likelier a fish is to bite a line cast where it's shoaling.
    pub shoal_bonus: f32,
    /// Strike windows, bite waits, how fish of each size fight, and traps
    /// (see `venture_rpg::fishing::Tuning`).
    pub tuning: venture_rpg::fishing::Tuning,
    /// How the fight feels (see `venture_simulation::fishing::FightTuning`).
    pub fight: venture_simulation::fishing::FightTuning,
}

impl Default for FishingConfig {
    fn default() -> Self {
        Self {
            strike_allowance: 0.3,
            patience: 20.0,
            shoal_bonus: 3.0,
            tuning: Default::default(),
            fight: Default::default(),
        }
    }
}

/// Rules for woodcutting (see `crate::woodcutting`; the rest are in
/// `venture_rpg::woodcutting` and the content's `trees/woodcutting.ron`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WoodcuttingConfig {
    /// Seconds between swings of an axe.
    pub swing_seconds: f32,
    /// How far from a tree's trunk (or a fallen tree's) a player can chop it, in metres.
    pub reach: f32,
    /// How long a fallen tree lies before it goes, in seconds.
    pub fallen_seconds: f32,
    /// How long after its fallen trunk goes a tree grows back, in seconds.
    pub regrow_seconds: f32,
}

impl Default for WoodcuttingConfig {
    fn default() -> Self {
        Self { swing_seconds: 1.6, reach: 3.0, fallen_seconds: 180.0, regrow_seconds: 60.0 }
    }
}

impl WoodcuttingConfig {
    /// What's wrong with these settings, if anything.
    pub fn problems(&self) -> Option<String> {
        let numbers = [self.swing_seconds, self.reach, self.fallen_seconds, self.regrow_seconds];
        numbers
            .iter()
            .any(|number| !(number.is_finite() && *number >= 0.0))
            .then(|| "woodcutting's times and reach must be numbers, none negative".to_string())
            .or_else(|| {
                (self.swing_seconds < 0.2).then(|| "woodcutting.swing_seconds must be at least 0.2".to_string())
            })
    }
}

impl ServerConfig {
    /// Reads the settings from a RON file. A missing file gives the defaults.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => return Err(format!("can't read {}: {error}", path.display())),
        };
        let config: Self = ron::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?;
        config.validate().map_err(|problem| format!("{}: {problem}", path.display()))?;
        Ok(config)
    }

    /// The game rules these settings describe, as shared with clients.
    pub fn rules(&self) -> Rules {
        Rules { max_move_distance: self.player.max_move_distance, day: self.day, movement: self.movement }
    }

    fn validate(&self) -> Result<(), String> {
        let distance = self.player.max_move_distance;
        if !distance.is_finite() || distance <= 0.0 {
            return Err(format!("player.max_move_distance must be a positive number of metres, not {distance}"));
        }
        let cooldown = self.player.item_cooldown;
        if !cooldown.is_finite() || cooldown < 0.0 {
            return Err(format!("player.item_cooldown must be a number of seconds, not {cooldown}"));
        }
        let movement = self.movement;
        let lengths = [movement.gravity, movement.jump_height, movement.step_height];
        if lengths.iter().any(|length| !length.is_finite() || *length < 0.0) {
            return Err("movement.gravity, jump_height and step_height must be numbers, at least 0".to_string());
        }
        if !(0.0..90.0).contains(&movement.max_climb_degrees) {
            return Err(format!("movement.max_climb_degrees must be 0 to 90, not {}", movement.max_climb_degrees));
        }
        let day = self.day;
        if !day.day_minutes.is_finite() || day.day_minutes < 1.0 {
            return Err(format!("day.day_minutes must be at least 1, not {}", day.day_minutes));
        }
        if !(0.0..24.0).contains(&day.sunrise) || !(day.sunrise + 1.0..=day.sunrise + 23.0).contains(&day.sunset) {
            return Err("day.sunrise must be an hour from 0 to 24, and day.sunset 1 to 23 hours after it".to_string());
        }
        if !(5.0..=89.0).contains(&day.noon_height) {
            return Err(format!("day.noon_height must be from 5 to 89 degrees, not {}", day.noon_height));
        }
        if self.party.max_members < 2 {
            return Err(format!("party.max_members must be at least 2, not {}", self.party.max_members));
        }
        let share = self.party.share_distance;
        if !share.is_finite() || share < 0.0 {
            return Err(format!("party.share_distance must be a number of metres, not {share}"));
        }
        let allowance = self.fishing.strike_allowance;
        if !(0.0..=2.0).contains(&allowance) {
            return Err(format!("fishing.strike_allowance must be from 0 to 2 seconds, not {allowance}"));
        }
        let patience = self.fishing.patience;
        if !patience.is_finite() || patience <= 0.0 {
            return Err(format!("fishing.patience must be a positive number of seconds, not {patience}"));
        }
        let bonus = self.fishing.shoal_bonus;
        if !bonus.is_finite() || bonus < 1.0 {
            return Err(format!("fishing.shoal_bonus must be at least 1, not {bonus}"));
        }
        if let Some(problem) = self.woodcutting.problems() {
            return Err(problem);
        }
        if let Some(problem) = self.fishing.tuning.problems().or_else(|| self.fishing.fight.problems()) {
            return Err(format!("fishing: {problem}"));
        }
        let seconds = self.party.invite_seconds;
        if !seconds.is_finite() || seconds <= 0.0 {
            return Err(format!("party.invite_seconds must be a positive number of seconds, not {seconds}"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_settings_take_their_defaults() {
        let config: ServerConfig = ron::from_str("(player: (max_move_distance: 80.0))").unwrap();
        assert_eq!(config.player.max_move_distance, 80.0);
        assert_eq!(config.network, NetworkConfig::default());
    }

    #[test]
    fn typos_are_errors() {
        assert!(ron::from_str::<ServerConfig>("(player: (max_move_distanse: 80.0))").is_err());
    }

    #[test]
    fn the_example_settings_load_and_are_the_defaults_for_fishing() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../server.ron");
        let config = ServerConfig::load(&path).unwrap();
        assert_eq!(config.fishing, FishingConfig::default(), "server.ron lists fishing's defaults");
        assert_eq!(config.woodcutting, WoodcuttingConfig::default(), "server.ron lists woodcutting's defaults");
        let mut tuned = config.clone();
        tuned.fishing.tuning.bycatch_share = 2.0;
        assert!(tuned.validate().unwrap_err().contains("bycatch_share"));
    }

    #[test]
    fn nonsense_values_are_rejected() {
        let config = ServerConfig {
            player: PlayerConfig { max_move_distance: -5.0, ..Default::default() },
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }
}
