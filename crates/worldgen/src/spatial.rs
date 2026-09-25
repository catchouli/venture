//! A simple spatial index: items grouped into square buckets by position, so
//! a chunk only needs to look at the items near it.

use crate::geometry::Point;

/// Items grouped into square buckets covering the world.
#[derive(Clone, Debug)]
pub struct Buckets<T> {
    bucket_size: f32,
    per_side: usize,
    buckets: Vec<Vec<T>>,
}

impl<T: Clone> Buckets<T> {
    /// Empty buckets `bucket_size` metres wide covering a `world_size`-metre world.
    pub fn new(world_size: f32, bucket_size: f32) -> Self {
        let per_side = (world_size / bucket_size).ceil().max(1.0) as usize;
        Self { bucket_size, per_side, buckets: vec![Vec::new(); per_side * per_side] }
    }

    /// Adds an item to the bucket containing `position`.
    pub fn insert(&mut self, position: Point, item: T) {
        let (bx, by) = self.bucket_of(position);
        self.buckets[by * self.per_side + bx].push(item);
    }

    /// Every item whose position is within `reach` metres of the area from
    /// `min` to `max` (and possibly a few more), in a fixed order.
    pub fn near(&self, min: Point, max: Point, reach: f32) -> Vec<T> {
        let (bx0, by0) = self.bucket_of((min.0 - reach, min.1 - reach));
        let (bx1, by1) = self.bucket_of((max.0 + reach, max.1 + reach));
        let mut items = Vec::new();
        for by in by0..=by1 {
            for bx in bx0..=bx1 {
                items.extend_from_slice(&self.buckets[by * self.per_side + bx]);
            }
        }
        items
    }

    /// Every item.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.buckets.iter().flatten()
    }

    /// The bucket containing a position, clamped to the world.
    fn bucket_of(&self, (x, y): Point) -> (usize, usize) {
        let to_bucket = |metres: f32| ((metres / self.bucket_size).floor().max(0.0) as usize).min(self.per_side - 1);
        (to_bucket(x), to_bucket(y))
    }
}
