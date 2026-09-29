//! Bridges (see `docs/plan-bridges.md`): what an author draws (a
//! [`BridgePlan`]: a path, a kind, a width and a curve), and a bridge as the
//! bake leaves it ([`BakedBridge`]: the plan, and its deck sampled along the
//! path with its height). Clients build its meshes (with
//! `venture_structures`), and it's solid, and walked on, like the ground.

use serde::{Deserialize, Serialize};

/// What a bridge is built of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum BridgeKind {
    /// Dry stone, on an arch, with low walls either side.
    #[default]
    Stone,
    /// Wooden planks on ropes, between wooden posts, with rope handrails.
    Rope,
}

impl BridgeKind {
    /// Every kind, in the order they're offered.
    pub const ALL: [BridgeKind; 2] = [BridgeKind::Stone, BridgeKind::Rope];

    /// Its name, for people.
    pub fn name(self) -> &'static str {
        match self {
            BridgeKind::Stone => "Stone bridge",
            BridgeKind::Rope => "Rope bridge",
        }
    }

    /// How wide one usually is, in metres.
    pub fn usual_width(self) -> f32 {
        match self {
            BridgeKind::Stone => 4.5,
            BridgeKind::Rope => 1.8,
        }
    }

    /// How much one usually curves (up in the middle; down, if negative), in metres.
    pub fn usual_curve(self) -> f32 {
        match self {
            BridgeKind::Stone => 1.5,
            BridgeKind::Rope => -0.8,
        }
    }
}

/// A bridge as an author draws it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub struct BridgePlan {
    /// Its id.
    pub id: String,
    /// What it's built of.
    #[serde(default)]
    pub kind: BridgeKind,
    /// The points its deck's middle passes through, end to end, in metres.
    pub points: Vec<(f32, f32)>,
    /// How wide its deck is, in metres.
    pub width: f32,
    /// How far its middle rises above a straight line between its ends (or
    /// hangs below it, if negative), in metres.
    pub curve: f32,
    /// Its seed: the same seed always builds the same bridge.
    #[serde(default)]
    pub seed: u64,
}

/// A bridge as the bake leaves it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BakedBridge {
    /// What the author drew (or the bake placed).
    pub plan: BridgePlan,
    /// Its deck's middle, sampled evenly along its path: `[x, y, height of
    /// the deck's top]`, in metres. Its ends are on the ground.
    pub deck: Vec<[f32; 3]>,
    /// The ground's height under each sample (for its piers and arch), in metres.
    pub ground: Vec<f32>,
}
