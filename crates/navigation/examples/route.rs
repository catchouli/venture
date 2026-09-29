//! Finds a route over a baked world's navigation mesh (the tiles within
//! three chunks of the start), to check it:
//! `cargo run --release -p venture_navigation --example route -- x y x y [world folder]`.

use std::path::PathBuf;

use venture_navigation::NavWorld;
use venture_world::{CellPos, ChunkPos, chunk_file_path, decode_chunk};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let number = |i: usize| args.get(i).and_then(|arg| arg.parse::<f32>().ok()).expect("x y x y [world folder]");
    let (start, goal) = ((number(0), number(1)), (number(2), number(3)));
    let folder = PathBuf::from(args.get(4).cloned().unwrap_or_else(|| "world".to_string()));
    let middle = CellPos::new(start.0 as i32, start.1 as i32).chunk();
    let mut nav = NavWorld::new();
    let mut height = 0.0;
    for dy in -3..=3 {
        for dx in -3..=3 {
            let pos = ChunkPos::new(middle.x + dx, middle.y + dy);
            let Ok(bytes) = std::fs::read(folder.join(chunk_file_path(pos))) else { continue };
            let chunk = decode_chunk(&bytes).expect("a chunk");
            if pos == middle {
                let (x, y) = pos.origin_cell();
                height = chunk.height_at(start.0 - x as f32, start.1 - y as f32);
            }
            if let Err(error) = nav.add_tile(pos, &chunk.navmesh) {
                println!("{error}");
            }
        }
    }
    nav.join();
    let start = [start.0, start.1, height];
    println!("Nearest standing to the start: {:?}", nav.nearest_standing(start, 5.0));
    println!("Route: {:?}", nav.route(start, [goal.0, goal.1, height]));
}
