//! Distance to the nearest "source" cell for every cell of a grid.
//!
//! Uses the classic two-pass chamfer distance transform: one pass from the
//! top-left and one from the bottom-right, each looking at the already-visited
//! neighbours. Straight steps cost 1 and diagonal steps √2, which gives a
//! close approximation of true Euclidean distance in linear time.

use std::f32::consts::SQRT_2;

/// The nearest source to a cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Nearest {
    /// Approximate distance to it, in cells.
    pub distance: f32,
    /// The source cell's index.
    pub source: usize,
}

/// For each cell of a `size` × `size` grid (stored row by row), the nearest
/// cell for which `is_source` is true.
pub fn nearest_sources(size: usize, is_source: &[bool]) -> Vec<Option<Nearest>> {
    assert_eq!(is_source.len(), size * size);
    let mut field: Vec<Option<Nearest>> = is_source
        .iter()
        .enumerate()
        .map(|(index, &is_source)| is_source.then_some(Nearest { distance: 0.0, source: index }))
        .collect();

    // Neighbours already visited by each pass, with the cost of stepping from them.
    let forward = [(-1, -1, SQRT_2), (0, -1, 1.0), (1, -1, SQRT_2), (-1, 0, 1.0)];
    let backward = [(1, 1, SQRT_2), (0, 1, 1.0), (-1, 1, SQRT_2), (1, 0, 1.0)];

    let mut relax = |x: usize, y: usize, steps: &[(isize, isize, f32)]| {
        for &(dx, dy, cost) in steps {
            let (nx, ny) = (x as isize + dx, y as isize + dy);
            if nx < 0 || ny < 0 || nx >= size as isize || ny >= size as isize {
                continue;
            }
            let Some(neighbour) = field[ny as usize * size + nx as usize] else { continue };
            let candidate = Nearest { distance: neighbour.distance + cost, source: neighbour.source };
            let cell = &mut field[y * size + x];
            if cell.is_none_or(|current| candidate.distance < current.distance) {
                *cell = Some(candidate);
            }
        }
    };

    for y in 0..size {
        for x in 0..size {
            relax(x, y, &forward);
        }
    }
    for y in (0..size).rev() {
        for x in (0..size).rev() {
            relax(x, y, &backward);
        }
    }
    field
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances_grow_away_from_a_source() {
        let size = 5;
        let mut sources = vec![false; size * size];
        sources[0] = true;
        let field = nearest_sources(size, &sources);
        assert_eq!(field[0].unwrap().distance, 0.0);
        assert_eq!(field[4].unwrap().distance, 4.0);
        assert!((field[size * 4 + 4].unwrap().distance - 4.0 * SQRT_2).abs() < 1e-5);
        assert!(field.iter().all(|cell| cell.unwrap().source == 0));
    }

    #[test]
    fn no_sources_means_no_distances() {
        assert!(nearest_sources(3, &[false; 9]).iter().all(Option::is_none));
    }
}
