//! Fishing: which fish a bait or lure draws, whether its hook suits them, how
//! long they take to bite and how long there is to strike, what a player
//! sees in the water, whether a reel can land a fish, and how traps fill.
//!
//! The fight itself (tension, line out, the fish tiring) is a simulation both
//! the server and the client step (`venture_simulation::fishing`); it takes
//! its numbers from the fish's size ([`Tuning::size`]) and the reel's.
//!
//! The numbers tuned in playtests are in [`Tuning`] (the server's come from
//! `server.ron`).
//!
//! Every fish carries a few traits (its diet, temperament, depth, size, and
//! whether it goes for flashy or natural lures), and baits and lures are
//! described by the same traits, so what bites is worked out from the
//! traits, never from lists of fish. See `design_docs/fishing_skill/`.
//!
//! Rolls are numbers from 0 to 1 passed in, so the server can use real
//! randomness and tests fixed rolls.

use serde::{Deserialize, Serialize};

/// How many levels below a fish's a player starts to see its shadow (faintly).
pub const FAINT_LEVELS: u32 = 10;

/// What a fish eats, which says which baits draw it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum Diet {
    /// Insects and larvae (worms draw them).
    Insect,
    /// Other fish.
    Fish,
    /// Whatever's on the bottom.
    Bottom,
    /// Shrimps, crabs and crayfish.
    Crustacean,
    /// Plankton, filtered from the water.
    Plankton,
    /// Plants.
    Herbivore,
    /// Anything dead.
    Scavenger,
}

/// How readily a fish commits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum Temperament {
    /// Strikes lures readily, bites quickly.
    Aggressive,
    /// Takes what comes.
    Opportunist,
    /// Wary: slow to bite, nibbles first, rarely takes a lure.
    Skittish,
}

/// Where in the water a fish feeds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum Depth {
    /// On the bottom.
    Bottom,
    /// In the middle of the water.
    Midwater,
    /// At the surface.
    Surface,
}

/// How big a fish is (or what size of fish a hook suits), smallest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum Size {
    /// Minnows, shrimps.
    Tiny,
    /// Most river fish.
    Small,
    /// Bream, mullet, trout.
    Medium,
    /// Pike, salmon, big sea fish.
    Large,
    /// The biggest of all.
    Trophy,
}

/// A lure's finish, and the kind a fish goes for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum Finish {
    /// Bright and flashing.
    Flashy,
    /// Muted, like the real thing.
    Natural,
}

/// The ways of fishing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
pub enum Method {
    /// Bait held on the bottom by a weight; the rod tip shows a bite.
    Ledger,
    /// Bait hung under a float at any depth; the float shows a bite.
    Float,
    /// A lure drawn through the middle of the water; fish hook themselves.
    Lure,
    /// A baited cage left in the water.
    Trap,
}

/// A fish's traits.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct FishTraits {
    /// What it eats.
    pub diet: Diet,
    /// How readily it bites.
    pub temperament: Temperament,
    /// Where it feeds.
    pub depth: Depth,
    /// How big it is.
    pub size: Size,
    /// The finish of lure it goes for, if it cares.
    #[serde(default)]
    pub finish: Option<Finish>,
}

/// A bait: the diets it appeals to, and the size of hook it comes on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct Bait {
    /// The fish it draws, by diet (live shrimp draws fish-eaters and
    /// crustacean-eaters).
    pub diets: Vec<Diet>,
    /// Its hook, which suits fish of about this size.
    pub hook: Size,
}

/// A lure: the fish it's made for, by their traits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "reflect", derive(bevy_reflect::Reflect))]
#[serde(deny_unknown_fields)]
pub struct Lure {
    /// The diets it looks like food to.
    pub diets: Vec<Diet>,
    /// The temperament it's made to provoke.
    pub temperament: Temperament,
    /// The depth it runs at.
    pub depth: Depth,
    /// Its finish.
    pub finish: Finish,
    /// Its hook, which suits fish of about this size.
    pub hook: Size,
}

