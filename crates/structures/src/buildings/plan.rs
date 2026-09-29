//! A building's plan, worked out: its own frame (where every edge lies
//! along an axis), its footprint in that frame, the rectangles (*wings*) its
//! roof covers, the inside of its walls, and where its doors and windows go.

use glam::Vec2;
use venture_world::{BuildingPlan, WallRef};

use super::style::BuildingStyle;
use crate::random::Random;

/// Corners are right angles if they're within this (the cosine of the
/// angle's difference from one).
const SQUARE: f32 = 0.05;
/// The sharpest a corner can be (its inside angle), in radians.
const SHARPEST: f32 = 0.6;
/// The shortest an outer wall can be, in metres.
pub const SHORTEST_WALL: f32 = 1.5;
/// The widest a building can be, in metres.
pub const WIDEST: f32 = 40.0;
/// The most floors a building can have.
pub const MOST_FLOORS: u32 = 4;

/// A building's own frame: its first corner, and the directions along its
/// first wall (x) and across it (y, into the building for an anticlockwise
/// outline). A square building's every wall lies along one of its axes.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    /// Where its origin is, in the world (metres).
    pub origin: Vec2,
    /// The way its x runs, in the world.
    pub x: Vec2,
    /// The way its y runs, in the world.
    pub y: Vec2,
}

impl Frame {
    /// A world point in the frame.
    pub fn local(&self, world: Vec2) -> Vec2 {
        let d = world - self.origin;
        Vec2::new(d.dot(self.x), d.dot(self.y))
    }

    /// A point in the frame, in the world.
    pub fn world(&self, local: Vec2) -> Vec2 {
        self.origin + self.x * local.x + self.y * local.y
    }

    /// A point of the frame `height` up, relative to its origin, in the world's directions (x, up, y).
    pub fn point(&self, at: Vec2, height: f32) -> glam::Vec3 {
        let flat = self.x * at.x + self.y * at.y;
        glam::Vec3::new(flat.x, height, flat.y)
    }

    /// A direction in the frame, in the world's (level).
    pub fn direction(&self, way: Vec2) -> glam::Vec3 {
        self.point(way, 0.0)
    }
}

/// A plan in its own frame.
#[derive(Clone, Debug)]
pub struct Footprint {
    /// Its frame.
    pub frame: Frame,
    /// Its outline (the outside of its walls), anticlockwise, every edge along an axis.
    pub outline: Vec<Vec2>,
    /// The inside of its outer walls, corner for corner.
    pub inside: Vec<Vec2>,
    /// Whether every corner is a right angle (so its roof can be gables
    /// and hips over rectangular wings).
    pub square: bool,
}

impl Footprint {
    /// Works a plan out in its own frame (with its outline squared up and
    /// turned anticlockwise), and the inside of walls `thickness` thick.
    /// `None` if it has fewer than four corners.
    pub fn of(plan: &BuildingPlan, thickness: f32) -> Option<Self> {
        let mut points: Vec<Vec2> = plan.outline.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
        if points.len() < 3 {
            return None;
        }
        if signed_area(&points) < 0.0 {
            points.reverse();
        }
        let x = (points[1] - points[0]).normalize_or(Vec2::X);
        let frame = Frame { origin: points[0], x, y: x.perp() };
        let mut outline: Vec<Vec2> = points.iter().map(|&point| frame.local(point)).collect();
        let square = is_square(&points);
        // Square it up (if it's meant to be): each edge along whichever axis it's nearer.
        for i in 0..if square { outline.len() } else { 0 } {
            let (a, b) = (outline[i], outline[(i + 1) % outline.len()]);
            let along_x = (b - a).x.abs() >= (b - a).y.abs();
            let next = (i + 1) % outline.len();
            if along_x {
                outline[next].y = a.y;
            } else {
                outline[next].x = a.x;
            }
        }
        let inside = inset(&outline, thickness);
        Some(Self { frame, outline, inside, square })
    }

    /// Its outer wall `i`: from corner `i` to the next.
    pub fn edge(&self, i: usize) -> (Vec2, Vec2) {
        (self.outline[i], self.outline[(i + 1) % self.outline.len()])
    }

    /// Whether a point (in the frame) is inside its outline.
    pub fn contains(&self, point: Vec2) -> bool {
        contains(&self.outline, point)
    }
}

