# Plan: real character models, and a character creator

**Status (September 2026):** steps 1 to 4 are done, with Quaternius's
Universal Base Characters, Modular Character Outfits, Universal Animation
Library and Fantasy Props (all CC0). People are a body, hair and outfit
parts on one skeleton, dressed by what they wear, holding their sword and
shield; see "Character models" in [content-authoring.md](content-authoring.md)
and `crates/client/src/characters/humans.rs`. Still to do: the creator
(steps 5 and 6), creatures (step 7), a sit-on-the-ground animation
(resting kneels for now), grips for staffs and clubs (placeholder shapes for
now), and a check of the download size and frame rate on the web.

This plan replaced the placeholder shapes (spheres and boxes, animated in
code) with real models and animations as cheaply as possible, then grows into a **character creator**: sliders and choices
(build, face, hair, colours, outfits) that make every human from the same
parts. Designers use it to make NPCs, and later players use it too.

It's the second part of the "soul" phase (see [roadmap.md](roadmap.md)),
after the interface layouts.

## The constraints

- Almost no budget: free, or a few dollars.
- One beginner 3D modeller (the lead), and nobody else to spare.
- Bevy loads glTF models with skeletons and animations, blends animations
  (animation graphs, with masks for "attack while running"), and supports
  morph targets (blend shapes). It has **no animation retargeting**: an
  animation only plays on the skeleton it was made for. So **every human
  shares one skeleton**, and anything new is rigged to that skeleton.

## What's out there (checked September 2026)

| What | What it gives us | Cost and licence |
| --- | --- | --- |
| [Quaternius Universal Base Characters](https://quaternius.com/packs/universalbasecharacters.html) | 6 rigged base humans (three builds, masculine and feminine), 20 hairstyles, skin and eye colours; glTF and FBX | CC0. Most of it is free; the rest and the .blend sources are $20 |
| [Quaternius Universal Animation Library](https://quaternius.com/packs/universalanimationlibrary.html) | 120+ animations on the **same rig**: locomotion in 8 directions, jogging, sprinting, crawling, **swimming**, sitting, deaths, combat, emotes | CC0. Standard and Pro are free; the .blend source is paid |
| [Universal Animation Library 2](https://quaternius.com/packs/universalanimationlibrary2.html) | 130+ more on the same rig: melee combos, parkour, **farming, fishing** (handy for skills) | CC0. Much of it free; the source is paid |
| [MPFB 2](https://static.makehumancommunity.org/mpfb.html) (MakeHuman for Blender) | A free human generator inside Blender, with sliders for body and face, hair, clothes and eyes, rigs (including game engine and Mixamo rigs), and glTF export | Free. The add-on is GPL, but what it makes (and its bundled assets) are CC0 |
| [Mixamo](https://helpx.adobe.com/creative-cloud/faq/mixamo-faq.html) | Automatic rigging of any humanoid mesh, and 2,500+ animations | Free, and fine to ship in a game. But the files can't be redistributed as they are (awkward for a content pack), and it's no longer developed |

Sources: [Quaternius](https://quaternius.com/), [Universal Animation Library on
OpenGameArt](https://opengameart.org/content/universal-animation-library),
[MPFB on GitHub](https://github.com/makehumancommunity/mpfb2),
[MPFB export docs](https://static.makehumancommunity.org/mpfb/docs/exporting.html),
[Bevy skinned glTF example](https://bevy.org/examples/animation/gltf-skinned-mesh/),
[Bevy morph targets example](https://bevy.org/examples/animation/morph-targets/),
[Mixamo alternatives (2026)](https://app.cinevva.com/guides/free-character-animations-rigging).

## The recommendation

**Standardise on the Quaternius universal rig.** Its base characters and both
animation libraries share it, everything is CC0 (so it can go in the content
pack and be changed freely), and it comes as glTF, which Bevy loads directly.
That gets real, animated people into the game with almost no modelling.
Spending $20 to $35 on the .blend sources is worth it (it lets us edit the rig
and make clothes that fit), but isn't needed to start.

**Use MPFB for what Quaternius doesn't cover:** more realistic faces and
bodies, and the slider shapes for the creator. MPFB is itself a character
creator with sliders, inside Blender, so designers can already make NPC
variants there. Its meshes are rigged to the universal skeleton in Blender:
parent them to it with automatic weights, and tidy the weights where needed.
This is the one step that takes some Blender skill, and it only has to be done
once per base mesh.

**Keep Mixamo as a fallback** for a missing animation: auto-rig, download,
then retarget onto the universal rig in Blender before it goes in the content.

## How a character is put together

A character becomes a recipe (in the content, for NPCs; saved in the profile,
for players) made from a small set of parts:

- **Skeleton:** the universal rig, shared by everyone.
- **Body:** a base mesh (a build and body type). **Sliders** for height,
  build, and the size of the head and hands, done by scaling bones: no extra
  art needed. Later, sliders for weight, muscle and the face, done by
  **morph targets** made in Blender (MPFB can produce them).
- **Hair and face details:** separate meshes, chosen from a list, attached to
  the head.
- **Colours:** skin, hair and eyes are material colours, like the tint on
  today's placeholder clothes.
- **Clothes and equipment:** meshes rigged to the same skeleton (armour,
  robes), and weapons and shields attached to the hands. Our items' `look`
  already says what's worn; it would name a mesh instead of a placeholder
  shape.

**The NPC generator** picks from all this within ranges ("any villager: build
50–70, hair from this list, these skin tones"), seeded by the NPC's id so each
NPC always looks the same. A designer can pin exact choices for named
characters. **The player creator** is the same thing with sliders in the
character creation screen.

## Steps

1. **A spike:** load one base character and a few animations (idle, run,
   attack) from glTF in Bevy, on desktop and the web. Check the look, the size
   of the download, and the frame rate with twenty people on screen.
2. **Replace the placeholders:** players and NPCs become a base body, hair and
   colours, animated with an animation graph driven by what the code already
   tracks (standing, running, swinging, waving, resting, knocked down), with
   masks so the upper body can attack while the legs run.
3. **Characters as content:** bodies, hair, clothes and animations as glTF
   files in the content pack (`content/characters/`), sent to clients like
   icons are. The game loads them from the downloaded content, so new ones hot
   reload. Appearances (for NPCs) name a body, hair, colours and slider values.
4. **Equipment on the body:** weapons and shields on the hands, and armour
   meshes on the skeleton, replacing the placeholder shapes item by item.
5. **The creator:** bone-scale sliders, hair and colour choices, and the NPC
   generator. The character creation screen gets the sliders.
6. **Morph-target sliders:** body and face shapes made in Blender (with MPFB's
   help), added to the base meshes.
7. **Creatures:** goblins can share the human skeleton (a smaller body with
   their own head). Rabbits and other animals need their own models, and
   Quaternius's CC0 animal packs are a place to start.

## Open questions

- **The look:** Quaternius is stylised and clean; MPFB is more realistic.
  Lineage 2 is somewhere between. The spike should try both next to the
  terrain, to settle the style before much art is made.
- **Web download size:** real models and animations are much bigger than
  today's shapes. glTF compression and sharing parts (one skeleton, reused
  animations) keep it down; the spike will tell us how much it matters.
