use venture_structures::buildings::parse_styles;
use venture_world::{
    BakedBuilding, BakedWall, BuildingPlan, Chunk, ChunkPos, DoorPlan, Fluid, FluidBody, FluidKind, Prop, PropKind,
    RoofShape, WallKind, WallRef, WallSample,
};

use super::*;

/// A flat chunk at the world's origin (0, 0), with its ground at height 0.
fn flat() -> Chunk {
    Chunk::new(ChunkPos::new(0, 0))
}

fn world_of(chunk: &Chunk) -> Collision {
    let mut collision = Collision::default();
    collision.add_chunk(chunk);
    collision
}

/// A fence along x = 20, from y = 5 to y = 35, sampled every metre.
fn fence() -> BakedWall {
    BakedWall {
        id: "fence".into(),
        kind: WallKind::Fence,
        height: 1.2,
        seed: 1,
        samples: (5..=35).map(|y| WallSample { x: 20.0, y: y as f32, ground: 0.0, open: false }).collect(),
    }
}

fn styles() -> Vec<BuildingStyle> {
    parse_styles(include_str!("../../../content/buildings/styles.ron")).expect("the content's styles parse")
}

/// A 10 × 8 m stone cottage with its corner at (10, 10), and a door in the
/// middle of its south wall (from (10, 10) to (20, 10)).
fn cottage() -> BakedBuilding {
    let plan = BuildingPlan {
        id: "cottage".into(),
        style: "stone_cottage".into(),
        floors: 1,
        seed: 1,
        roof: RoofShape::Gable,
        chimney: None,
        outline: vec![(10.0, 10.0), (20.0, 10.0), (20.0, 18.0), (10.0, 18.0)],
        inner_walls: Vec::new(),
        doors: vec![DoorPlan { wall: WallRef::Outer(0), along: 0.5 }],
    };
    BakedBuilding { plan, floor: 0.0, lowest: 0.0 }
}

#[test]
fn open_ground_fits_and_is_stood_on() {
    let collision = world_of(&flat());
    assert!(collision.fits((10.5, 10.5, 0.0)));
    let ground = collision.surface_below((10.5, 10.5), 5.0, 10.0).expect("there's ground");
    assert!(ground.abs() < 1e-4, "{ground}");
}

#[test]
fn fences_block_only_as_far_as_they_are_thick() {
    let mut collision = world_of(&flat());
    collision.set_structures(&[fence()], &[], &[]);
    // The fence is 0.15 m each side of x = 20, and characters are 0.3 m round.
    assert!(!collision.fits((20.0, 20.0, 0.0)), "on it");
    assert!(!collision.fits((20.4, 20.0, 0.0)), "brushing it");
    assert!(collision.fits((20.5, 20.0, 0.0)), "just clear of it (a cell's grid would stop you a metre away)");
    assert!(collision.fits((19.5, 20.0, 0.0)));
    // Its ends are open.
    assert!(collision.fits((20.0, 3.0, 0.0)));
}

#[test]
fn running_into_a_fence_stops_at_its_face_and_slides_along_it() {
    let mut collision = world_of(&flat());
    collision.set_structures(&[fence()], &[], &[]);
    let mut feet = (17.0, 20.0, 0.0);
    // Running diagonally into it, at 5 m/s for 2 s.
    for _ in 0..40 {
        feet = collision.move_character(feet, (0.25 * 0.7, 0.25 * 0.7, -0.05), 0.05).feet;
    }
    assert!(feet.0 < 20.0 - 0.15 - CHARACTER_RADIUS + 0.05, "never through it: {feet:?}");
    assert!(feet.0 > 19.4, "right up against it: {feet:?}");
    assert!(feet.1 > 26.0, "and slid along it: {feet:?}");
}

#[test]
fn doorways_are_open_and_walls_are_not() {
    let mut collision = world_of(&flat());
    collision.set_structures(&[], &[cottage()], &styles());
    assert!(collision.fits((15.0, 10.3, 0.0)), "in the doorway");
    assert!(collision.fits((15.0, 14.0, 0.0)), "inside");
    assert!(!collision.fits((12.0, 10.3, 0.0)), "in its south wall, beside the door");
    assert!(!collision.fits((10.3, 14.0, 0.0)), "in its west wall");
    assert!(collision.fits((15.0, 9.0, 0.0)), "outside the door");

    // Walking in through the door works; walking in through a wall doesn't.
    let walk = |from: (f32, f32, f32)| {
        let mut feet = from;
        for _ in 0..60 {
            feet = collision.move_character(feet, (0.0, 0.25, -0.05), 0.05).feet;
        }
        feet
    };
    assert!(walk((15.0, 8.0, 0.0)).1 > 14.0, "in through the door");
    assert!(walk((12.0, 8.0, 0.0)).1 < 10.0, "not through the wall");
}