/// The signed area of a polygon (positive when anticlockwise).
pub fn signed_area(points: &[Vec2]) -> f32 {
    let mut area = 0.0;
    for i in 0..points.len() {
        let (a, b) = (points[i], points[(i + 1) % points.len()]);
        area += a.x * b.y - b.x * a.y;
    }
    area / 2.0
}

/// A plan with its outline running anticlockwise (as the generator wants),
/// its doors moved with their walls if it had to be turned round.
pub fn anticlockwise(plan: &BuildingPlan) -> BuildingPlan {
    let points: Vec<Vec2> = plan.outline.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
    if points.len() < 3 || signed_area(&points) >= 0.0 {
        return plan.clone();
    }
    let count = plan.outline.len();
    let mut turned = plan.clone();
    turned.outline.reverse();
    // Wall i ran from corner i to i + 1; reversed, corner i is at count - 1 - i,
    // so that wall now runs (backwards) from count - 2 - i.
    for door in &mut turned.doors {
        if let WallRef::Outer(wall) = &mut door.wall {
            *wall = ((2 * count - 2 - *wall as usize) % count) as u32;
            door.along = 1.0 - door.along;
        }
    }
    turned
}

/// Whether a point is inside a polygon (even-odd).
pub fn contains(polygon: &[Vec2], point: Vec2) -> bool {
    let mut inside = false;
    for i in 0..polygon.len() {
        let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
        if (a.y > point.y) != (b.y > point.y) {
            let x = a.x + (point.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if point.x < x {
                inside = !inside;
            }
        }
    }
    inside
}

/// An anticlockwise polygon moved in by `by` (every edge moved in by it,
/// each corner mitred to where its edges meet; very sharp corners are kept
/// from reaching too far).
pub fn inset(outline: &[Vec2], by: f32) -> Vec<Vec2> {
    let count = outline.len();
    (0..count)
        .map(|i| {
            let before = outline[(i + count - 1) % count];
            let (here, after) = (outline[i], outline[(i + 1) % count]);
            let inward = |a: Vec2, b: Vec2| (b - a).normalize_or(Vec2::X).perp();
            let (a, b) = (inward(before, here), inward(here, after));
            here + (a + b) / (1.0 + a.dot(b)).max(0.25) * by
        })
        .collect()
}

/// Whether every corner of a polygon is (near enough) a right angle.
pub fn is_square(points: &[Vec2]) -> bool {
    let count = points.len();
    count >= 4
        && count.is_multiple_of(2)
        && (0..count).all(|i| {
            let (a, b, c) = (points[i], points[(i + 1) % count], points[(i + 2) % count]);
            (b - a).normalize_or(Vec2::X).dot((c - b).normalize_or(Vec2::X)).abs() <= SQUARE
        })
}

/// Splits a simple polygon (anticlockwise) into triangles, by clipping
/// off its ears, as corner indices.
pub fn triangulate(polygon: &[Vec2]) -> Vec<[usize; 3]> {
    let mut left: Vec<usize> = (0..polygon.len()).collect();
    let mut triangles = Vec::new();
    let mut guard = 0;
    while left.len() > 3 && guard < 10_000 {
        guard += 1;
        let n = left.len();
        let ear = (0..n).find(|&k| {
            let (a, b, c) = (polygon[left[(k + n - 1) % n]], polygon[left[k]], polygon[left[(k + 1) % n]]);
            if (b - a).perp_dot(c - b) <= 1e-6 {
                return false;
            }
            // No other corner inside it.
            left.iter().all(|&other| {
                let p = polygon[other];
                if p == a || p == b || p == c {
                    return true;
                }
                let inside =
                    (b - a).perp_dot(p - a) > 0.0 && (c - b).perp_dot(p - b) > 0.0 && (a - c).perp_dot(p - c) > 0.0;
                !inside
            })
        });
        let k = ear.unwrap_or(0);
        triangles.push([left[(k + n - 1) % n], left[k], left[(k + 1) % n]]);
        left.remove(k);
    }
    if left.len() == 3 {
        triangles.push([left[0], left[1], left[2]]);
    }
    triangles
}

/// An inner wall's end (in the frame) near an outer wall, moved onto that
/// wall's inside (and a little into it, so no gap shows).
pub fn snap_inner(footprint: &Footprint, style: &BuildingStyle, point: Vec2) -> Vec2 {
    let inside = &footprint.inside;
    let reach = style.wall_thickness + 0.6;
    let mut best: Option<(f32, Vec2, Vec2)> = None;
    for i in 0..inside.len() {
        let (a, b) = (inside[i], inside[(i + 1) % inside.len()]);
        let run = b - a;
        let t = ((point - a).dot(run) / run.length_squared().max(1e-6)).clamp(0.0, 1.0);
        let nearest = a + run * t;
        let distance = nearest.distance(point);
        if best.is_none_or(|(d, _, _)| distance < d) {
            best = Some((distance, nearest, -run.normalize_or(Vec2::X).perp()));
        }
    }
    match best {
        Some((distance, nearest, outward)) if distance < reach => nearest + outward * 0.05,
        _ => point,
    }
}

/// Problems with a plan (the empty list if it's fine), given its style.
pub fn check_plan(plan: &BuildingPlan, style: Option<&BuildingStyle>) -> Vec<String> {
    let mut problems = Vec::new();
    let name = format!("building \"{}\"", plan.id);
    let Some(style) = style.map(BuildingStyle::scaled) else {
        return vec![format!("{name}: there's no style \"{}\"", plan.style)];
    };
    let style = &style;
    let points: Vec<Vec2> = plan.outline.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
    if points.len() < 3 {
        problems.push(format!("{name} needs at least three corners"));
        return problems;
    }
    let count = points.len();
    for i in 0..count {
        let (a, b, c) = (points[i], points[(i + 1) % count], points[(i + 2) % count]);
        if a.distance(b) < SHORTEST_WALL {
            problems.push(format!("{name}: its wall {} is shorter than {SHORTEST_WALL} m", i + 1));
        }
        // (A corner's inside angle can't be sharper than this, or its walls would crowd each other.)
        let turn = (b - a).normalize_or(Vec2::X).dot((c - b).normalize_or(Vec2::X));
        if turn < -SHARPEST.cos() {
            problems.push(format!("{name}: its corner {} is too sharp", (i + 1) % count + 1));
        }
    }
    for i in 0..count {
        for j in i + 2..count {
            if i == 0 && j == count - 1 {
                continue;
            }
            let (a, b) = (points[i], points[(i + 1) % count]);
            let (c, d) = (points[j], points[(j + 1) % count]);
            if segments_cross(a, b, c, d) {
                problems.push(format!("{name}: its walls {} and {} cross", i + 1, j + 1));
            }
        }
    }
    let (low, high) = points.iter().fold((points[0], points[0]), |(low, high), &p| (low.min(p), high.max(p)));
    if (high - low).max_element() > WIDEST * std::f32::consts::SQRT_2 {
        problems.push(format!("{name} is more than {WIDEST} m across"));
    }
    if !(1..=MOST_FLOORS).contains(&plan.floors) {
        problems.push(format!("{name} must have from 1 to {MOST_FLOORS} floors"));
    }
    if let Some(footprint) = Footprint::of(plan, style.wall_thickness) {
        for (i, &(from, to)) in plan.inner_walls.iter().enumerate() {
            let (from, to) = (footprint.frame.local(Vec2::from(from)), footprint.frame.local(Vec2::from(to)));
            if !footprint.contains((from + to) / 2.0) || from.distance(to) < 0.5 {
                problems.push(format!("{name}: its inner wall {} isn't inside it", i + 1));
            }
        }
    }
    for (i, door) in plan.doors.iter().enumerate() {
        let exists = match door.wall {
            WallRef::Outer(wall) => (wall as usize) < count,
            WallRef::Inner(wall) => (wall as usize) < plan.inner_walls.len(),
        };
        if !exists || !(0.0..=1.0).contains(&door.along) {
            problems.push(format!("{name}: its door {} isn't on one of its walls", i + 1));
        }
    }
    problems
}

/// Whether two segments cross (touching ends don't count).
fn segments_cross(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let side = |p: Vec2, q: Vec2, r: Vec2| (q - p).perp_dot(r - p);
    let (d1, d2) = (side(c, d, a), side(c, d, b));
    let (d3, d4) = (side(a, b, c), side(a, b, d));
    d1 * d2 < -1e-4 && d3 * d4 < -1e-4
}

/// A rectangle in a building's frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// Its least corner.
    pub low: Vec2,
    /// Its greatest corner.
    pub high: Vec2,
}

