//! The tree generator on its own: writes a tree as a glTF model (and a
//! picture of it), or a gallery picture of every species at three ages.
//!
//! ```text
//! cargo run --release -p venture_trees --bin tree -- oak --age 1.5 --seed 7 --out oak.glb --preview oak.png
//! cargo run --release -p venture_trees --bin tree -- gallery --out preview/trees.png
//! ```
//!
//! Options (for one tree): `--age` (0.1 sapling, 1 mature, 2 ancient),
//! `--height` (0 to 1 within the species), `--branchiness`, `--fullness`,
//! `--lean` (1 is usual for each), `--seed`, and `--reduced` for the
//! further-away detail. `--content FOLDER` reads the species (and their
//! photographs) from somewhere other than `content`. For galleries, `--only oak,ash` picks
//! some species, and `--seed` still applies. `--textures FOLDER` also saves
//! a tree's bark, sprig and impostor pictures. `--foliage FOLDER` saves a
//! foliage texture from the species' sprigs instead (a leafy surface that
//! tiles, two metres across: `colour.jpg`, `normal.jpg` and `surface.jpg`, as
//! content textures are), such as the hedges' `content/textures/hedge`.

use std::path::PathBuf;
use std::process::ExitCode;

use glam::Vec3;
use image::RgbaImage;
use venture_trees::preview::{Drawable, View, block, render};
use venture_trees::{Detail, Photos, Species, Tree, TreeParams, TreeTextures, gltf, grow, parse_species, textures};

/// How many metres a pixel covers in pictures.
const METRES_PER_PIXEL: f32 = 0.04;
/// The gallery's ages.
const GALLERY_AGES: [f32; 3] = [0.3, 1.0, 2.0];
/// Space between trees in the gallery, in metres.
const GALLERY_GAP: f32 = 2.0;

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run(arguments: Vec<String>) -> Result<(), String> {
    let mut content = PathBuf::from("content");
    let mut params = TreeParams::default();
    let (mut out, mut preview, mut name, mut detail, mut only) = (None, None, None, Detail::Full, None);
    let mut textures_folder: Option<PathBuf> = None;
    let mut foliage_folder: Option<PathBuf> = None;
    let mut words = arguments.into_iter();
    while let Some(word) = words.next() {
        let mut value = |what: &str| words.next().ok_or_else(|| format!("{word} needs {what} after it"));
        let number = |text: String| text.parse::<f32>().map_err(|_| format!("\"{text}\" isn't a number"));
        match word.as_str() {
            "--content" => content = PathBuf::from(value("a folder")?),
            "--age" => params.age = number(value("an age")?)?,
            "--height" => params.height = number(value("a height from 0 to 1")?)?,
            "--branchiness" => params.branchiness = number(value("a number")?)?,
            "--fullness" => params.fullness = number(value("a number")?)?,
            "--lean" => params.lean = number(value("a number")?)?,
            "--seed" => {
                let text = value("a whole number")?;
                params.seed = text.parse().map_err(|_| format!("\"{text}\" isn't a whole number"))?;
            }
            "--out" => out = Some(PathBuf::from(value("a file")?)),
            "--preview" => preview = Some(PathBuf::from(value("a file")?)),
            "--reduced" => detail = Detail::Reduced,
            "--textures" => textures_folder = Some(PathBuf::from(value("a folder")?)),
            "--only" => only = Some(value("species ids, separated by commas")?),
            "--foliage" => foliage_folder = Some(PathBuf::from(value("a folder")?)),
            "--help" | "-h" => {
                println!("{}", include_str!("tree.rs").lines().take(15).collect::<Vec<_>>().join("\n"));
                return Ok(());
            }
            other if other.starts_with("--") => return Err(format!("There's no option {other}.")),
            other => name = Some(other.to_string()),
        }
    }
    let species_file = content.join("trees/species.ron");
    let text = std::fs::read_to_string(&species_file)
        .map_err(|error| format!("Can't read {}: {error}", species_file.display()))?;
    let all = parse_species(&text).map_err(|error| format!("{}: {error}", species_file.display()))?;
    let name = name.ok_or("Say which species (or `gallery`).")?;
    if name == "gallery" {
        let out = out.unwrap_or_else(|| PathBuf::from("preview/trees.png"));
        let chosen: Vec<Species> = match &only {
            Some(ids) => all.iter().filter(|species| ids.split(',').any(|id| id == species.id)).cloned().collect(),
            None => all,
        };
        return save_picture(&gallery(&chosen, &content, params.seed)?, &out);
    }
    let species = all.iter().find(|species| species.id == name).ok_or_else(|| {
        let ids: Vec<&str> = all.iter().map(|species| species.id.as_str()).collect();
        format!("There's no species \"{name}\". There are: {}.", ids.join(", "))
    })?;
    let textures = textures::paint(species, &Photos::load(species, &content)?);
    if let Some(folder) = foliage_folder {
        return save_foliage(species, &textures, params.seed, &folder);
    }
    let tree = grow(species, &params, detail, &textures);
    println!(
        "{}: {:.1} m tall, trunk {:.2} m across, crown {:.1} m across; {} bark and {} leaf triangles.",
        species.name,
        tree.height,
        tree.trunk_radius * 2.0,
        tree.crown_radius * 2.0,
        tree.meshes.bark.triangles(),
        tree.meshes.leaves.triangles()
    );
    let out = out.unwrap_or_else(|| PathBuf::from(format!("{}.glb", species.id)));
    let bytes = gltf::write_glb(&species.name, &tree.meshes, &textures)?;
    std::fs::write(&out, bytes).map_err(|error| format!("Can't write {}: {error}", out.display()))?;
    println!("Wrote {}.", out.display());
    if let Some(folder) = textures_folder {
        let impostor = venture_trees::impostor::impostor(&tree, &textures);
        save_picture(&impostor.texture, &folder.join(format!("{}_impostor.png", species.id)))?;
        save_picture(&textures.bark, &folder.join(format!("{}_bark.png", species.id)))?;
        save_picture(&textures.leaves, &folder.join(format!("{}_leaves.png", species.id)))?;
    }
    if let Some(preview) = preview {
        save_picture(&picture(&[(&tree, &textures, 0.0)], tree.crown_radius * 2.0 + 4.0, tree.height + 2.0), &preview)?;
    }
    Ok(())
}

