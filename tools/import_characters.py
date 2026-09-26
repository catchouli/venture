#!/usr/bin/env python3
"""Copies the Quaternius character models into the content pack, ready for the game.

The Universal Base Characters, Modular Character Outfits, Universal
Animation Library and Fantasy Props MegaKit (for weapons and shields; all
CC0, from https://quaternius.com) are downloaded separately, and left as
they are. (Add any pack you take from to licenses/README.md, with its
licence file.) This copies the parts the game uses into content/characters/,
flat, with lower-case names, and prepares them:

- **Textures are shrunk** (colour to 1024 pixels, normal and roughness maps
  to 512), so the content pack and the web download stay reasonable.
- **Four bones per vertex**, the most Bevy uses: each vertex keeps its four
  strongest, with weights adding up to 1.
- **Vertex colours are dropped.** Quaternius uses them as masks for his own
  shaders; the game's standard material would darken the model with them.
- **Each file keeps only the data it uses**, packed into its own .bin.
- **Animations keep only bone rotations** (plus the hips' movement), and
  only the ones the game uses. The animation library's figure has slightly
  different proportions from the bodies, and keeping its bone positions
  would stretch the bodies to fit it.

Usage (macOS, since it shrinks textures with `sips`):

    python3 tools/import_characters.py ~/assets

where ~/assets holds the three downloaded packs as they unzip. Running it
again replaces what it copied.
"""

import json
import os
import shutil
import struct
import subprocess
import sys

OUT = os.path.join(os.path.dirname(__file__), "..", "content", "characters")

BASE = "Universal Base Characters[Standard]"
OUTFITS = "Modular Character Outfits - Fantasy[Standard]"
ANIMATIONS = "Universal Animation Library[Standard]"
PROPS = "Fantasy Props MegaKit[Standard]"

# (source, output name) for each model.
MODELS = [
    (f"{BASE}/Base Characters/Godot - UE/Superhero_Male_FullBody.gltf", "body_male"),
    (f"{BASE}/Base Characters/Godot - UE/Superhero_Female_FullBody.gltf", "body_female"),
] + [
    (f"{BASE}/Hairstyles/Rigged to Head Bone/glTF (Godot -Unreal)/{name}.gltf", name.lower())
    for name in ["Eyebrows_Female", "Eyebrows_Regular", "Hair_Beard", "Hair_Buns", "Hair_Buzzed",
                 "Hair_BuzzedFemale", "Hair_Long", "Hair_SimpleParted"]
] + [
    (f"{OUTFITS}/Exports/glTF (Godot-Unreal)/Modular Parts/{body}_{outfit}_{part}.gltf",
     f"{body}_{outfit}_{part}".lower())
    for body, outfit, parts in [
        ("Male", "Peasant", ["Arms", "Body", "Feet", "Legs"]),
        ("Female", "Peasant", ["Arms", "Body", "Feet", "Legs"]),
        ("Male", "Ranger", ["Arms", "Body", "Feet_Boots", "Legs", "Head_Hood", "Acc_Pauldron"]),
        ("Female", "Ranger", ["Arms", "Body", "Feet", "Legs", "Head_Hood", "Acc_Pauldrons"]),
    ]
    for part in parts
]

# Things characters hold.
MODELS += [
    (f"{PROPS}/Exports/glTF/Sword_Bronze.gltf", "sword_bronze"),
    (f"{PROPS}/Exports/glTF/Shield_Wooden.gltf", "shield_wooden"),
]

# Extra skin colours for the bodies (the models use the dark ones).
EXTRA_TEXTURES = [
    f"{BASE}/Base Characters/Textures/T_Superhero_Male_Ligh.png",
    f"{BASE}/Base Characters/Textures/T_Superhero_Female_Light_BaseColor.png",
]

ANIMATION_FILE = f"{ANIMATIONS}/Unreal-Godot/UAL1_Standard.glb"
# The animations the game uses.
ANIMATIONS_KEPT = [
    "Idle_Loop", "Walk_Loop", "Jog_Fwd_Loop", "Sprint_Loop",
    "Sword_Attack", "Sword_Idle", "Punch_Jab", "Punch_Cross",
    "Spell_Simple_Enter", "Spell_Simple_Idle_Loop", "Spell_Simple_Shoot", "Spell_Simple_Exit",
    "Hit_Chest", "Hit_Head", "Death01",
    "Jump_Start", "Jump_Loop", "Jump_Land",
    "Sitting_Enter", "Sitting_Idle_Loop", "Sitting_Exit", "Fixing_Kneeling",
    "Interact", "PickUp_Table", "Idle_Talking_Loop", "Swim_Fwd_Loop", "Swim_Idle_Loop", "Crouch_Idle_Loop",
    "Crouch_Fwd_Loop", "Roll",
]
# Bones whose movement (not just turning) the animations keep.
MOVING_BONES = {"root", "pelvis"}
# The body's hips are this much higher than the animation figure's (metres):
# added to the hips' movement so feet stay on the ground.
HIPS_RAISE = 0.949 - 0.917

