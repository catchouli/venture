//! The fight with a hooked fish, stepped once a tick from whether the player
//! is reeling.
//!
//! The fish runs and rests. Running, it pulls hard and swims away; resting,
//! it pulls lightly and can be drawn in. Each run tires it, so its runs get
//! weaker and shorter. The player either reels (drawing line in, which adds
//! to the tension) or lets it run (the line pays out, the tension drops).
//! The fight is lost if the tension stays over the line's strength (it
//! snaps) or the fish takes more line than the reel holds (it spools out),
//! and won when the fish is drawn in to the bank.
//!
//! It also says which fish are shoaling where ([`shoals`]): the water is
//! divided into square regions, and which fish gather in each changes with
//! the hour of the day. Both sides work it out the same way from the world's
//! seed, so no messages are needed: each client shows the shadows (as its
//! player can see them), and the server makes a fish that's shoaling where a
//! line goes in likelier to bite.
//!
//! The fight is deterministic: the same setup (with its seed) and the same inputs
//! give the same fight on the server (which decides) and the client (which
//! can run it ahead, so reeling feels instant). It uses only arithmetic, no
//! functions whose results could differ between machines.

use serde::{Deserialize, Serialize};

use crate::TICK_RATE;

/// How fast reeling draws line in, in metres a second (less whatever a
/// running fish takes).
pub const REEL_SPEED: f32 = 3.0;
/// How much reeling adds to the tension, as a share of the line's strength.
pub const REEL_EFFORT: f32 = 0.3;
/// How much of the fish's pull reaches the line while it's let run.
pub const SLACK_SHARE: f32 = 0.35;
/// How long the tension may stay over the line's strength before it snaps,
/// in seconds (a moment to stop reeling).
pub const SNAP_GRACE: f32 = 0.3;
/// How close the fish must come to be landed, in metres.
pub const LAND_DISTANCE: f32 = 1.5;
/// How much of its strength a resting fish regains a second.
const RECOVERY: f32 = 0.02;

/// What a fight starts from.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct FightSetup {
    /// How hard the fish pulls when steady, in kilograms (it pulls up to
    /// twice that running).
    pub pull: f32,
    /// How far it can run when fresh, in metres.
    pub run: f32,
    /// How long its running takes to tire it out, in seconds.
    pub stamina: f32,
    /// The line's strength, in kilograms.
    pub strength: f32,
    /// How much line the reel holds, in metres.
    pub capacity: f32,
    /// How far out the fish is hooked, in metres.
    pub distance: f32,
    /// Seeds the fish's runs and rests.
    pub seed: u64,
}

/// How a fight is going.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fight {
    /// What it started from.
    pub setup: FightSetup,
    /// How far out the fish is: the line out, in metres.
    pub distance: f32,
    /// How much fight the fish has left, from 1 (fresh) to 0 (spent).
    pub strength: f32,
    /// The pull on the line, in kilograms.
    pub tension: f32,
    /// Whether the fish is running.
    pub running: bool,
    /// Seconds until it changes between running and resting.
    pub phase_left: f32,
    /// Seconds the tension has been over the line's strength.
    pub over: f32,
    /// Ticks stepped so far.
    pub ticks: u32,
    rng: u64,
}

/// How a fight ended, if it has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FightEnd {
    /// The fish is drawn in.
    Landed,
    /// The line snapped.
    Snapped,
    /// The fish took all the line.
    SpooledOut,
}

impl Fight {
    /// A fight just hooked: the fish starts with a run.
    pub fn new(setup: FightSetup) -> Self {
        let mut fight = Self {
            setup,
            distance: setup.distance,
            strength: 1.0,
            tension: 0.0,
            running: true,
            phase_left: 0.0,
            over: 0.0,
            ticks: 0,
            // (Never zero: xorshift would stay at zero.)
            rng: setup.seed | 1,
        };
        fight.phase_left = fight.run_length();
        fight
    }

