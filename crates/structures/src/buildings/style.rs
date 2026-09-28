//! Building styles: everything about how a building is built that isn't
//! its plan. Styles are content (`buildings/styles.ron`), so a new one is
//! mostly data; how walls and roofs are put together (their
//! *construction*) is what the generator knows how to do.

use serde::{Deserialize, Serialize};

/// A building style.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub struct BuildingStyle {
    /// Its id (what plans name).
    pub id: String,
    /// Its name, for people.
    pub name: String,
    /// How its walls are built.
    pub walls: WallConstruction,
    /// How big it is: every size below (storeys, doors, windows, walls,
    /// roof, chimney) is multiplied by this (1 as written).
    #[serde(default = "defaults::scale")]
    pub scale: f32,
    /// How thick its outer walls are, in metres.
    #[serde(default = "defaults::wall_thickness")]
    pub wall_thickness: f32,
    /// How thick its inner walls are, in metres.
    #[serde(default = "defaults::inner_thickness")]
    pub inner_thickness: f32,
    /// How tall each floor is (floor to floor), in metres.
    #[serde(default = "defaults::storey")]
    pub storey: f32,
    /// Its roof.
    #[serde(default)]
    pub roof: RoofStyle,
    /// Its doors.
    #[serde(default)]
    pub door: DoorStyle,
    /// Its windows.
    #[serde(default)]
    pub window: WindowStyle,
    /// Its chimney.
    #[serde(default)]
    pub chimney: ChimneyStyle,
    /// Its colours (sRGB, multiplying the textures): each is a range the seed picks from.
    #[serde(default)]
    pub colours: StyleColours,
}

/// How walls are built.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum WallConstruction {
    /// Stones laid in courses (as a dry stone wall is), with quoins at the
    /// corners and stone lintels and sills.
    #[default]
    Stone,
    /// Overlapping horizontal boards (weatherboarding) with posts at the
    /// corners and wooden frames, on a stone plinth.
    Timber,
}

/// A roof.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(default)]
pub struct RoofStyle {
    /// How steep it is, in degrees: the least and most (the seed picks).
    pub pitch: (f32, f32),
    /// How far it reaches past the walls at the eaves, in metres.
    pub eaves: f32,
    /// How far it reaches past the gable walls, in metres.
    pub verge: f32,
    /// How thick it is, in metres (thatch is thick).
    pub thickness: f32,
}

impl Default for RoofStyle {
    fn default() -> Self {
        Self { pitch: (46.0, 54.0), eaves: 0.45, verge: 0.3, thickness: 0.35 }
    }
}

/// Doors.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(default)]
pub struct DoorStyle {
    /// How wide and tall a doorway is, in metres.
    pub size: (f32, f32),
}

impl Default for DoorStyle {
    fn default() -> Self {
        Self { size: (1.5, 2.35) }
    }
}

/// Windows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(default)]
pub struct WindowStyle {
    /// How wide and tall one is, in metres.
    pub size: (f32, f32),
    /// How high its sill is above the floor, in metres.
    pub sill: f32,
    /// The least room between windows (and a window and a door), in metres.
    pub gap: f32,
    /// How far from a corner the nearest can be, in metres.
    pub corner: f32,
    /// How likely each place a window could go gets one (0 to 1).
    pub chance: f32,
    /// Whether they have shutters.
    pub shutters: bool,
}

impl Default for WindowStyle {
    fn default() -> Self {
        Self { size: (0.85, 1.05), sill: 1.0, gap: 1.5, corner: 0.9, chance: 0.8, shutters: false }
    }
}

/// Chimneys.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(default)]
pub struct ChimneyStyle {
    /// How likely a building has one (0 to 1), unless its plan says.
    pub chance: f32,
    /// How wide (along the ridge) and deep it is, in metres.
    pub size: (f32, f32),
    /// How far it rises above the ridge, in metres.
    pub above_ridge: f32,
}

impl Default for ChimneyStyle {
    fn default() -> Self {
        Self { chance: 0.8, size: (0.9, 0.7), above_ridge: 0.9 }
    }
}