impl Rect {
    /// Its size.
    pub fn size(&self) -> Vec2 {
        self.high - self.low
    }

    /// Its area.
    pub fn area(&self) -> f32 {
        self.size().x * self.size().y
    }
}

/// Splits a square-cornered polygon into rectangles: sliced across one
/// axis at every corner, with neighbouring slices that are alike joined.
/// Both ways are tried; the one with fewer rectangles (then the biggest
/// one) wins, so an L is two wings and a T three.
pub fn wings(polygon: &[Vec2]) -> Vec<Rect> {
    let across_x = slice(polygon, false);
    let across_y = slice(polygon, true);
    let biggest = |rects: &[Rect]| rects.iter().map(Rect::area).fold(0.0, f32::max);
    if across_x.len() != across_y.len() {
        return if across_x.len() < across_y.len() { across_x } else { across_y };
    }
    if biggest(&across_x) >= biggest(&across_y) { across_x } else { across_y }
}

/// Slices a polygon at every corner's x (or y, if `swap`), and joins alike neighbours.
fn slice(polygon: &[Vec2], swap: bool) -> Vec<Rect> {
    let flip = |p: Vec2| if swap { Vec2::new(p.y, p.x) } else { p };
    let points: Vec<Vec2> = polygon.iter().map(|&p| flip(p)).collect();
    let mut cuts: Vec<f32> = points.iter().map(|p| p.x).collect();
    cuts.sort_by(f32::total_cmp);
    cuts.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
    // Each slice's spans across it (inside the polygon), and the rectangles they grow into.
    let mut open: Vec<(f32, f32, f32)> = Vec::new(); // (from x, low y, high y)
    let mut rects = Vec::new();
    for pair in cuts.windows(2) {
        let middle = (pair[0] + pair[1]) / 2.0;
        let mut crossings: Vec<f32> = Vec::new();
        for i in 0..points.len() {
            let (a, b) = (points[i], points[(i + 1) % points.len()]);
            if (a.x - b.x).abs() > 1e-4 && (a.x < middle) != (b.x < middle) {
                crossings.push(a.y);
            }
        }
        crossings.sort_by(f32::total_cmp);
        let spans: Vec<(f32, f32)> = crossings.chunks(2).filter(|c| c.len() == 2).map(|c| (c[0], c[1])).collect();
        // Spans carried on from the last slice keep growing; others close.
        let mut still = Vec::new();
        for &(from, low, high) in &open {
            if spans.iter().any(|&(l, h)| (l - low).abs() < 1e-3 && (h - high).abs() < 1e-3) {
                still.push((from, low, high));
            } else {
                rects.push((from, pair[0], low, high));
            }
        }
        for &(low, high) in &spans {
            if !still.iter().any(|&(_, l, h)| (l - low).abs() < 1e-3 && (h - high).abs() < 1e-3) {
                still.push((pair[0], low, high));
            }
        }
        open = still;
    }
    let end = *cuts.last().unwrap_or(&0.0);
    rects.extend(open.into_iter().map(|(from, low, high)| (from, end, low, high)));
    rects
        .into_iter()
        .map(|(x0, x1, y0, y1)| {
            let (a, b) = (flip(Vec2::new(x0, y0)), flip(Vec2::new(x1, y1)));
            Rect { low: a.min(b), high: a.max(b) }
        })
        .collect()
}

