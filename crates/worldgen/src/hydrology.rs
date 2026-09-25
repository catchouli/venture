//! Where water goes: drainage, rivers and lakes on a square height grid.
//!
//! Uses two standard terrain-analysis algorithms:
//! - **Priority-Flood** (Barnes, Lehman & Mulla, 2014) fills every depression
//!   up to its spill height, starting from the grid's edges (the sea). The
//!   neighbour a cell is first reached from becomes its downstream neighbour,
//!   which gives every cell a path to the sea that never goes uphill (D8).
//! - **Flow accumulation** counts how many cells drain through each cell.
//!   Cells with a large count carry rivers.

use std::collections::{BinaryHeap, VecDeque};

use crate::grid::{MinEntry, neighbours};

/// The result of draining a height grid.
pub struct Drainage {
    /// Heights with every depression filled to its spill height.
    pub filled: Vec<f32>,
    /// For each cell, the neighbour its water flows into. `None` on the grid edge.
    pub downstream: Vec<Option<usize>>,
    /// For each cell, how many cells (including itself) drain through it.
    pub flow: Vec<u32>,
}

/// Computes drainage for a `size` × `size` grid of heights stored row by row.
pub fn drain(heights: &[f32], size: usize) -> Drainage {
    assert_eq!(heights.len(), size * size);
    let mut filled = heights.to_vec();
    let mut downstream = vec![None; heights.len()];
    let mut visited = vec![false; heights.len()];
    let mut order = Vec::with_capacity(heights.len());
    let mut queue = BinaryHeap::new();

    // The grid's edges are the sea: water leaves the world there.
    for index in 0..heights.len() {
        let (x, y) = (index % size, index / size);
        if x == 0 || y == 0 || x == size - 1 || y == size - 1 {
            visited[index] = true;
            queue.push(MinEntry { key: filled[index], index });
        }
    }

    // Always expand the lowest known cell. Any unvisited neighbour lower than
    // it is in a depression, and is raised to the current level.
    while let Some(MinEntry { key: height, index }) = queue.pop() {
        order.push(index);
        for neighbour in neighbours(index, size) {
            if visited[neighbour] {
                continue;
            }
            visited[neighbour] = true;
            filled[neighbour] = filled[neighbour].max(height);
            downstream[neighbour] = Some(index);
            queue.push(MinEntry { key: filled[neighbour], index: neighbour });
        }
    }

    // Cells came out of the queue lowest first, so walking that order backwards
    // visits every cell before the cell it drains into.
    let mut flow = vec![1; heights.len()];
    for &index in order.iter().rev() {
        if let Some(next) = downstream[index] {
            flow[next] += flow[index];
        }
    }

    Drainage { filled, downstream, flow }
}

/// Finds lakes: filled depressions at least `min_depth` deep somewhere and
/// covering at least `min_cells` cells. Cells below `sea_level` are sea, not
/// lake. Returns each cell's lake number (`0, 1, 2, ...`), if it has one.
pub fn find_lakes(
    heights: &[f32],
    filled: &[f32],
    size: usize,
    sea_level: f32,
    min_depth: f32,
    min_cells: usize,
) -> Vec<Option<usize>> {
    let is_flooded = |i: usize| filled[i] > heights[i] && filled[i] >= sea_level;
    let mut lakes = vec![None; heights.len()];
    let mut seen = vec![false; heights.len()];
    let mut lake_count = 0;

    for start in 0..heights.len() {
        if seen[start] || !is_flooded(start) {
            continue;
        }
        // Collect this connected basin, which all shares one water level.
        let mut basin = Vec::new();
        let mut deepest = 0.0_f32;
        let mut frontier = VecDeque::from([start]);
        seen[start] = true;
        while let Some(index) = frontier.pop_front() {
            basin.push(index);
            deepest = deepest.max(filled[index] - heights[index]);
            for neighbour in neighbours(index, size) {
                if !seen[neighbour] && is_flooded(neighbour) && filled[neighbour] == filled[index] {
                    seen[neighbour] = true;
                    frontier.push_back(neighbour);
                }
            }
        }
        if basin.len() >= min_cells && deepest >= min_depth {
            for index in basin {
                lakes[index] = Some(lake_count);
            }
            lake_count += 1;
        }
    }
    lakes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 5×5 bowl: high rim, a pit in the middle, and one low gap in the rim.
    fn bowl() -> Vec<f32> {
        #[rustfmt::skip]
        let heights = vec![
            5.0, 5.0, 5.0, 5.0, 5.0,
            5.0, 4.0, 4.0, 4.0, 5.0,
            5.0, 4.0, 1.0, 4.0, 2.0,
            5.0, 4.0, 4.0, 4.0, 5.0,
            5.0, 5.0, 5.0, 5.0, 5.0,
        ];
        heights
    }

    #[test]
    fn depressions_fill_to_spill_height() {
        let drainage = drain(&bowl(), 5);
        // The pit spills over its lowest neighbour (4.0).
        assert_eq!(drainage.filled[12], 4.0);
    }

    #[test]
    fn water_never_flows_uphill() {
        let drainage = drain(&bowl(), 5);
        for (index, next) in drainage.downstream.iter().enumerate() {
            if let Some(next) = *next {
                assert!(drainage.filled[next] <= drainage.filled[index]);
            }
        }
    }

    #[test]
    fn all_flow_reaches_the_edge() {
        let size = 5;
        let drainage = drain(&bowl(), size);
        let edge_flow: u32 = drainage
            .downstream
            .iter()
            .zip(&drainage.flow)
            .filter(|(next, _)| next.is_none())
            .map(|(_, flow)| flow)
            .sum();
        assert_eq!(edge_flow as usize, size * size);
    }

    #[test]
    fn lakes_respect_minimum_size() {
        let heights = bowl();
        let drainage = drain(&heights, 5);
        assert_eq!(find_lakes(&heights, &drainage.filled, 5, 0.0, 0.5, 1)[12], Some(0));
        assert_eq!(find_lakes(&heights, &drainage.filled, 5, 0.0, 0.5, 2)[12], None);
    }
}