    /// Steps one tick, with the player reeling or not. Returns how it ended,
    /// if it did this tick.
    pub fn step(&mut self, reeling: bool) -> Option<FightEnd> {
        let dt = 1.0 / TICK_RATE;
        let setup = self.setup;
        self.ticks += 1;

        // The fish: running (strong, swimming away) or resting (weak, still).
        let vigour = 0.5 + 0.5 * self.strength;
        let (pull, away) = if self.running {
            (setup.pull * 2.0 * vigour, setup.run / 8.0 * vigour)
        } else {
            (setup.pull * 0.5 * vigour, 0.0)
        };
        if self.running {
            // Running tires it, faster against a reeling angler.
            let effort = if reeling { 1.5 } else { 1.0 };
            self.strength = (self.strength - dt * effort / setup.stamina.max(0.1)).max(0.0);
        } else {
            self.strength = (self.strength + RECOVERY * dt).min(1.0);
        }

        // The line: reeling draws it in, against the fish's weight in the
        // water (however tired it is) and its pull; letting it run pays out.
        if reeling {
            self.tension = setup.pull + 0.5 * pull + REEL_EFFORT * setup.strength;
            self.distance += (away - REEL_SPEED) * dt;
        } else {
            self.tension = pull * SLACK_SHARE;
            self.distance += away * dt;
        }

        self.phase_left -= dt;
        if self.phase_left <= 0.0 {
            self.running = !self.running;
            self.phase_left = if self.running { self.run_length() } else { self.rest_length() };
        }

        self.over = if self.tension > setup.strength { self.over + dt } else { 0.0 };
        if self.over >= SNAP_GRACE {
            Some(FightEnd::Snapped)
        } else if self.distance > setup.capacity {
            Some(FightEnd::SpooledOut)
        } else if self.distance <= LAND_DISTANCE {
            self.distance = LAND_DISTANCE;
            Some(FightEnd::Landed)
        } else {
            None
        }
    }

    /// A run's length, in seconds: 1 to 3, shorter as the fish tires.
    fn run_length(&mut self) -> f32 {
        (1.0 + 2.0 * self.roll()) * (0.4 + 0.6 * self.strength)
    }

    /// A rest's length, in seconds: 1 to 2.5.
    fn rest_length(&mut self) -> f32 {
        1.0 + 1.5 * self.roll()
    }

