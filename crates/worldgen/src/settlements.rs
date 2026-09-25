//! Choosing where settlements (future towns and villages) go.
//!
//! Good sites are flat, dry, not too high, and close to (but not on) a river.
//! Candidates are scored, then picked best-first, skipping any too close to
//! a site already chosen.
//!
//! This only chooses the sites. Each settlement is given level ground and a
//! square through the overlay system (see [`crate::overlay`]); buildings will
//! come later.

use std::collections::VecDeque;

use venture_world::Biome;

use crate::WorldGenConfig;
use crate::geometry::Point;
use crate::grid::{cell_distance, cell_hash, neighbours};
use crate::world_map::MapGrid;

/// A place where a town or village will be.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settlement {
    /// Centre of the settlement, in metres.
    pub position: Point,
    /// The biome it is in, which decides who lives there.
    pub biome: Biome,
}

/// Map point indices of the chosen settlement sites, best site first.
pub fn choose_settlements(config: &WorldGenConfig, grid: &MapGrid) -> Vec<usize> {
    let river_distance = distance_to_rivers(grid);
    let river_reach = config.river_max_half_width + config.floodplain_width + config.settlement_radius * 0.5;
    let clear_of_river = (river_reach / grid.cell_size).ceil() as u32;
    let highest_site = config.highlands.rock_start_height + config.highlands.base_height;

    let mut candidates: Vec<(f32, usize)> = (0..grid.size * grid.size)
        .filter(|&index| {
            let height = grid.land_height[index];
            !grid.is_water(index)
                && !grid.is_river[index]
                && !grid.is_edge(index)
                && height > config.sea_level + 1.0
                && height < highest_site
        })
        .map(|index| {
            let slope = neighbours(index, grid.size)
                .map(|n| (grid.land_height[n] - grid.land_height[index]).abs())
                .fold(0.0, f32::max);
            // Best just beyond reach of the river's floodplain, so the town's
            // level ground isn't carved away.
            let distance = river_distance[index];
            let river_bonus = if distance < clear_of_river {
                -10.0
            } else if distance < clear_of_river + 4 {
                2.0
            } else if distance < clear_of_river + 10 {
                1.0
            } else {
                0.0
            };
            let jitter = cell_hash(index, config.seed) * 0.5;
            (river_bonus - slope * 2.0 + jitter, index)
        })
        .collect();
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));

    let min_cells = config.settlement_spacing / grid.cell_size;
    let mut chosen: Vec<usize> = Vec::new();
    for (_, index) in candidates {
        let far_enough = chosen.iter().all(|&other| cell_distance(index, other, grid.size) >= min_cells);
        if far_enough {
            chosen.push(index);
        }
    }
    chosen
}

/// For every map point, how many steps it is to the nearest river point
/// (a breadth-first search outwards from every river at once).
fn distance_to_rivers(grid: &MapGrid) -> Vec<u32> {
    let mut distance = vec![u32::MAX; grid.size * grid.size];
    let mut frontier = VecDeque::new();
    for (index, &is_river) in grid.is_river.iter().enumerate() {
        if is_river {
            distance[index] = 0;
            frontier.push_back(index);
        }
    }
    while let Some(index) = frontier.pop_front() {
        for neighbour in neighbours(index, grid.size) {
            if distance[neighbour] == u32::MAX {
                distance[neighbour] = distance[index] + 1;
                frontier.push_back(neighbour);
            }
        }
    }
    distance
}
