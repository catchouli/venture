//! Writing a tree as a binary glTF 2.0 file (`.glb`): one mesh with two
//! primitives, the bark and the leaves, each with its material and
//! texture (the textures go inside the file, as PNGs).
//!
//! - The bark's material is opaque and rough, and its texture repeats.
//! - The leaves' material is alpha-tested (`MASK`) and double-sided.
//! - Vertex colours (`COLOR_0`) carry the bark's colour and the crown's
//!   shading.
//! - Each vertex's wind sway is in a custom attribute, `_SWAY` (glTF
//!   allows custom attributes starting with an underscore).
//!
//! The format: <https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html>.

use std::io::Cursor;

use image::{ImageFormat, RgbaImage};
use serde_json::{Value, json};

use crate::mesh::{MeshPart, TreeMeshes};
use crate::textures::TreeTextures;

/// glTF's numbers for component types, buffer targets and samplers.
const FLOAT: u32 = 5126;
const UNSIGNED_INT: u32 = 5125;
const ARRAY_BUFFER: u32 = 34962;
const ELEMENT_ARRAY_BUFFER: u32 = 34963;
const LINEAR: u32 = 9729;
const LINEAR_MIPMAP_LINEAR: u32 = 9987;
const REPEAT: u32 = 10497;
const CLAMP_TO_EDGE: u32 = 33071;

/// The file's bytes, for a tree called `name`.
pub fn write_glb(name: &str, meshes: &TreeMeshes, textures: &TreeTextures) -> Result<Vec<u8>, String> {
    let mut file = Builder::default();
    let mut primitives = Vec::new();
    for (part, material) in [(&meshes.bark, 0), (&meshes.leaves, 1)] {
        if part.indices.is_empty() {
            continue;
        }
        primitives.push(file.primitive(part, material));
    }
    let bark_image = file.image(&textures.bark)?;
    let leaves_image = file.image(&textures.leaves)?;
    let json = json!({
        "asset": { "version": "2.0", "generator": "venture_trees" },
        "scene": 0,
        "scenes": [{ "nodes": [0] }],
        "nodes": [{ "mesh": 0, "name": name }],
        "meshes": [{ "name": name, "primitives": primitives }],
        "materials": [
            {
                "name": "bark",
                "pbrMetallicRoughness": {
                    "baseColorTexture": { "index": 0 },
                    "metallicFactor": 0.0,
                    "roughnessFactor": 0.95
                }
            },
            {
                "name": "leaves",
                "pbrMetallicRoughness": {
                    "baseColorTexture": { "index": 1 },
                    "metallicFactor": 0.0,
                    "roughnessFactor": 0.75
                },
                "alphaMode": "MASK",
                "alphaCutoff": 0.5,
                "doubleSided": true
            }
        ],
        "textures": [{ "source": bark_image, "sampler": 0 }, { "source": leaves_image, "sampler": 1 }],
        "samplers": [
            { "magFilter": LINEAR, "minFilter": LINEAR_MIPMAP_LINEAR, "wrapS": REPEAT, "wrapT": REPEAT },
            { "magFilter": LINEAR, "minFilter": LINEAR_MIPMAP_LINEAR, "wrapS": CLAMP_TO_EDGE, "wrapT": CLAMP_TO_EDGE }
        ],
        "images": file.images,
        "accessors": file.accessors,
        "bufferViews": file.views,
        "buffers": [{ "byteLength": file.binary.len() }]
    });
    Ok(glb(&json, &file.binary))
}

/// The binary buffer, and what's been put in it.
#[derive(Default)]
struct Builder {
    binary: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
    images: Vec<Value>,
}

impl Builder {
    /// Adds bytes as a buffer view (four-byte aligned), and returns its index.
    fn view(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        while !self.binary.len().is_multiple_of(4) {
            self.binary.push(0);
        }
        let mut view = json!({ "buffer": 0, "byteOffset": self.binary.len(), "byteLength": bytes.len() });
        if let Some(target) = target {
            view["target"] = json!(target);
        }
        self.binary.extend_from_slice(bytes);
        self.views.push(view);
        self.views.len() - 1
    }

    /// Adds floats (`components` a vertex) as an accessor, and returns its index.
    fn floats(&mut self, values: &[f32], components: usize, kind: &str, bounds: bool) -> usize {
        let bytes: Vec<u8> = values.iter().flat_map(|value| value.to_le_bytes()).collect();
        let view = self.view(&bytes, Some(ARRAY_BUFFER));
        let mut accessor =
            json!({ "bufferView": view, "componentType": FLOAT, "count": values.len() / components, "type": kind });
        if bounds {
            let (mut low, mut high) = (vec![f32::MAX; components], vec![f32::MIN; components]);
            for vertex in values.chunks_exact(components) {
                for (i, &value) in vertex.iter().enumerate() {
                    low[i] = low[i].min(value);
                    high[i] = high[i].max(value);
                }
            }
            accessor["min"] = json!(low);
            accessor["max"] = json!(high);
        }
        self.accessors.push(accessor);
        self.accessors.len() - 1
    }