/// What an opening is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpeningKind {
    /// A doorway.
    Door,
    /// A window.
    Window,
}

/// An opening in a wall: along it (from its start, in metres) and up (above the ground floor, in metres).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Opening {
    /// What it is.
    pub kind: OpeningKind,
    /// Where it starts and ends along the wall.
    pub from: f32,
    /// See `from`.
    pub to: f32,
    /// Its bottom and top, above the ground floor.
    pub bottom: f32,
    /// See `bottom`.
    pub top: f32,
}

impl Opening {
    /// Whether it's in the way between `low` and `high` up the wall.
    pub fn spans(&self, low: f32, high: f32) -> bool {
        self.top > low && self.bottom < high
    }
}

/// Where the doors go in a wall `length` long: each door's middle `along`
/// (0 to 1), kept inside the wall.
pub fn doors_in(length: f32, alongs: &[f32], style: &BuildingStyle) -> Vec<Opening> {
    let (width, height) = style.door.size;
    let margin = 0.25 + width / 2.0;
    alongs
        .iter()
        .map(|&along| {
            let middle = (along * length).clamp(margin.min(length / 2.0), (length - margin).max(length / 2.0));
            Opening {
                kind: OpeningKind::Door,
                from: middle - width / 2.0,
                to: middle + width / 2.0,
                bottom: 0.0,
                top: height,
            }
        })
        .collect()
}