# Largest texture sizes, by kind.
COLOUR_SIZE = 1024
DETAIL_SIZE = 512

COMPONENTS = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}
SIZES = {5120: 1, 5121: 1, 5122: 2, 5123: 2, 5125: 4, 5126: 4}


def load(path):
    """A glTF's JSON and its binary buffer."""
    data = open(path, "rb").read()
    if data[:4] == b"glTF":
        json_length = struct.unpack("<I", data[12:16])[0]
        gltf = json.loads(data[20:20 + json_length])
        rest = data[20 + json_length:]
        binary = rest[8:8 + struct.unpack("<I", rest[:4])[0]] if rest else b""
        return gltf, binary
    gltf = json.loads(data)
    uri = gltf["buffers"][0]["uri"]
    return gltf, open(os.path.join(os.path.dirname(path), uri), "rb").read()


def compact(gltf, binary):
    """Rebuilds the buffer with only the accessors still used, one buffer
    view each. Returns the new binary."""
    used = set()
    for mesh in gltf.get("meshes", []):
        for primitive in mesh["primitives"]:
            used.update(primitive["attributes"].values())
            if "indices" in primitive:
                used.add(primitive["indices"])
    for skin in gltf.get("skins", []):
        if "inverseBindMatrices" in skin:
            used.add(skin["inverseBindMatrices"])
    for animation in gltf.get("animations", []):
        for sampler in animation["samplers"]:
            used.update([sampler["input"], sampler["output"]])
    old_views = gltf["bufferViews"]
    remap, accessors, views, out = {}, [], [], bytearray()
    for index in sorted(used):
        accessor = dict(gltf["accessors"][index])
        view = old_views[accessor["bufferView"]]
        element = COMPONENTS[accessor["type"]] * SIZES[accessor["componentType"]]
        start = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
        stride = view.get("byteStride", element)
        chunk = b"".join(binary[start + i * stride:start + i * stride + element] for i in range(accessor["count"]))
        while len(out) % 4:
            out.append(0)
        new_view = {"buffer": 0, "byteOffset": len(out), "byteLength": len(chunk)}
        if "target" in view:
            new_view["target"] = view["target"]
        out += chunk
        accessor["bufferView"] = len(views)
        accessor.pop("byteOffset", None)
        views.append(new_view)
        remap[index] = len(accessors)
        accessors.append(accessor)
    for mesh in gltf.get("meshes", []):
        for primitive in mesh["primitives"]:
            primitive["attributes"] = {k: remap[v] for k, v in primitive["attributes"].items()}
            if "indices" in primitive:
                primitive["indices"] = remap[primitive["indices"]]
    for skin in gltf.get("skins", []):
        if "inverseBindMatrices" in skin:
            skin["inverseBindMatrices"] = remap[skin["inverseBindMatrices"]]
    for animation in gltf.get("animations", []):
        for sampler in animation["samplers"]:
            sampler["input"], sampler["output"] = remap[sampler["input"]], remap[sampler["output"]]
    gltf["accessors"], gltf["bufferViews"] = accessors, views
    gltf["buffers"] = [{"byteLength": len(out)}]
    return bytes(out)


def read_accessor(gltf, binary, index):
    """An accessor's values, as a list of tuples."""
    accessor = gltf["accessors"][index]
    view = gltf["bufferViews"][accessor["bufferView"]]
    count, width = accessor["count"], COMPONENTS[accessor["type"]]
    code = {5120: "b", 5121: "B", 5122: "h", 5123: "H", 5125: "I", 5126: "f"}[accessor["componentType"]]
    size = SIZES[accessor["componentType"]]
    start = view.get("byteOffset", 0) + accessor.get("byteOffset", 0)
    stride = view.get("byteStride", width * size)
    values = [struct.unpack_from(f"<{width}{code}", binary, start + i * stride) for i in range(count)]
    if accessor.get("normalized") and code in "BH":
        top = 255.0 if code == "B" else 65535.0
        values = [tuple(v / top for v in value) for value in values]
    return values