/// Saves a foliage texture made from a species' sprigs, as a content texture.
fn save_foliage(species: &Species, textures: &TreeTextures, seed: u64, folder: &std::path::Path) -> Result<(), String> {
    // Between the leaves: the species' darkest leaf colour, in deep shade.
    let shade = species.leaves.colours.1.scaled(0.5).to_bytes();
    let settings = venture_trees::foliage::FoliageSettings { seed, ..Default::default() };
    let foliage = venture_trees::foliage::foliage(&textures.leaves, shade, settings);
    std::fs::create_dir_all(folder).map_err(|error| format!("Can't make {}: {error}", folder.display()))?;
    for (name, picture) in
        [("colour.jpg", &foliage.colour), ("normal.jpg", &foliage.normal), ("surface.jpg", &foliage.surface)]
    {
        let path = folder.join(name);
        let rgb = image::DynamicImage::ImageRgba8(picture.clone()).to_rgb8();
        let mut file =
            std::fs::File::create(&path).map_err(|error| format!("Can't write {}: {error}", path.display()))?;
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, 90)
            .encode_image(&rgb)
            .map_err(|error| format!("Can't write {}: {error}", path.display()))?;
        println!("Wrote {}.", path.display());
    }
    Ok(())
}

fn save_picture(picture: &RgbaImage, path: &PathBuf) -> Result<(), String> {
    if let Some(folder) = path.parent().filter(|folder| !folder.as_os_str().is_empty()) {
        std::fs::create_dir_all(folder).map_err(|error| format!("Can't make {}: {error}", folder.display()))?;
    }
    picture.save(path).map_err(|error| format!("Can't write {}: {error}", path.display()))?;
    println!("Wrote {}.", path.display());
    Ok(())
}

