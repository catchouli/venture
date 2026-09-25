//! Finding paths between points.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::f32::consts::SQRT_2;

/// A position in metres: `(x, y)` on the horizontal plane.
pub type Point = (f32, f32);

/// Neighbouring cells, with the cost of stepping to each.
const STEPS: [(i32, i32, f32); 8] = [
    (1, 0, 1.0),
    (-1, 0, 1.0),
    (0, 1, 1.0),
    (0, -1, 1.0),
    (1, 1, SQRT_2),
    (1, -1, SQRT_2),
    (-1, 1, SQRT_2),
    (-1, -1, SQRT_2),
];

/// Spacing of the checks along a straight line when smoothing a path, in metres.
const LINE_CHECK_STEP: f32 = 0.25;

/// Finds a walking route from `start` to `goal`, as waypoints ending near
/// the goal (the start itself is not included).
///
/// `is_blocked(x, y)` says whether terrain cell `(x, y)` can't be entered.
/// The search gives up after exploring `max_cells` cells. If the goal can't be
/// reached (it's blocked, or too far), the route leads to the closest point
/// found instead. If the start is boxed in, the route is empty.
pub fn find_path(start: Point, goal: Point, is_blocked: impl Fn(i32, i32) -> bool, max_cells: usize) -> Vec<Point> {
    let start_cell = cell_of(start);
    let goal_cell = cell_of(goal);
    let estimate = |(x, y): (i32, i32)| ((x - goal_cell.0) as f32).hypot((y - goal_cell.1) as f32);

    let mut cost_so_far = HashMap::from([(start_cell, 0.0_f32)]);
    let mut came_from: HashMap<(i32, i32), (i32, i32)> = HashMap::new();
    let mut queue = BinaryHeap::from([Candidate { priority: estimate(start_cell), cell: start_cell }]);
    let mut closest = start_cell;
    let mut explored = 0;

    while let Some(Candidate { cell, .. }) = queue.pop() {
        if cell == goal_cell {
            closest = cell;
            break;
        }
        if estimate(cell) < estimate(closest) {
            closest = cell;
        }
        explored += 1;
        if explored > max_cells {
            break;
        }
        for (dx, dy, step) in STEPS {
            let next = (cell.0 + dx, cell.1 + dy);
            // No cutting corners diagonally past a blocked cell.
            let diagonal = dx != 0 && dy != 0;
            if is_blocked(next.0, next.1)
                || (diagonal && (is_blocked(cell.0 + dx, cell.1) || is_blocked(cell.0, cell.1 + dy)))
            {
                continue;
            }
            let cost = cost_so_far[&cell] + step;
            if cost_so_far.get(&next).is_none_or(|&known| cost < known) {
                cost_so_far.insert(next, cost);
                came_from.insert(next, cell);
                queue.push(Candidate { priority: cost + estimate(next), cell: next });
            }
        }
    }
    // Walk back from the end to the start.
    let mut cells = vec![closest];
    while let Some(&previous) = came_from.get(cells.last().expect("cells is not empty")) {
        cells.push(previous);
    }
    cells.reverse();

    // Aim for the exact goal point if it was reached, otherwise cell centres.
    let mut points: Vec<Point> = cells.iter().map(|&(x, y)| (x as f32 + 0.5, y as f32 + 0.5)).collect();
    points[0] = start;
    if closest == goal_cell {
        *points.last_mut().expect("points is not empty") = goal;
    }
    let smoothed = smooth(&points, &is_blocked);
    smoothed.into_iter().skip(1).collect()
}

/// Removes every waypoint that the path can skip with a clear straight line
/// (the "string pulling" technique).
fn smooth(points: &[Point], is_blocked: &impl Fn(i32, i32) -> bool) -> Vec<Point> {
    let Some(&first) = points.first() else { return Vec::new() };
    let mut smoothed = vec![first];
    let mut anchor = 0;
    while anchor < points.len() - 1 {
        // The furthest point still visible from the anchor.
        let furthest = (anchor + 1..points.len())
            .rev()
            .find(|&i| line_is_clear(points[anchor], points[i], is_blocked))
            .unwrap_or(anchor + 1);
        smoothed.push(points[furthest]);
        anchor = furthest;
    }
    smoothed
}

/// Whether the straight line from `a` to `b` crosses no blocked cell.
fn line_is_clear(a: Point, b: Point, is_blocked: &impl Fn(i32, i32) -> bool) -> bool {
    let length = (b.0 - a.0).hypot(b.1 - a.1);
    let steps = (length / LINE_CHECK_STEP).ceil().max(1.0) as usize;
    (0..=steps).all(|i| {
        let t = i as f32 / steps as f32;
        let (x, y) = cell_of((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
        !is_blocked(x, y)
    })
}

fn cell_of((x, y): Point) -> (i32, i32) {
    (x.floor() as i32, y.floor() as i32)
}

/// A cell waiting to be explored. Lowest priority comes out first; ties are
/// broken by position so results never depend on heap internals.
#[derive(PartialEq)]
struct Candidate {
    priority: f32,
    cell: (i32, i32),
}

impl Eq for Candidate {}

impl Ord for Candidate {
    fn cmp(&self, other: &Self) -> Ordering {
        other.priority.total_cmp(&self.priority).then_with(|| other.cell.cmp(&self.cell))
    }
}

impl PartialOrd for Candidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_ground_gives_a_straight_line() {
        let path = find_path((0.5, 0.5), (10.5, 3.5), |_, _| false, 10_000);
        assert_eq!(path, vec![(10.5, 3.5)]);
    }

    #[test]
    fn walls_are_walked_around() {
        // A wall at x = 5 from y = -10 to y = 10.
        let wall = |x: i32, y: i32| x == 5 && (-10..=10).contains(&y);
        let path = find_path((0.5, 0.5), (10.5, 0.5), wall, 10_000);
        assert_eq!(*path.last().unwrap(), (10.5, 0.5));
        assert!(path.len() > 1, "the path must bend around the wall");
        let mut from = (0.5, 0.5);
        for &to in &path {
            assert!(line_is_clear(from, to, &wall));
            from = to;
        }
    }

    #[test]
    fn unreachable_goals_lead_as_close_as_possible() {
        // The goal is inside a closed box.
        let boxed =
            |x: i32, y: i32| (8..=12).contains(&x) && (8..=12).contains(&y) && (x == 8 || x == 12 || y == 8 || y == 12);
        let path = find_path((0.5, 0.5), (10.5, 10.5), boxed, 10_000);
        let (x, y) = *path.last().unwrap();
        assert!((x - 10.5).hypot(y - 10.5) < 4.0);
    }
}