#[test]
fn trees_block_round_their_trunks_and_bushes_do_not() {
    let mut chunk = flat();
    let tree = |x: f32, scale: f32| Prop {
        kind: PropKind::Tree { species: 0, variant: 1 },
        x,
        y: 30.0,
        ground_height: 0.0,
        scale,
        rotation: 0.0,
    };
    chunk.props.push(tree(30.0, 1.0));
    chunk.props.push(tree(40.0, venture_world::BUSH_SCALE * 0.5));
    let mut collision = world_of(&chunk);
    // A mature trunk is 0.45 m round.
    assert!(!collision.fits((30.6, 30.0, 0.0)));
    assert!(collision.fits((30.8, 30.0, 0.0)));
    assert!(collision.fits((40.0, 30.0, 0.0)), "bushes can be pushed through");

    // Felled, it's out of the way; grown again, it's back.
    collision.set_prop_standing(ChunkPos::new(0, 0), 0, false);
    assert!(collision.fits((30.0, 30.0, 0.0)));
    collision.set_prop_standing(ChunkPos::new(0, 0), 0, true);
    assert!(!collision.fits((30.0, 30.0, 0.0)));
}

#[test]
fn deep_water_is_no_entry_but_shallows_are_waded() {
    let mut chunk = flat();
    let water = |level| Some(Fluid { kind: FluidKind::Water, body: FluidBody::Lake, level });
    for x in 30..64 {
        for y in 0..64 {
            chunk.cell_mut(x, y).fluid = water(if x < 35 { 0.3 } else { 2.0 });
        }
    }
    let collision = world_of(&chunk);
    assert!(collision.fits((32.5, 10.0, 0.0)), "shallows");
    assert!(!collision.fits((36.5, 10.0, 0.0)), "deep");
    let mut feet = (30.0, 10.0, 0.0);
    for _ in 0..40 {
        feet = collision.move_character(feet, (0.25, 0.1, -0.05), 0.05).feet;
    }
    assert!(feet.0 < 35.0 && feet.1 > 12.0, "stopped at the deep water's edge, sliding along it: {feet:?}");
}

#[test]
fn slopes_too_steep_can_be_come_down_but_not_gone_up() {
    // A bank rising 3 m over 2 m (56°) from x = 20 to x = 22, level either side.
    let mut chunk = flat();
    for y in 0..=64 {
        for x in 0..=64 {
            let height = ((x as f32 - 20.0) * 1.5).clamp(0.0, 3.0);
            chunk.set_corner_height(x, y, height);
        }
    }
    let collision = world_of(&chunk);
    let gravity = |from: (f32, f32, f32), way: f32| {
        let (mut feet, mut falling) = (from, 0.0_f32);
        for _ in 0..60 {
            let moved = collision.move_character(feet, (0.25 * way, 0.0, falling * 0.05), 0.05);
            feet = moved.feet;
            falling = if moved.grounded { 0.0 } else { falling - 20.0 * 0.05 };
        }
        feet
    };
    let up = gravity((17.0, 30.0, 0.0), 1.0);
    assert!(up.0 < 21.0 && up.2 < 1.6, "stopped at the foot of the bank: {up:?}");
    let down = gravity((25.0, 30.0, 3.0), -1.0);
    assert!(down.0 < 19.0 && down.2 < 0.2, "came down it: {down:?}");
}

#[test]
fn low_ledges_are_stepped_up() {
    // A 0.3 m high step at x = 20.
    let mut chunk = flat();
    for y in 0..=64 {
        for x in 21..=64 {
            chunk.set_corner_height(x, y, 0.3);
        }
    }
    // (A cliff in a heightfield is a slope over one cell: 0.3 m over 1 m is 17°,
    // so this checks it's walked up; walls' kerbs are real steps.)
    let collision = world_of(&chunk);
    let mut feet = (18.0, 30.0, 0.0);
    for _ in 0..40 {
        feet = collision.move_character(feet, (0.25, 0.0, -0.05), 0.05).feet;
    }
    assert!(feet.0 > 25.0 && (feet.2 - 0.3).abs() < 0.05, "{feet:?}");
}

#[test]
fn chunks_come_and_go() {
    let mut collision = world_of(&flat());
    assert!(collision.has_chunk(ChunkPos::new(0, 0)));
    assert!(collision.no_entry(70, 10), "not loaded");
    collision.remove_chunk(ChunkPos::new(0, 0));
    assert!(collision.surface_below((10.0, 10.0), 5.0, 10.0).is_none());
    assert!(!collision.fits((10.0, 10.0, 0.0)));
}