/// A picture of trees standing in a row (each at its `x`, in metres) on
/// the ground, with a person for scale, `width` × `height` metres.
fn picture(trees: &[(&Tree, &TreeTextures, f32)], width: f32, height: f32) -> RgbaImage {
    let ground = block(Vec3::new(-200.0, -1.0, -6.0), Vec3::new(200.0, 0.0, 6.0), [0.16, 0.22, 0.09]);
    let person = block(Vec3::new(-0.25, 0.0, -0.15), Vec3::new(0.25, 1.8, 0.15), [0.5, 0.1, 0.1]);
    let left = trees.first().map_or(0.0, |(tree, _, x)| x - tree.crown_radius) - 1.0;
    let mut things = vec![
        Drawable { mesh: &ground, texture: None, repeats: false, leaves: false, offset: Vec3::ZERO },
        Drawable { mesh: &person, texture: None, repeats: false, leaves: false, offset: Vec3::new(left, 0.0, 0.0) },
    ];
    for (tree, textures, x) in trees {
        let offset = Vec3::new(*x, 0.0, 0.0);
        things.push(Drawable {
            mesh: &tree.meshes.bark,
            texture: Some(&textures.bark),
            repeats: true,
            leaves: false,
            offset,
        });
        things.push(Drawable {
            mesh: &tree.meshes.leaves,
            texture: Some(&textures.leaves),
            repeats: false,
            leaves: true,
            offset,
        });
    }
    let first_x = left - 1.0;
    let view = View {
        centre: Vec3::new(first_x + width / 2.0, height / 2.0 - 0.5, 0.0),
        looking: Vec3::new(0.0, -0.2, -1.0).normalize(),
        metres_per_pixel: METRES_PER_PIXEL,
    };
    let pixels = |metres: f32| (metres / METRES_PER_PIXEL).ceil().max(16.0) as u32;
    render(&things, view, pixels(width), pixels(height))
}

/// Every species at three ages, a row each, all at the same scale.
fn gallery(all: &[Species], content: &std::path::Path, seed: u64) -> Result<RgbaImage, String> {
    let rows: Vec<RgbaImage> = all
        .iter()
        .map(|species| {
            let textures = textures::paint(species, &Photos::load(species, content)?);
            let trees: Vec<Tree> = GALLERY_AGES
                .iter()
                .map(|&age| grow(species, &TreeParams { age, seed, ..TreeParams::default() }, Detail::Full, &textures))
                .collect();
            let mut x = 0.0;
            let mut placed = Vec::new();
            for (i, tree) in trees.iter().enumerate() {
                if i > 0 {
                    x += tree.crown_radius;
                }
                placed.push((tree, &textures, x));
                x += tree.crown_radius + GALLERY_GAP;
            }
            let first = trees[0].crown_radius;
            let width = x + first + 2.0;
            let height = trees.iter().map(|tree| tree.height).fold(0.0, f32::max) + 2.0;
            println!("{}: {}", species.name, trees.iter().map(describe).collect::<Vec<_>>().join("; "));
            Ok(picture(&placed, width, height))
        })
        .collect::<Result<_, String>>()?;
    let width = rows.iter().map(RgbaImage::width).max().unwrap_or(1);
    let height = rows.iter().map(RgbaImage::height).sum::<u32>().max(1);
    let mut sheet = RgbaImage::from_pixel(width, height, image::Rgba([30, 30, 30, 255]));
    let mut top = 0;
    for row in &rows {
        image::imageops::overlay(&mut sheet, row, 0, i64::from(top));
        top += row.height();
    }
    Ok(sheet)
}

fn describe(tree: &Tree) -> String {
    format!(
        "{:.0} m, {}k triangles",
        tree.height,
        (tree.meshes.bark.triangles() + tree.meshes.leaves.triangles()) / 1000
    )
}
