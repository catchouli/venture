//! A drawn path: a smooth curve through the points an author clicked, and
//! even samples along it.
//!
//! The curve is a centripetal Catmull–Rom spline (Yuksel, Schaefer and
//! Keyser, "Parameterization and Applications of Catmull–Rom Curves",
//! 2011). It passes through every point, and unlike the uniform kind it
//! never loops or overshoots at tight bends or unevenly spaced points. A
//! closed path wraps round from its last point to its first.

use glam::Vec2;

/// Segments of the curve are measured with this many straight steps each,
/// for sampling it evenly by length.
const STEPS_PER_SEGMENT: usize = 24;

/// A point on the curve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathSample {
    /// Where it is, in metres.
    pub at: Vec2,
    /// The way the curve runs there (a unit vector).
    pub along: Vec2,
    /// How far along the curve it is, in metres.
    pub distance: f32,
}

/// The point `t` (0 to 1) of the way along the centripetal Catmull–Rom
/// segment from `p1` to `p2` (with `p0` and `p3` the points either side).
fn catmull_rom(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, t: f32) -> Vec2 {
    // Knots spaced by the square root of each chord's length ("centripetal").
    let knot = |a: Vec2, b: Vec2| a.distance(b).sqrt().max(1e-4);
    let t0 = 0.0;
    let t1 = t0 + knot(p0, p1);
    let t2 = t1 + knot(p1, p2);
    let t3 = t2 + knot(p2, p3);
    let u = t1 + (t2 - t1) * t;
    let lerp = |a: Vec2, b: Vec2, ta: f32, tb: f32| a * ((tb - u) / (tb - ta)) + b * ((u - ta) / (tb - ta));
    let a1 = lerp(p0, p1, t0, t1);
    let a2 = lerp(p1, p2, t1, t2);
    let a3 = lerp(p2, p3, t2, t3);
    let b1 = lerp(a1, a2, t0, t2);
    let b2 = lerp(a2, a3, t1, t3);
    lerp(b1, b2, t1, t2)
}

/// The curve through `points` as a fine polyline (closed paths come back
/// to the start).
fn polyline(points: &[Vec2], closed: bool) -> Vec<Vec2> {
    let count = points.len();
    if count < 2 {
        return points.to_vec();
    }
    let point = |i: isize| -> Vec2 {
        if closed {
            points[i.rem_euclid(count as isize) as usize]
        } else if i < 0 {
            // (Beyond the ends, the end points are extended straight.)
            points[0] * 2.0 - points[1]
        } else if i as usize >= count {
            points[count - 1] * 2.0 - points[count - 2]
        } else {
            points[i as usize]
        }
    };
    let segments = if closed { count } else { count - 1 };
    let mut line = Vec::with_capacity(segments * STEPS_PER_SEGMENT + 1);
    for segment in 0..segments as isize {
        let (p0, p1, p2, p3) = (point(segment - 1), point(segment), point(segment + 1), point(segment + 2));
        for step in 0..STEPS_PER_SEGMENT {
            line.push(catmull_rom(p0, p1, p2, p3, step as f32 / STEPS_PER_SEGMENT as f32));
        }
    }
    line.push(if closed { points[0] } else { points[count - 1] });
    line
}

/// Samples the curve through `points` every `spacing` metres along it,
/// from its start to its end (its last sample is at the end, however near
/// that is to the one before).
pub fn sample_path(points: &[Vec2], closed: bool, spacing: f32) -> Vec<PathSample> {
    let line = polyline(points, closed);
    if line.len() < 2 {
        return Vec::new();
    }
    let spacing = spacing.max(0.01);
    let mut samples = Vec::new();
    let mut travelled = 0.0;
    let mut next = 0.0;
    for pair in line.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let length = a.distance(b);
        if length < 1e-6 {
            continue;
        }
        let along = (b - a) / length;
        while next <= travelled + length {
            let at = a + along * (next - travelled);
            samples.push(PathSample { at, along, distance: next });
            next += spacing;
        }
        travelled += length;
    }
    let end = *line.last().expect("two points at least");
    if samples.last().is_none_or(|last| last.distance < travelled - 1e-3) {
        let along = (end - line[line.len() - 2]).normalize_or(Vec2::X);
        samples.push(PathSample { at: end, along, distance: travelled });
    }
    samples
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curve_passes_through_every_point() {
        let points = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 12.0)];
        let line = polyline(&points, false);
        for point in points {
            assert!(line.iter().any(|p| p.distance(point) < 1e-3), "{point} is on the curve");
        }
    }

    #[test]
    fn samples_are_even_and_follow_the_curve() {
        let points = [Vec2::new(0.0, 0.0), Vec2::new(20.0, 0.0)];
        let samples = sample_path(&points, false, 0.5);
        assert_eq!(samples.len(), 41);
        assert!(samples.windows(2).all(|pair| (pair[1].at.distance(pair[0].at) - 0.5).abs() < 1e-3));
        assert!(samples.iter().all(|sample| (sample.along - Vec2::X).length() < 1e-3));
    }

    #[test]
    fn closed_paths_come_back_round() {
        let square = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)];
        let samples = sample_path(&square, true, 0.5);
        let (first, last) = (samples[0].at, samples.last().unwrap().at);
        assert!(first.distance(last) < 0.01, "ends where it began");
        // Through a square's corners, bulging a little between them: a little over its 40 m.
        let length = samples.last().unwrap().distance;
        assert!((38.0..46.0).contains(&length), "{length}");
    }
}