/// Where windows go on an outer wall `length` long, on floor `storey`
/// (0 is the ground floor), keeping clear of its ends and of `keep_clear`
/// (doors, and where inner walls meet it: along the wall, from and to).
pub fn windows_in(
    length: f32,
    storey: u32,
    keep_clear: &[(f32, f32)],
    style: &BuildingStyle,
    random: &mut Random,
) -> Vec<Opening> {
    let window = &style.window;
    let (width, height) = window.size;
    let bottom = storey as f32 * style.storey + window.sill;
    // The stretches of wall with room for windows.
    let mut free = vec![(window.corner, length - window.corner)];
    for &(from, to) in keep_clear {
        let (from, to) = (from - window.gap / 2.0, to + window.gap / 2.0);
        free = free
            .into_iter()
            .flat_map(|(a, b)| if to <= a || from >= b { vec![(a, b)] } else { vec![(a, from), (to, b)] })
            .filter(|(a, b)| b - a >= width)
            .collect();
    }
    let mut windows = Vec::new();
    for (a, b) in free {
        // As many as fit, spread evenly; each place may go without.
        let fits = ((b - a + window.gap) / (width + window.gap)).floor().max(0.0) as usize;
        let count = if fits > 2 { fits - usize::from(random.unit() < 0.4) } else { fits };
        for k in 0..count {
            if random.unit() > window.chance && count > 1 {
                continue;
            }
            let middle = a + (b - a) * (k as f32 + 0.5) / count as f32;
            windows.push(Opening {
                kind: OpeningKind::Window,
                from: middle - width / 2.0,
                to: middle + width / 2.0,
                bottom,
                top: bottom + height,
            });
        }
    }
    windows
}

#[cfg(test)]
mod tests {
    use super::*;
    use venture_world::{DoorPlan, RoofShape};

    pub fn plan(outline: &[(f32, f32)]) -> BuildingPlan {
        BuildingPlan {
            id: "b".into(),
            style: "s".into(),
            floors: 1,
            seed: 1,
            roof: RoofShape::Gable,
            chimney: None,
            outline: outline.to_vec(),
            inner_walls: Vec::new(),
            doors: vec![DoorPlan { wall: WallRef::Outer(0), along: 0.5 }],
        }
    }

