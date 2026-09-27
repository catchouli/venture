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
//! It's deterministic: the same setup (with its seed) and the same inputs
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
