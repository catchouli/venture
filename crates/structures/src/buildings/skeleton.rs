//! The straight skeleton of a convex polygon (Aichholzer and Aurenhammer,
//! 1995): shrink the polygon, every edge moving inward at the same speed,
//! and trace where its corners go. They meet, edge by edge, until nothing is
//! left. A roof pitched the same on every side is exactly this: each edge's
//! *face* (the ground it sweeps) is one plane of the roof, rising with how
//! far the edge has moved in, and the traced corners are its hips and
//! ridges.
//!
//! Convex polygons only shrink by *edge events* (an edge shrinking to
//! nothing, its two corners meeting), which makes this short and sturdy;
//! concave ones also need *split events* (a corner running into another
//! edge), which this doesn't do.

use glam::Vec2;

/// A point of the skeleton: where it is, and how far in the edges had moved
/// when it formed (the roof's height there is this times the pitch's tangent).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Node {
    /// Where it is.
    pub at: Vec2,
    /// How far in the edges had moved.
    pub time: f32,
}

/// A convex polygon's straight skeleton: each edge's face, as its corners'
/// nodes in order (the edge's own two corners first, then round).
#[derive(Clone, Debug, Default)]
pub struct Skeleton {
    /// Every node (the polygon's own corners first).
    pub nodes: Vec<Node>,
    /// Each edge's face (edge `i` runs from corner `i` to the next), as
    /// indices into `nodes`, anticlockwise.
    pub faces: Vec<Vec<usize>>,
}

/// Whether a polygon (anticlockwise) is convex.
pub fn is_convex(polygon: &[Vec2]) -> bool {
    let count = polygon.len();
    count >= 3
        && (0..count).all(|i| {
            let (a, b, c) = (polygon[i], polygon[(i + 1) % count], polygon[(i + 2) % count]);
            (b - a).perp_dot(c - b) >= -1e-4
        })
}

/// A shrinking corner: where it started (a node), its velocity, and the
/// edges before and after it.
#[derive(Clone, Copy, Debug)]
struct Corner {
    node: usize,
    velocity: Vec2,
    before: usize,
    after: usize,
}