    /// Adds a mesh part as a primitive, and returns its description.
    fn primitive(&mut self, part: &MeshPart, material: usize) -> Value {
        let flat = |values: &[[f32; 3]]| values.iter().flatten().copied().collect::<Vec<f32>>();
        let position = self.floats(&flat(&part.positions), 3, "VEC3", true);
        let normal = self.floats(&flat(&part.normals), 3, "VEC3", false);
        let uvs: Vec<f32> = part.uvs.iter().flatten().copied().collect();
        let texcoord = self.floats(&uvs, 2, "VEC2", false);
        let colours: Vec<f32> = part.colours.iter().flatten().copied().collect();
        let colour = self.floats(&colours, 4, "VEC4", false);
        let sway = self.floats(&part.sway, 1, "SCALAR", false);
        let bytes: Vec<u8> = part.indices.iter().flat_map(|index| index.to_le_bytes()).collect();
        let view = self.view(&bytes, Some(ELEMENT_ARRAY_BUFFER));
        self.accessors.push(json!({
            "bufferView": view,
            "componentType": UNSIGNED_INT,
            "count": part.indices.len(),
            "type": "SCALAR"
        }));
        let indices = self.accessors.len() - 1;
        json!({
            "attributes": {
                "POSITION": position,
                "NORMAL": normal,
                "TEXCOORD_0": texcoord,
                "COLOR_0": colour,
                "_SWAY": sway
            },
            "indices": indices,
            "material": material
        })
    }

    /// Adds an image as a PNG, and returns its index.
    fn image(&mut self, image: &RgbaImage) -> Result<usize, String> {
        let mut png = Vec::new();
        image.write_to(&mut Cursor::new(&mut png), ImageFormat::Png).map_err(|error| error.to_string())?;
        let view = self.view(&png, None);
        self.images.push(json!({ "bufferView": view, "mimeType": "image/png" }));
        Ok(self.images.len() - 1)
    }
}

/// A `.glb` file: a header, then the JSON chunk and the binary chunk, each padded to four bytes.
fn glb(json: &Value, binary: &[u8]) -> Vec<u8> {
    let mut text = json.to_string().into_bytes();
    while !text.len().is_multiple_of(4) {
        text.push(b' ');
    }
    let mut binary = binary.to_vec();
    while !binary.len().is_multiple_of(4) {
        binary.push(0);
    }
    let total = 12 + 8 + text.len() + 8 + binary.len();
    let mut file = Vec::with_capacity(total);
    file.extend_from_slice(b"glTF");
    file.extend_from_slice(&2u32.to_le_bytes());
    file.extend_from_slice(&(total as u32).to_le_bytes());
    file.extend_from_slice(&(text.len() as u32).to_le_bytes());
    file.extend_from_slice(b"JSON");
    file.extend_from_slice(&text);
    file.extend_from_slice(&(binary.len() as u32).to_le_bytes());
    file.extend_from_slice(b"BIN\0");
    file.extend_from_slice(&binary);
    file
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::{Detail, build};
    use crate::params::TreeParams;
    use crate::skeleton::grow;
    use crate::species::{Bark, BranchLevel, CrownShape, Leaves, Species, Trunk};
    use crate::textures::paint;

    #[test]
    fn glb_files_have_a_valid_header_and_chunks() {
        let species = Species {
            id: "test".to_string(),
            name: "Test".to_string(),
            height: (8.0, 10.0),
            trunk: Trunk::default(),
            crown: CrownShape::Spherical,
            branches: vec![BranchLevel { count: 6.0, ..BranchLevel::default() }],
            leaves: Leaves::default(),
            bark: Bark::default(),
        };
        let meshes = build(&species, &grow(&species, &TreeParams::default()), Detail::Full, 1.0);
        let bytes = write_glb("test", &meshes, &paint(&species, &Default::default())).unwrap();
        assert_eq!(&bytes[0..4], b"glTF");
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize, bytes.len());
        let json_length = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        assert_eq!(&bytes[16..20], b"JSON");
        let json: Value = serde_json::from_slice(&bytes[20..20 + json_length]).unwrap();
        assert_eq!(json["meshes"][0]["primitives"].as_array().unwrap().len(), 2);
        assert_eq!(&bytes[24 + json_length..28 + json_length], b"BIN\0");
    }
}