/// How well a method reaches fish feeding at a depth (0 to 1): a ledger
/// sits on the bottom, a float hangs at any depth, a lure runs midwater,
/// and a trap sits on the bottom.
pub fn reach(method: Method, depth: Depth) -> f32 {
    match (method, depth) {
        (Method::Float, _) => 1.0,
        (Method::Ledger | Method::Trap, Depth::Bottom) | (Method::Lure, Depth::Midwater) => 1.0,
        (Method::Ledger, Depth::Midwater) | (Method::Lure, Depth::Surface) => 0.5,
        (Method::Ledger, Depth::Surface) | (Method::Lure, Depth::Bottom) => 0.25,
        (Method::Trap, _) => 0.0,
    }
}

/// How strongly a bait, fished with `method`, draws a fish (0 to 1): only
/// fish whose diet it appeals to, as far as the method reaches them.
pub fn bait_attraction(fish: &FishTraits, bait: &Bait, method: Method) -> f32 {
    if bait.diets.contains(&fish.diet) { reach(method, fish.depth) } else { 0.0 }
}

/// How strongly a lure draws a fish (0 to 1). Aggressive fish chase most
/// lures and skittish ones hardly any; the lure does best with fish of the
/// diet, temperament, depth and finish it's made for.
pub fn lure_attraction(fish: &FishTraits, lure: &Lure) -> f32 {
    let temperament = if fish.temperament == lure.temperament {
        1.0
    } else {
        match fish.temperament {
            Temperament::Aggressive => 0.6,
            Temperament::Opportunist => 0.3,
            Temperament::Skittish => 0.1,
        }
    };
    let diet = if lure.diets.contains(&fish.diet) { 1.0 } else { 0.5 };
    let finish = match fish.finish {
        Some(finish) if finish != lure.finish => 0.5,
        _ => 1.0,
    };
    let depth = if fish.depth == lure.depth { 1.0 } else { reach(Method::Lure, fish.depth) };
    temperament * diet * finish * depth
}

/// How well a hook suits a fish's size (0 to 1). A hook takes fish of its
/// own size; the smallest hooks take tiny fish too; a hook one size too
/// small rarely holds a bigger fish, and small fish rarely take a hook one
/// size too big. Anything further off, never.
pub fn hook_fit(hook: Size, fish: Size) -> f32 {
    let (hook_rank, fish_rank) = (hook as i32, fish as i32);
    match fish_rank - hook_rank {
        0 => 1.0,
        -1 if hook <= Size::Small => 1.0,
        -1 => 0.3,
        1 => 0.15,
        _ => 0.0,
    }
}

/// Picks one of several choices by weight, with `roll` from 0 to 1. `None`
/// if none has any weight.
pub fn choose(weights: &[f32], roll: f32) -> Option<usize> {
    let total: f32 = weights.iter().map(|weight| weight.max(0.0)).sum();
    if total <= 0.0 {
        return None;
    }
    let mut left = roll.clamp(0.0, 1.0) * total;
    let last = weights.iter().rposition(|&weight| weight > 0.0)?;
    weights.iter().enumerate().find_map(|(index, &weight)| {
        left -= weight.max(0.0);
        (weight > 0.0 && (left < 0.0 || index == last)).then_some(index)
    })
}

