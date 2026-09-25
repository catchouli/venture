//! 2D geometry helpers, in tile coordinates.

/// A point in tile coordinates.
pub type Point = (f32, f32);

/// The closest point to `p` on the segment from `a` to `b`, as the fraction
/// `t` along the segment (`0.0` at `a`, `1.0` at `b`) and the distance to it.
pub fn closest_on_segment(a: Point, b: Point, p: Point) -> (f32, f32) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length_sq = dx * dx + dy * dy;
    let t = if length_sq > 0.0 { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length_sq).clamp(0.0, 1.0) } else { 0.0 };
    let (closest_x, closest_y) = (a.0 + dx * t, a.1 + dy * t);
    (t, (p.0 - closest_x).hypot(p.1 - closest_y))
}

/// The point a fraction `t` of the way from `a` to `b`.
pub fn lerp_point(a: Point, b: Point, t: f32) -> Point {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

/// Rounds the corners of a polyline with Chaikin's corner-cutting algorithm.
/// Each iteration replaces every segment with points at 1/4 and 3/4 along it.
/// The end points are kept.
pub fn chaikin_smooth(mut points: Vec<Point>, iterations: usize) -> Vec<Point> {
    for _ in 0..iterations {
        let (Some(&first), Some(&last)) = (points.first(), points.last()) else {
            return points;
        };
        let mut smoothed = vec![first];
        for pair in points.windows(2) {
            smoothed.push(lerp_point(pair[0], pair[1], 0.25));
            smoothed.push(lerp_point(pair[0], pair[1], 0.75));
        }
        smoothed.push(last);
        points = smoothed;
    }
    points
}