    /// A number from 0 to 1 (xorshift64).
    fn roll(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// The side of a shoaling region, in metres.
pub const SHOAL_REGION: f32 = 12.0;
/// The most shoals a region has at once.
const MOST_SHOALS: u64 = 3;
/// The most fish in a shoal.
const MOST_IN_SHOAL: u64 = 4;

/// Some fish of one kind gathered in a region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shoal {
    /// Which fish (an index into the list given to [`shoals`]).
    pub fish: usize,
    /// How many.
    pub count: u32,
    /// A number to vary how they look and move by.
    pub seed: u64,
}

/// The region a point (in metres) is in.
pub fn shoal_region((x, y): (f32, f32)) -> (i32, i32) {
    ((x / SHOAL_REGION).floor() as i32, (y / SHOAL_REGION).floor() as i32)
}

/// The cells (their `(x, y)`) of a region.
pub fn region_cells((rx, ry): (i32, i32)) -> impl Iterator<Item = (i32, i32)> {
    let side = SHOAL_REGION as i32;
    (ry * side..(ry + 1) * side).flat_map(move |y| (rx * side..(rx + 1) * side).map(move |x| (x, y)))
}

/// The kind of water (body and biome, or whatever pairs are given) most of a
/// region's water cells are, ties going to the pair that comes first in the
/// list. `None` for a region with no water.
pub fn most_common_water<T: Copy + PartialEq>(kinds: &[T]) -> Option<T> {
    let mut best: Option<(T, usize)> = None;
    for &kind in kinds {
        let count = kinds.iter().filter(|&&other| other == kind).count();
        if best.is_none_or(|(_, most)| count > most) {
            best = Some((kind, count));
        }
    }
    best.map(|(kind, _)| kind)
}

/// The fish shoaling in a region of water at an hour of the day (0 to 23),
/// chosen from the fish that live there (their relative numbers in
/// `weights`, in an order both sides agree on, such as by id). Only integer
/// arithmetic and sums decide it, so every machine gets the same answer.
pub fn shoals(world_seed: u64, (x, y): (i32, i32), hour: u32, weights: &[f32]) -> Vec<Shoal> {
    let total: f32 = weights.iter().map(|weight| weight.max(0.0)).sum();
    if total <= 0.0 {
        return Vec::new();
    }
    let mut state = world_seed
        ^ (x as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as i64 as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
        ^ u64::from(hour).wrapping_mul(0x1656_67B1_9E37_79F9);
    let count = mix(&mut state) % (MOST_SHOALS + 1);
    (0..count)
        .filter_map(|_| {
            let roll = (mix(&mut state) >> 40) as f32 / (1u64 << 24) as f32;
            let mut left = roll * total;
            let fish = weights.iter().position(|&weight| {
                left -= weight.max(0.0);
                left < 0.0
            });
            let fish = fish.or_else(|| weights.iter().rposition(|&weight| weight > 0.0))?;
            let count = 1 + (mix(&mut state) % MOST_IN_SHOAL) as u32;
            Some(Shoal { fish, count, seed: mix(&mut state) })
        })
        .collect()
}

/// The next number from a SplitMix64 generator.
fn mix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small fish on the first reel (see `venture_rpg::fishing`).
    fn small_fish() -> FightSetup {
        FightSetup { pull: 1.2, run: 15.0, stamina: 8.0, strength: 3.0, capacity: 30.0, distance: 8.0, seed: 42 }
    }

    /// Plays a fight with a player who reels only while the fish rests.
    fn play_well(setup: FightSetup) -> (FightEnd, u32) {
        let mut fight = Fight::new(setup);
        loop {
            let reeling = !fight.running;
            if let Some(end) = fight.step(reeling) {
                return (end, fight.ticks);
            }
            assert!(fight.ticks < 20 * 600, "a fight shouldn't last ten minutes");
        }
    }

    #[test]
    fn reeling_between_runs_lands_a_fish_the_reel_suits() {
        for seed in 0..50 {
            let (end, ticks) = play_well(FightSetup { seed, ..small_fish() });
            assert_eq!(end, FightEnd::Landed, "seed {seed}");
            let seconds = ticks as f32 / TICK_RATE;
            assert!((3.0..60.0).contains(&seconds), "seed {seed}: {seconds} s");
        }
    }

    #[test]
    fn reeling_against_a_fresh_run_snaps_the_line() {
        let mut fight = Fight::new(small_fish());
        let end = (0..200).find_map(|_| fight.step(true));
        assert_eq!(end, Some(FightEnd::Snapped));
    }

    #[test]
    fn a_fish_let_run_takes_all_the_line() {
        let mut fight = Fight::new(FightSetup { capacity: 12.0, ..small_fish() });
        let end = (0..20 * 120).find_map(|_| fight.step(false));
        assert_eq!(end, Some(FightEnd::SpooledOut));
    }

    #[test]
    fn a_fish_too_big_for_the_reel_wins_even_played_well() {
        // A medium fish on the first reel.
        let setup = FightSetup { pull: 3.5, run: 40.0, stamina: 14.0, ..small_fish() };
        for seed in 0..20 {
            assert_ne!(play_well(FightSetup { seed, ..setup }).0, FightEnd::Landed, "seed {seed}");
        }
    }

    #[test]
    fn the_next_reel_lands_medium_fish() {
        let setup = FightSetup { pull: 3.5, run: 40.0, stamina: 10.0, strength: 8.0, capacity: 90.0, ..small_fish() };
        for seed in 0..20 {
            let (end, ticks) = play_well(FightSetup { seed, ..setup });
            assert_eq!(end, FightEnd::Landed, "seed {seed}");
            let seconds = ticks as f32 / TICK_RATE;
            assert!((10.0..90.0).contains(&seconds), "seed {seed}: {seconds} s");
        }
    }

    #[test]
    fn shoals_are_the_same_everywhere_and_change_with_the_hour() {
        let weights = [1.0, 1.0, 1.0];
        assert_eq!(shoals(7, (3, 4), 12, &weights), shoals(7, (3, 4), 12, &weights));
        let mut differ = 0;
        let mut shoaling = 0;
        for x in 0..50 {
            let now = shoals(7, (x, 4), 12, &weights);
            differ += usize::from(now != shoals(7, (x, 4), 13, &weights));
            shoaling += now.len();
            assert!(now.iter().all(|shoal| shoal.fish < 3 && (1..=4).contains(&shoal.count)));
        }
        assert!(differ > 25, "the fish move on with the hours");
        assert!((50..=100).contains(&shoaling), "one or two shoals a region: {shoaling}");
        assert!(shoals(7, (3, 4), 12, &[]).is_empty());
        let only_second = [0.0, 1.0];
        assert!((0..50).flat_map(|x| shoals(1, (x, 0), 0, &only_second)).all(|shoal| shoal.fish == 1));
        assert_eq!(shoal_region((13.0, -1.0)), (1, -1));
        assert_eq!(region_cells((1, -1)).count(), 144);
        assert!(region_cells((1, -1)).all(|cell| shoal_region((cell.0 as f32 + 0.5, cell.1 as f32 + 0.5)) == (1, -1)));
        assert_eq!(most_common_water(&['a', 'b', 'b', 'a', 'c']), Some('a'), "ties go to the first");
        assert_eq!(most_common_water(&['a', 'b', 'b']), Some('b'));
        assert_eq!(most_common_water::<char>(&[]), None);
    }

    #[test]
    fn the_same_setup_and_inputs_give_the_same_fight() {
        let inputs = |tick: u32| tick % 7 < 3;
        let play = || {
            let mut fight = Fight::new(small_fish());
            let mut tick = 0;
            while fight.step(inputs(tick)).is_none() && tick < 2000 {
                tick += 1;
            }
            fight
        };
        assert_eq!(play(), play());
    }
}