/// The numbers behind fishing that are tuned in playtests (the server's
/// are in `server.ron`, under `fishing`); the defaults are the design's.
///
/// ```ron
/// (strike_windows: (ledger: 0.6, float: 1.0), bite_waits: (aggressive: (4.0, 10.0), ...))
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Tuning {
    /// How long after a bite the player has to strike, in seconds (the
    /// server adds an allowance for the network): a float shows a bite more
    /// plainly than a rod tip, so it allows longer.
    pub strike_windows: StrikeWindows,
    /// How long a fish takes to bite once the line is in, in seconds (the
    /// least and the most), by its temperament.
    pub bite_waits: BiteWaits,
    /// How fish of each size fight.
    pub sizes: SizeTable,
    /// How much harder than its steady pull a fish pulls when it runs.
    pub peak_pull: f32,
    /// The share of a trap's catches that are bycatch (junk and critters).
    pub bycatch_share: f32,
    /// How much likelier a trap is to catch a fish its bait appeals to.
    pub trap_bait_bias: f32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Tuning {
    /// The design's numbers.
    pub const DEFAULT: Tuning = Tuning {
        strike_windows: StrikeWindows { ledger: 0.6, float: 1.0 },
        bite_waits: BiteWaits { aggressive: (4.0, 10.0), opportunist: (6.0, 16.0), skittish: (10.0, 25.0) },
        sizes: SizeTable {
            tiny: SizeTuning { pull: 0.4, run: 5.0, stamina: 4.0 },
            small: SizeTuning { pull: 1.2, run: 15.0, stamina: 6.0 },
            medium: SizeTuning { pull: 3.5, run: 40.0, stamina: 10.0 },
            large: SizeTuning { pull: 9.0, run: 90.0, stamina: 16.0 },
            trophy: SizeTuning { pull: 22.0, run: 180.0, stamina: 25.0 },
        },
        peak_pull: 2.0,
        bycatch_share: 0.25,
        trap_bait_bias: 3.0,
    };
}

/// How long after a bite the player has to strike, by method, in seconds.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StrikeWindows {
    /// With a ledger (watching the rod's tip).
    pub ledger: f32,
    /// With a float.
    pub float: f32,
}

impl Default for StrikeWindows {
    fn default() -> Self {
        Tuning::DEFAULT.strike_windows
    }
}

/// How long fish take to bite, by temperament: (least, most) seconds.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BiteWaits {
    /// Aggressive fish.
    pub aggressive: (f32, f32),
    /// Opportunists.
    pub opportunist: (f32, f32),
    /// Skittish fish.
    pub skittish: (f32, f32),
}

impl Default for BiteWaits {
    fn default() -> Self {
        Tuning::DEFAULT.bite_waits
    }
}

/// How fish of one size fight.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SizeTuning {
    /// How hard they pull at their steadiest, in kilograms of force (their
    /// runs pull up to [`Tuning::peak_pull`] times that).
    pub pull: f32,
    /// How far they run when fresh, in metres.
    pub run: f32,
    /// How long they can keep running before they're spent, in seconds.
    pub stamina: f32,
}

/// How fish of each size fight.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SizeTable {
    /// Tiny fish.
    pub tiny: SizeTuning,
    /// Small fish.
    pub small: SizeTuning,
    /// Medium fish.
    pub medium: SizeTuning,
    /// Large fish.
    pub large: SizeTuning,
    /// Trophy fish.
    pub trophy: SizeTuning,
}

impl Default for SizeTable {
    fn default() -> Self {
        Tuning::DEFAULT.sizes
    }
}

impl Tuning {
    /// How fish of a size fight.
    pub fn size(&self, size: Size) -> SizeTuning {
        let sizes = &self.sizes;
        match size {
            Size::Tiny => sizes.tiny,
            Size::Small => sizes.small,
            Size::Medium => sizes.medium,
            Size::Large => sizes.large,
            Size::Trophy => sizes.trophy,
        }
    }

    /// How long a fish of this temperament takes to bite once the line is
    /// in, in seconds, with `roll` from 0 to 1.
    pub fn bite_wait(&self, temperament: Temperament, roll: f32) -> f32 {
        let (least, most) = match temperament {
            Temperament::Aggressive => self.bite_waits.aggressive,
            Temperament::Opportunist => self.bite_waits.opportunist,
            Temperament::Skittish => self.bite_waits.skittish,
        };
        least + (most - least) * roll.clamp(0.0, 1.0)
    }