/// The straight skeleton of a convex polygon (anticlockwise). `None` if
/// it isn't convex, or is too small to have one.
pub fn skeleton(polygon: &[Vec2]) -> Option<Skeleton> {
    let count = polygon.len();
    if count < 3 || !is_convex(polygon) {
        return None;
    }
    // Each edge's inward normal.
    let inward: Vec<Vec2> =
        (0..count).map(|i| (polygon[(i + 1) % count] - polygon[i]).normalize_or(Vec2::X).perp()).collect();
    // A corner between two edges moves along its bisector, fast enough to
    // keep both edges moving in at speed 1.
    let velocity = |before: usize, after: usize| {
        let (a, b) = (inward[before], inward[after]);
        let sum = a + b;
        sum / (1.0 + a.dot(b)).max(1e-3)
    };
    let mut nodes: Vec<Node> = polygon.iter().map(|&at| Node { at, time: 0.0 }).collect();
    // Each edge's face so far: its start corner's trail, and its end corner's.
    let mut starts: Vec<Vec<usize>> = (0..count).map(|i| vec![i]).collect();
    let mut ends: Vec<Vec<usize>> = (0..count).map(|i| vec![(i + 1) % count]).collect();
    let mut faces: Vec<Option<Vec<usize>>> = vec![None; count];
    let mut corners: Vec<Corner> = (0..count)
        .map(|i| {
            let before = (i + count - 1) % count;
            Corner { node: i, velocity: velocity(before, i), before, after: i }
        })
        .collect();
    let position = |corner: &Corner, nodes: &[Node], time: f32| {
        let node = nodes[corner.node];
        node.at + corner.velocity * (time - node.time)
    };
    // A face, anticlockwise: along its edge, up its end corner's trail, (to
    // where they met,) and back down its start corner's.
    let close = |edge: usize, meet: Option<usize>, starts: &[Vec<usize>], ends: &[Vec<usize>]| {
        let mut face = vec![starts[edge][0]];
        face.extend(ends[edge].iter());
        face.extend(meet);
        face.extend(starts[edge][1..].iter().rev());
        face.dedup();
        if face.len() > 1 && face.first() == face.last() {
            face.pop();
        }
        face
    };
    while corners.len() > 2 {
        // The next edge to shrink to nothing: between corners k and k + 1.
        let n = corners.len();
        let mut next: Option<(f32, usize)> = None;
        for k in 0..n {
            let (a, b) = (corners[k], corners[(k + 1) % n]);
            let (pa, pb) = (nodes[a.node], nodes[b.node]);
            let time_now = pa.time.max(pb.time);
            let gap = position(&b, &nodes, time_now) - position(&a, &nodes, time_now);
            let way = (polygon[(a.after + 1) % count] - polygon[a.after]).normalize_or(Vec2::X);
            let closing = (a.velocity - b.velocity).dot(way);
            if closing <= 1e-6 {
                continue;
            }
            let when = time_now + gap.dot(way) / closing;
            if next.is_none_or(|(best, _)| when < best - 1e-6) {
                next = Some((when, k));
            }
        }
        let Some((time, k)) = next else { break };
        let (a, b) = (corners[k], corners[(k + 1) % n]);
        let at = (position(&a, &nodes, time) + position(&b, &nodes, time)) / 2.0;
        nodes.push(Node { at, time });
        let meet = nodes.len() - 1;
        // The edge between them is gone: its face closes at the new node.
        let gone = a.after;
        faces[gone] = Some(close(gone, Some(meet), &starts, &ends));
        ends[a.before].push(meet);
        starts[b.after].push(meet);
        let merged = Corner { node: meet, velocity: velocity(a.before, b.after), before: a.before, after: b.after };
        if n == 3 {
            // A triangle shrinks to one point: every face closes there.
            for edge in [a.before, b.after] {
                faces[edge] = Some(close(edge, Some(meet), &starts, &ends));
            }
            corners.clear();
            break;
        }
        corners[k] = merged;
        corners.remove((k + 1) % n);
    }
    // Two corners left: the faces of the two edges between them close along the ridge.
    if corners.len() == 2 {
        let (a, b) = (corners[0], corners[1]);
        for edge in [a.after, b.after] {
            if faces[edge].is_none() {
                faces[edge] = Some(close(edge, None, &starts, &ends));
            }
        }
    }
    let faces: Option<Vec<Vec<usize>>> = faces.into_iter().collect();
    Some(Skeleton { nodes, faces: faces? })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn points(list: &[(f32, f32)]) -> Vec<Vec2> {
        list.iter().map(|&(x, y)| Vec2::new(x, y)).collect()
    }

    /// A face's area (anticlockwise, positive).
    fn area(skeleton: &Skeleton, face: &[usize]) -> f32 {
        let mut total = 0.0;
        for i in 0..face.len() {
            let (a, b) = (skeleton.nodes[face[i]].at, skeleton.nodes[face[(i + 1) % face.len()]].at);
            total += a.perp_dot(b);
        }
        total / 2.0
    }

    #[test]
    fn a_rectangle_is_a_hip_roof() {
        let skeleton = skeleton(&points(&[(0.0, 0.0), (10.0, 0.0), (10.0, 4.0), (0.0, 4.0)])).unwrap();
        // Two new nodes: the ridge's ends, 2 m in (half its width), at (2, 2) and (8, 2).
        let inner: Vec<&Node> = skeleton.nodes[4..].iter().collect();
        assert_eq!(inner.len(), 2, "{:?}", skeleton.nodes);
        for node in inner {
            assert!((node.time - 2.0).abs() < 1e-3 && (node.at.y - 2.0).abs() < 1e-3);
        }
        // The faces cover it, each rising from its own edge.
        let total: f32 = skeleton.faces.iter().map(|face| area(&skeleton, face)).sum();
        assert!((total - 40.0).abs() < 0.01, "{total}: {:?}", skeleton.faces);
        assert!(skeleton.faces.iter().all(|face| area(&skeleton, face) > 0.0));
    }

    #[test]
    fn a_regular_polygon_is_a_pyramid() {
        let hexagon: Vec<Vec2> = (0..6)
            .map(|i| {
                let angle = i as f32 / 6.0 * std::f32::consts::TAU;
                Vec2::new(angle.cos(), angle.sin()) * 5.0
            })
            .collect();
        let skeleton = skeleton(&hexagon).unwrap();
        let top = skeleton
            .nodes
            .iter()
            .copied()
            .fold(Node { at: Vec2::ZERO, time: 0.0 }, |best, node| if node.time > best.time { node } else { best });
        assert!(top.at.length() < 0.05, "it peaks in the middle: {top:?}");
        assert!((top.time - 5.0 * (std::f32::consts::PI / 6.0).cos()).abs() < 0.05);
        let total: f32 = skeleton.faces.iter().map(|face| area(&skeleton, face)).sum();
        assert!((total - 64.95).abs() < 0.1, "{total}");
    }

    #[test]
    fn skewed_quadrilaterals_have_skeletons_too() {
        let quad = points(&[(0.0, 0.0), (9.0, 0.0), (11.0, 6.0), (1.0, 5.0)]);
        let skeleton = skeleton(&quad).unwrap();
        assert_eq!(skeleton.faces.len(), 4);
        let total: f32 = skeleton.faces.iter().map(|face| area(&skeleton, face)).sum();
        let expected = {
            let mut a = 0.0;
            for i in 0..4 {
                a += quad[i].perp_dot(quad[(i + 1) % 4]);
            }
            a / 2.0
        };
        assert!((total - expected).abs() < 0.05, "{total} of {expected}");
        let dent = points(&[(0.0, 0.0), (4.0, 0.0), (2.0, 1.0), (4.0, 4.0), (0.0, 4.0)]);
        assert!(super::skeleton(&dent).is_none(), "not convex");
    }
}