def add_accessor(gltf, binary, values, component_type, kind):
    """Appends values as a new accessor; returns its index and the new binary."""
    code = {5123: "H", 5126: "f"}[component_type]
    width = COMPONENTS[kind]
    data = b"".join(struct.pack(f"<{width}{code}", *value) for value in values)
    binary += b"\0" * (-len(binary) % 4)
    gltf["bufferViews"].append({"buffer": 0, "byteOffset": len(binary), "byteLength": len(data)})
    gltf["accessors"].append({"bufferView": len(gltf["bufferViews"]) - 1, "componentType": component_type,
                              "count": len(values), "type": kind})
    return len(gltf["accessors"]) - 1, binary + data


def merge_weights(gltf, binary):
    """Bevy uses only four bones per vertex (JOINTS_0 and WEIGHTS_0): keep
    each vertex's four strongest bones from both sets, with weights adding up
    to 1. (Otherwise vertices pulled by more bones have weights adding up to
    less than 1, which drags them towards the world's origin.)"""
    for mesh in gltf.get("meshes", []):
        for primitive in mesh["primitives"]:
            attributes = primitive["attributes"]
            if "JOINTS_0" not in attributes:
                continue
            joints = read_accessor(gltf, binary, attributes["JOINTS_0"])
            weights = read_accessor(gltf, binary, attributes["WEIGHTS_0"])
            if "JOINTS_1" in attributes:
                extra_joints = read_accessor(gltf, binary, attributes["JOINTS_1"])
                extra_weights = read_accessor(gltf, binary, attributes["WEIGHTS_1"])
                joints = [a + b for a, b in zip(joints, extra_joints)]
                weights = [a + b for a, b in zip(weights, extra_weights)]
            new_joints, new_weights = [], []
            for vertex_joints, vertex_weights in zip(joints, weights):
                strongest = sorted(zip(vertex_weights, vertex_joints), reverse=True)[:4]
                total = sum(weight for weight, _ in strongest) or 1.0
                new_joints.append(tuple(joint for _, joint in strongest))
                new_weights.append(tuple(weight / total for weight, _ in strongest))
            attributes["JOINTS_0"], binary = add_accessor(gltf, binary, new_joints, 5123, "VEC4")
            attributes["WEIGHTS_0"], binary = add_accessor(gltf, binary, new_weights, 5126, "VEC4")
            attributes.pop("JOINTS_1", None)
            attributes.pop("WEIGHTS_1", None)
    return binary


def texture_name(uri):
    """The output name of a texture: lower case, without Quaternius's
    doubled `_png` suffix."""
    return uri.replace("_png.png", ".png").lower()


def shrink(source, target, size):
    """Copies a PNG, shrunk to at most `size` pixels across."""
    shutil.copyfile(source, target)
    subprocess.run(["sips", "-Z", str(size), target], check=True, capture_output=True)


def find_texture(root, source_dir, uri):
    """A texture next to the model, or in its pack's Textures folders."""
    candidates = [os.path.join(source_dir, uri), os.path.join(source_dir, uri.replace("_png.png", ".png"))]
    for folder, _, files in os.walk(root):
        if "Normals" in folder:
            continue
        for name in files:
            if name in (uri, uri.replace("_png.png", ".png")):
                candidates.append(os.path.join(folder, name))
    return next((path for path in candidates if os.path.exists(path)), None)