    /// How long after a bite the player has to strike, in seconds, or
    /// `None` if the fish hooks itself (lures and traps).
    pub fn strike_window(&self, method: Method) -> Option<f32> {
        match method {
            Method::Ledger => Some(self.strike_windows.ledger),
            Method::Float => Some(self.strike_windows.float),
            Method::Lure | Method::Trap => None,
        }
    }

    /// Whether a strike `since_bite` seconds after the bite (negative:
    /// before it) hooks the fish, allowing `allowance` seconds for the network.
    pub fn strike_hooks(&self, method: Method, since_bite: f32, allowance: f32) -> bool {
        match self.strike_window(method) {
            Some(window) => (0.0..=window + allowance.max(0.0)).contains(&since_bite),
            None => true,
        }
    }

    /// Whether a reel's line (of `strength` kilograms) and spool (of
    /// `capacity` metres) can land a fish of this size played well: the
    /// line holds its hardest pull, and the spool its longest run. (Played
    /// badly, a fish can still snap a line strong enough or run out a spool
    /// long enough; that's the fight.)
    pub fn reel_lands(&self, strength: f32, capacity: f32, size: Size) -> bool {
        let fish = self.size(size);
        strength >= fish.pull * self.peak_pull && capacity > fish.run
    }

    /// How strongly a trap (with `bait`, if any) draws a fish that's
    /// usually caught in traps, from the `weight` its water gives it.
    pub fn trap_weight(&self, fish: &FishTraits, weight: f32, bait: Option<&Bait>) -> f32 {
        let baited = bait.is_some_and(|bait| bait.diets.contains(&fish.diet));
        weight.max(0.0) * if baited { self.trap_bait_bias } else { 1.0 }
    }

    /// What's wrong with these numbers, if anything (for the server's settings).
    pub fn problems(&self) -> Option<String> {
        let positive = |value: f32| value > 0.0 && value.is_finite();
        let windows = self.strike_windows;
        let waits = [self.bite_waits.aggressive, self.bite_waits.opportunist, self.bite_waits.skittish];
        let sizes = [self.sizes.tiny, self.sizes.small, self.sizes.medium, self.sizes.large, self.sizes.trophy];
        if !(positive(windows.ledger) && positive(windows.float)) {
            Some("strike windows must be more than 0 seconds".to_string())
        } else if !waits.iter().all(|&(least, most)| least >= 0.0 && most >= least && most.is_finite()) {
            Some("each bite wait must be (least, most) seconds, with most at least least".to_string())
        } else if !sizes.iter().all(|size| positive(size.pull) && positive(size.run) && positive(size.stamina)) {
            Some("each size's pull, run and stamina must be more than 0".to_string())
        } else if !(positive(self.peak_pull) && positive(self.trap_bait_bias)) {
            Some("peak_pull and trap_bait_bias must be more than 0".to_string())
        } else if !(0.0..=1.0).contains(&self.bycatch_share) {
            Some("bycatch_share must be from 0 to 1".to_string())
        } else {
            None
        }
    }
}

/// How many nibbles come before the bite (with `roll` from 0 to 1): none to
/// two, and one more for a skittish fish. Lures have none (fish strike).
pub fn nibbles(temperament: Temperament, method: Method, roll: f32) -> u32 {
    if matches!(method, Method::Lure | Method::Trap) {
        return 0;
    }
    let some = (roll.clamp(0.0, 0.999) * 3.0) as u32;
    some + u32::from(temperament == Temperament::Skittish)
}

/// How a player sees a fish in the water.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Shadow {
    /// Plainly: they can catch it.
    Solid,
    /// Faintly: they'll be able to within [`FAINT_LEVELS`] levels.
    Faint,
    /// Not at all.
    Hidden,
}

/// How a player at `skill_level` in fishing sees a fish that can be caught
/// from `fish_level`.
pub fn shadow(skill_level: u32, fish_level: u32) -> Shadow {
    if skill_level >= fish_level {
        Shadow::Solid
    } else if skill_level + FAINT_LEVELS >= fish_level {
        Shadow::Faint
    } else {
        Shadow::Hidden
    }
}