    fn style() -> BuildingStyle {
        super::super::style::parse_styles(r#"[(id: "s", name: "S", walls: Stone)]"#).unwrap().remove(0)
    }

    const L: [(f32, f32); 6] = [(0.0, 0.0), (10.0, 0.0), (10.0, 4.0), (4.0, 4.0), (4.0, 9.0), (0.0, 9.0)];

    #[test]
    fn clockwise_plans_are_turned_round_with_their_doors() {
        let mut clockwise = plan(&[(0.0, 0.0), (0.0, 5.0), (8.0, 5.0), (8.0, 0.0)]);
        // A door 30% along the long wall from (0, 5) to (8, 5): at (2.4, 5).
        clockwise.doors = vec![venture_world::DoorPlan { wall: WallRef::Outer(1), along: 0.3 }];
        let turned = anticlockwise(&clockwise);
        let WallRef::Outer(wall) = turned.doors[0].wall else { panic!() };
        let (a, b) = (turned.outline[wall as usize], turned.outline[(wall as usize + 1) % 4]);
        let at = Vec2::from(a).lerp(Vec2::from(b), turned.doors[0].along);
        assert!(at.distance(Vec2::new(2.4, 5.0)) < 1e-4, "{at}");
    }

    #[test]
    fn plans_are_checked() {
        assert!(check_plan(&plan(&L), Some(&style())).is_empty(), "{:?}", check_plan(&plan(&L), Some(&style())));
        let skew = plan(&[(0.0, 0.0), (8.0, 0.0), (9.0, 5.0), (0.0, 5.0)]);
        assert!(check_plan(&skew, Some(&style())).is_empty(), "free angles are fine");
        let sharp = plan(&[(0.0, 0.0), (10.0, 0.0), (0.0, 1.5)]);
        assert!(!check_plan(&sharp, Some(&style())).is_empty(), "a corner too sharp");
        assert!(!check_plan(&plan(&L), None).is_empty(), "no style");
    }

    #[test]
    fn footprints_are_squared_up_in_their_own_frame_and_turned_anticlockwise() {
        // Turned 30 degrees, and clockwise.
        let (sin, cos) = 30f32.to_radians().sin_cos();
        let turn = |(x, y): (f32, f32)| (x * cos - y * sin + 100.0, x * sin + y * cos + 50.0);
        let mut outline: Vec<(f32, f32)> = L.iter().map(|&p| turn(p)).collect();
        outline.reverse();
        let footprint = Footprint::of(&plan(&outline), 0.5).unwrap();
        assert!(signed_area(&footprint.outline) > 0.0);
        assert!((signed_area(&footprint.outline) - (10.0 * 4.0 + 4.0 * 5.0)).abs() < 0.01);
        for i in 0..footprint.outline.len() {
            let (a, b) = footprint.edge(i);
            assert!((a.x - b.x).abs() < 1e-3 || (a.y - b.y).abs() < 1e-3, "edges lie along its axes");
        }
        // The inside is smaller all round.
        let inside = signed_area(&footprint.inside);
        // (60 m², less about half a metre round its 38 m outline.)
        assert!(inside < 45.0 && inside > 38.0, "{inside}");
    }

    #[test]
    fn polygons_are_triangulated_and_inset_at_any_angle() {
        let shape = [(0.0, 0.0), (8.0, 0.0), (10.0, 5.0), (5.0, 3.0), (1.0, 6.0)].map(|(x, y)| Vec2::new(x, y));
        let triangles = triangulate(&shape);
        assert_eq!(triangles.len(), 3);
        let area: f32 =
            triangles.iter().map(|&[a, b, c]| (shape[b] - shape[a]).perp_dot(shape[c] - shape[a]) / 2.0).sum();
        assert!((area - signed_area(&shape)).abs() < 1e-3, "{area}");
        // Each inset edge is 0.5 m in from its own.
        let inside = inset(&shape, 0.5);
        for i in 0..shape.len() {
            let (a, b) = (shape[i], shape[(i + 1) % shape.len()]);
            let (c, d) = (inside[i], inside[(i + 1) % shape.len()]);
            let normal = (b - a).normalize().perp();
            assert!(((c - a).dot(normal) - 0.5).abs() < 1e-3 && ((d - a).dot(normal) - 0.5).abs() < 1e-3);
        }
    }

    #[test]
    fn an_l_is_two_wings_and_a_t_three() {
        let points = |outline: &[(f32, f32)]| outline.iter().map(|&(x, y)| Vec2::new(x, y)).collect::<Vec<_>>();
        let l = wings(&points(&L));
        assert_eq!(l.len(), 2, "{l:?}");
        assert!((l.iter().map(Rect::area).sum::<f32>() - 60.0).abs() < 0.01);
        let t = [(0.0, 0.0), (12.0, 0.0), (12.0, 4.0), (8.0, 4.0), (8.0, 9.0), (4.0, 9.0), (4.0, 4.0), (0.0, 4.0)];
        let t = wings(&points(&t));
        assert!(t.len() <= 3, "{t:?}");
        assert!((t.iter().map(Rect::area).sum::<f32>() - 68.0).abs() < 0.01);
    }

    #[test]
    fn windows_keep_clear_of_corners_and_doors() {
        let mut random = Random::new(3);
        let style = style();
        let door = doors_in(10.0, &[0.5], &style)[0];
        let windows = windows_in(10.0, 0, &[(door.from, door.to)], &style, &mut random);
        assert!(!windows.is_empty());
        for window in &windows {
            assert!(window.from >= style.window.corner - 1e-3 && window.to <= 10.0 - style.window.corner + 1e-3);
            assert!(window.to <= door.from - 0.5 || window.from >= door.to + 0.5, "{window:?} by {door:?}");
        }
    }
}
