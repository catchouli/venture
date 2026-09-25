//! The server's settings, read from a RON file (`server.ron` by default).
//!
//! Every setting has a default, so the file only needs the ones you want to
//! change, and a missing file means "all defaults". Unknown settings are an
//! error, so a typo is reported rather than silently ignored.

use std::net::{IpAddr, Ipv4Addr};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use venture_simulation::Rules;

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
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self { max_move_distance: Rules::default().max_move_distance }
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
        Rules { max_move_distance: self.player.max_move_distance }
    }

    fn validate(&self) -> Result<(), String> {
        let distance = self.player.max_move_distance;
        if !distance.is_finite() || distance <= 0.0 {
            return Err(format!("player.max_move_distance must be a positive number of metres, not {distance}"));
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
    fn nonsense_values_are_rejected() {
        let config = ServerConfig { player: PlayerConfig { max_move_distance: -5.0 }, ..Default::default() };
        assert!(config.validate().is_err());
    }
}