/// How many catches a trap holds after soaking for `soaked` seconds: one
/// every `soak / capacity` seconds, up to `capacity` after `soak`.
pub fn trap_catches(soaked: f32, soak: f32, capacity: u32) -> u32 {
    if soak <= 0.0 {
        return capacity;
    }
    ((soaked.max(0.0) / soak * capacity as f32).floor() as u32).min(capacity)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TUNING: &Tuning = &Tuning::DEFAULT;

    fn fish(diet: Diet, temperament: Temperament, depth: Depth, size: Size) -> FishTraits {
        FishTraits { diet, temperament, depth, size, finish: None }
    }

    const DACE: FishTraits = FishTraits {
        diet: Diet::Insect,
        temperament: Temperament::Skittish,
        depth: Depth::Midwater,
        size: Size::Small,
        finish: Some(Finish::Natural),
    };
    const PERCH: FishTraits = FishTraits {
        diet: Diet::Crustacean,
        temperament: Temperament::Aggressive,
        depth: Depth::Midwater,
        size: Size::Small,
        finish: Some(Finish::Flashy),
    };

    fn worm() -> Bait {
        Bait { diets: vec![Diet::Insect], hook: Size::Small }
    }

    fn spinner() -> Lure {
        Lure {
            diets: vec![Diet::Fish],
            temperament: Temperament::Aggressive,
            depth: Depth::Midwater,
            finish: Finish::Flashy,
            hook: Size::Small,
        }
    }

    #[test]
    fn bait_draws_fish_of_its_diet_where_the_method_reaches() {
        assert_eq!(bait_attraction(&DACE, &worm(), Method::Float), 1.0);
        assert_eq!(bait_attraction(&DACE, &worm(), Method::Ledger), 0.5, "a ledger sits below midwater fish");
        assert_eq!(bait_attraction(&PERCH, &worm(), Method::Float), 0.0, "perch don't eat worms");
        let live_shrimp = Bait { diets: vec![Diet::Fish, Diet::Crustacean], hook: Size::Medium };
        assert_eq!(bait_attraction(&PERCH, &live_shrimp, Method::Float), 1.0);
    }

    #[test]
    fn lures_draw_aggressive_fish_and_hardly_any_skittish_ones() {
        let perch = lure_attraction(&PERCH, &spinner());
        let dace = lure_attraction(&DACE, &spinner());
        assert!(perch >= 0.5, "perch chase a flashy spinner: {perch}");
        assert!(dace < 0.05, "dace hardly ever: {dace}");
        let plug = Lure { finish: Finish::Natural, ..spinner() };
        assert!(lure_attraction(&PERCH, &plug) < perch, "perch like flash");
    }

    #[test]
    fn hooks_suit_fish_of_their_size() {
        assert_eq!(hook_fit(Size::Small, Size::Small), 1.0);
        assert_eq!(hook_fit(Size::Small, Size::Tiny), 1.0, "small hooks take tiny fish too");
        assert!(hook_fit(Size::Small, Size::Medium) < 0.2, "and rarely a bigger one");
        assert_eq!(hook_fit(Size::Small, Size::Large), 0.0);
        assert!(hook_fit(Size::Medium, Size::Small) < 0.5, "medium hooks rarely take small fish");
        assert_eq!(hook_fit(Size::Medium, Size::Tiny), 0.0);
    }

    #[test]
    fn choosing_goes_by_weight() {
        let weights = [1.0, 0.0, 3.0];
        assert_eq!(choose(&weights, 0.0), Some(0));
        assert_eq!(choose(&weights, 0.2), Some(0));
        assert_eq!(choose(&weights, 0.3), Some(2));
        assert_eq!(choose(&weights, 1.0), Some(2), "a roll of 1 still picks the last");
        assert_eq!(choose(&[0.0, 0.0], 0.5), None);
        assert_eq!(choose(&[], 0.5), None);
    }

    #[test]
    fn skittish_fish_are_slow_and_nibble() {
        assert!(TUNING.bite_wait(Temperament::Aggressive, 1.0) <= TUNING.bite_wait(Temperament::Skittish, 0.0));
        assert_eq!(nibbles(Temperament::Skittish, Method::Float, 0.0), 1);
        assert_eq!(nibbles(Temperament::Opportunist, Method::Ledger, 0.99), 2);
        assert_eq!(nibbles(Temperament::Skittish, Method::Lure, 0.5), 0, "fish strike lures");
    }

    #[test]
    fn strikes_must_come_in_time() {
        assert!(TUNING.strike_hooks(Method::Ledger, 0.3, 0.0));
        assert!(!TUNING.strike_hooks(Method::Ledger, -0.1, 0.2), "too early");
        assert!(!TUNING.strike_hooks(Method::Ledger, 0.7, 0.0), "too late");
        assert!(TUNING.strike_hooks(Method::Ledger, 0.7, 0.2), "unless the network was slow");
        assert!(TUNING.strike_hooks(Method::Float, 0.9, 0.0), "a float gives longer");
        assert!(TUNING.strike_hooks(Method::Lure, 5.0, 0.0), "lures hook the fish themselves");
        // No window is too tight to play over the network.
        for method in [Method::Ledger, Method::Float] {
            assert!(TUNING.strike_window(method).unwrap() >= 0.33);
        }
    }

    #[test]
    fn shadows_show_what_can_be_caught_and_what_soon_can() {
        assert_eq!(shadow(5, 5), Shadow::Solid);
        assert_eq!(shadow(5, 1), Shadow::Solid);
        assert_eq!(shadow(5, 15), Shadow::Faint);
        assert_eq!(shadow(5, 16), Shadow::Hidden);
    }

    #[test]
    fn a_reel_lands_fish_up_to_its_tier() {
        // The slice's reels: the first lands small fish, the second medium.
        let (first, second) = ((3.0, 30.0), (8.0, 90.0));
        assert!(TUNING.reel_lands(first.0, first.1, Size::Small));
        assert!(!TUNING.reel_lands(first.0, first.1, Size::Medium));
        assert!(TUNING.reel_lands(second.0, second.1, Size::Medium));
        assert!(!TUNING.reel_lands(second.0, second.1, Size::Large));
        // Bigger fish pull harder and run further.
        let sizes = [Size::Tiny, Size::Small, Size::Medium, Size::Large, Size::Trophy];
        let fish = |size| TUNING.size(size);
        assert!(sizes.windows(2).all(|pair| fish(pair[0]).pull < fish(pair[1]).pull));
        assert!(sizes.windows(2).all(|pair| fish(pair[0]).run < fish(pair[1]).run));
        assert!(sizes.windows(2).all(|pair| fish(pair[0]).stamina < fish(pair[1]).stamina));
        assert_eq!(TUNING.problems(), None, "the defaults are sound");
    }

    #[test]
    fn traps_fill_as_they_soak() {
        assert_eq!(trap_catches(0.0, 1200.0, 3), 0);
        assert_eq!(trap_catches(399.0, 1200.0, 3), 0);
        assert_eq!(trap_catches(400.0, 1200.0, 3), 1);
        assert_eq!(trap_catches(1200.0, 1200.0, 3), 3);
        assert_eq!(trap_catches(1e9, 1200.0, 3), 3, "and no more");
        let crayfish = fish(Diet::Scavenger, Temperament::Opportunist, Depth::Bottom, Size::Small);
        let shrimp_bits = Bait { diets: vec![Diet::Crustacean, Diet::Scavenger], hook: Size::Small };
        assert_eq!(TUNING.trap_weight(&crayfish, 2.0, None), 2.0);
        assert_eq!(TUNING.trap_weight(&crayfish, 2.0, Some(&shrimp_bits)), 2.0 * TUNING.trap_bait_bias);
        assert_eq!(TUNING.trap_weight(&crayfish, 2.0, Some(&worm())), 2.0);
    }
}