def import_model(root, source, name, textures):
    gltf, binary = load(os.path.join(root, source))
    for mesh in gltf.get("meshes", []):
        for primitive in mesh["primitives"]:
            # (Only the first two texture coordinate sets are used.)
            for attribute in list(primitive["attributes"]):
                if attribute.startswith("COLOR_") or attribute in [f"TEXCOORD_{n}" for n in range(2, 8)]:
                    primitive["attributes"].pop(attribute)
    binary = merge_weights(gltf, binary)
    binary = compact(gltf, binary)
    gltf["buffers"][0]["uri"] = f"{name}.bin"
    # Which kind each image is (colour, or a detail map), to shrink it right.
    kinds = {}
    for material in gltf.get("materials", []):
        pbr = material.get("pbrMetallicRoughness", {})
        for key, kind in [("baseColorTexture", COLOUR_SIZE), ("metallicRoughnessTexture", DETAIL_SIZE)]:
            if key in pbr:
                kinds[gltf["textures"][pbr[key]["index"]]["source"]] = kind
        for key in ["normalTexture", "occlusionTexture"]:
            if key in material:
                kinds[gltf["textures"][material[key]["index"]]["source"]] = DETAIL_SIZE
    for index, image in enumerate(gltf.get("images", [])):
        out_name = texture_name(image["uri"])
        if out_name not in textures:
            found = find_texture(os.path.join(root, source.split("/")[0]), os.path.dirname(os.path.join(root, source)),
                                 image["uri"])
            if not found:
                sys.exit(f"can't find the texture {image['uri']} for {source}")
            shrink(found, os.path.join(OUT, out_name), kinds.get(index, DETAIL_SIZE))
            textures.add(out_name)
        image["uri"] = out_name
    open(os.path.join(OUT, f"{name}.bin"), "wb").write(binary)
    json.dump(gltf, open(os.path.join(OUT, f"{name}.gltf"), "w"), separators=(",", ":"))


def import_animations(root):
    gltf, binary = load(os.path.join(root, ANIMATION_FILE))
    nodes = gltf["nodes"]
    kept = []
    for animation in gltf["animations"]:
        if animation["name"] not in ANIMATIONS_KEPT:
            continue
        channels = []
        for channel in animation["channels"]:
            bone = nodes[channel["target"]["node"]]["name"]
            path = channel["target"]["path"]
            if path == "rotation" or (path == "translation" and bone in MOVING_BONES):
                channels.append(channel)
        # Samplers still used, renumbered.
        used = sorted({channel["sampler"] for channel in channels})
        renumber = {old: new for new, old in enumerate(used)}
        for channel in channels:
            channel["sampler"] = renumber[channel["sampler"]]
        animation["samplers"] = [animation["samplers"][index] for index in used]
        animation["channels"] = channels
        kept.append(animation)
    gltf["animations"] = kept
    # Just the skeleton and the animations: no mannequin mesh.
    for node in nodes:
        node.pop("mesh", None)
        node.pop("skin", None)
    for key in ["meshes", "skins", "materials", "textures", "images", "samplers"]:
        gltf.pop(key, None)
    binary = bytearray(compact(gltf, binary))
    # Raise the hips (their movement is along the armature's Z, which is up).
    pelvis = next(i for i, node in enumerate(nodes) if node["name"] == "pelvis")
    for animation in gltf["animations"]:
        for channel in animation["channels"]:
            if channel["target"] == {"node": pelvis, "path": "translation"}:
                accessor = gltf["accessors"][animation["samplers"][channel["sampler"]]["output"]]
                start = gltf["bufferViews"][accessor["bufferView"]]["byteOffset"]
                for i in range(accessor["count"]):
                    offset = start + i * 12 + 8
                    z = struct.unpack_from("<f", binary, offset)[0]
                    struct.pack_into("<f", binary, offset, z + HIPS_RAISE)
                accessor.pop("min", None)
                accessor.pop("max", None)
    gltf["buffers"] = [{"byteLength": len(binary)}]
    text = json.dumps(gltf, separators=(",", ":")).encode()
    text += b" " * (-len(text) % 4)
    binary = bytes(binary) + b"\0" * (-len(binary) % 4)
    total = 12 + 8 + len(text) + 8 + len(binary)
    with open(os.path.join(OUT, "animations.glb"), "wb") as file:
        file.write(struct.pack("<III", 0x46546C67, 2, total))
        file.write(struct.pack("<II", len(text), 0x4E4F534A) + text)
        file.write(struct.pack("<II", len(binary), 0x004E4942) + binary)
    return [animation["name"] for animation in kept]


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    root = os.path.expanduser(sys.argv[1])
    os.makedirs(OUT, exist_ok=True)
    for name in os.listdir(OUT):
        if name.endswith((".gltf", ".bin", ".glb", ".png")):
            os.remove(os.path.join(OUT, name))
    textures = set()
    for source, name in MODELS:
        import_model(root, source, name, textures)
        print("model", name)
    for source in EXTRA_TEXTURES:
        out_name = texture_name(os.path.basename(source))
        shrink(os.path.join(root, source), os.path.join(OUT, out_name), COLOUR_SIZE)
        print("texture", out_name)
    names = import_animations(root)
    print("animations", len(names))


if __name__ == "__main__":
    main()