/// A style's colours (sRGB, 0 to 1), each a range from one to the other that
/// the seed picks along.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(default)]
pub struct StyleColours {
    /// The walls.
    pub walls: ((f32, f32, f32), (f32, f32, f32)),
    /// The roof.
    pub roof: ((f32, f32, f32), (f32, f32, f32)),
    /// Frames, beams and shutters.
    pub trim: ((f32, f32, f32), (f32, f32, f32)),
    /// Doors (and shutters): a few to pick from.
    pub doors: Vec<(f32, f32, f32)>,
    /// Inside the walls (limewash).
    pub plaster: (f32, f32, f32),
}

impl Default for StyleColours {
    fn default() -> Self {
        Self {
            walls: ((0.95, 0.93, 0.88), (0.8, 0.76, 0.7)),
            roof: ((0.95, 0.9, 0.8), (0.75, 0.68, 0.58)),
            trim: ((0.62, 0.52, 0.42), (0.45, 0.38, 0.32)),
            doors: vec![(0.35, 0.42, 0.3), (0.5, 0.35, 0.25), (0.3, 0.35, 0.45), (0.55, 0.5, 0.42)],
            plaster: (0.92, 0.9, 0.84),
        }
    }
}

impl BuildingStyle {
    /// The style with its sizes multiplied by its [`scale`](Self::scale)
    /// (what the generator and the bake use).
    pub fn scaled(&self) -> BuildingStyle {
        let k = if self.scale > 0.0 { self.scale } else { 1.0 };
        let mut style = self.clone();
        style.scale = 1.0;
        style.wall_thickness *= k;
        style.inner_thickness *= k;
        style.storey *= k;
        style.roof.eaves *= k;
        style.roof.verge *= k;
        style.roof.thickness *= k;
        style.door.size = (style.door.size.0 * k, style.door.size.1 * k);
        let window = &mut style.window;
        window.size = (window.size.0 * k, window.size.1 * k);
        window.sill *= k;
        window.gap *= k;
        window.corner *= k;
        style.chimney.size = (style.chimney.size.0 * k, style.chimney.size.1 * k);
        style.chimney.above_ridge *= k;
        style
    }
}

mod defaults {
    pub fn scale() -> f32 {
        1.0
    }
    pub fn wall_thickness() -> f32 {
        0.5
    }
    pub fn inner_thickness() -> f32 {
        0.2
    }
    pub fn storey() -> f32 {
        3.0
    }
}

/// Reads a styles file (a list).
pub fn parse_styles(text: &str) -> Result<Vec<BuildingStyle>, String> {
    ron::from_str(text).map_err(|error| error.to_string())
}

/// Problems with styles: ids missing or used twice, or sizes that can't work.
pub fn check_styles(styles: &[BuildingStyle]) -> Vec<String> {
    let mut problems = Vec::new();
    for (i, style) in styles.iter().enumerate() {
        let name = if style.id.is_empty() { format!("style {}", i + 1) } else { format!("style \"{}\"", style.id) };
        if style.id.is_empty() {
            problems.push(format!("{name} has no id"));
        } else if styles[..i].iter().any(|other| other.id == style.id) {
            problems.push(format!("{name} is there twice"));
        }
        let positive = [style.wall_thickness, style.inner_thickness, style.storey, style.roof.thickness];
        if positive.iter().any(|&size| !(size > 0.0 && size < 10.0)) {
            problems.push(format!("{name}: thicknesses and the storey's height must be above 0"));
        }
        let (low, high) = style.roof.pitch;
        if !(10.0..=70.0).contains(&low) || !(low..=70.0).contains(&high) {
            problems.push(format!("{name}: its roof's pitch must be from 10 to 70 degrees, the least first"));
        }
        if !(0.25..=4.0).contains(&style.scale) {
            problems.push(format!("{name}: its scale must be from 0.25 to 4"));
        }
        if style.door.size.1 >= style.storey || style.window.sill + style.window.size.1 >= style.storey {
            problems.push(format!("{name}: its doors and windows must fit in a storey"));
        }
    }
    problems
}
