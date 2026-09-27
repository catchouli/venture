# Fishing: the implementation plan

The design is `design_docs/fishing_skill/`: `fishing.md` (the whole skill,
to level 70), `fishing_slice.md` (the first playable part, levels 1–20,
which this plan builds) and `fish_reference.md` (the pool of species). This
is how it goes into the game.

## The slice, in one paragraph

In the grassland round the starting town, a player with a rod equipped
clicks water within the rod's reach to cast. Fish shadows move in the water
(their own: each player sees what's there for them), solid if they can
catch them, faint if they'll be able to soon. A fish that likes the bait
comes to it; the rod tip or float shows a nibble, then a bite, and the
player strikes in time (lures hook themselves). Then the fight: reeling in
raises the line's tension (too much and it snaps), letting it run pays out
line (too far and the spool runs out), and a tiring fish is the moment to
reel. Landed, it's in the bag with its experience. Traps, baited and left on
a bank, fill up over twenty minutes. A tackle vendor in town sells gear and
bait, and buys fish.

## Decisions (from the review)

See "Decisions from review" in `fishing.md`. In short: fantasy names and
the game's biomes; skills level to 99, characters to 60; three items a
method (rod, reel, bait or lure); each player's own shadows; traps visible
to all but only their owner's, soaking while they're away; the server
decides everything, with the fight as shared code the client runs ahead;
fish sell, and will cook into food.

## How it fits the code

- **Rules** (`venture_rpg::fishing`, small tested functions, no Bevy): which
  fish a bait or lure attracts (trait matching), bite chances and timings,
  whether a hook can take a fish (hook size against fish size), whether a
  reel can land it, and the XP. Shared by the server (which decides) and
  the client (which shows).
- **The fight** (`venture_simulation::fishing`, like movement): a small,
  deterministic simulation stepped at the server's tick rate from the
  player's input (reeling or not) and a seed the server chooses: the
  fish's pulls and runs, how tired it is, the tension, the line out. The
  client runs it ahead so reeling feels instant; the server runs the same
  steps and its outcome is final (and corrects the client if they part).
- **Content** (RON in `content/`, checked by the content check, editable in
  the editor): `fish/` (species: name, size, level, XP, water, traits,
  value, icon), fishing gear as items with a `fishing` part (rod: method,
  tier, reach; reel: tier, line strength, capacity; bait: diet, hook size;
  lure: trait profile; trap), `fishing_spots.ron` if spots need more than
  "this kind of water in this biome", and the tackle vendor's shop.
- **The world**: fishable water is what the baked cells say. The bake gains
  each water cell's **kind** (river, lake or sea: the generator already
  tells them apart), which says fresh or salt and which fish live there
  (with the biome). Chunk format version up; bake again. (Done:
  `Fluid::body`, chunk format 2.)
- **Server** (`fishing.rs`, the only code that changes fishing state, as
  `combat.rs` is for fights): a player's fishing goes through stages: cast
  (checked: reach, water, gear, level) → waiting (bites scheduled from a
  seeded random number generator, by what the bait attracts here) →
  nibbles and a bite (the strike judged against when the bite happened,
  allowing for network delay: windows no tighter than about a third of a
  second) → the fight → landed (the fish through `Inventory::apply`, the XP
  through `Character`'s skill methods) or lost. Moving, attacking or
  another cast ends it. Traps are a separate part of `fishing.rs` (placed,
  soaking, full; saved in the profile). Requests come through the gate
  (`requests.rs`) as a `FishingRequest`.
- **Client** (a `fishing/` concern): with a rod equipped, clicking water in
  reach casts (the cursor shows it); the line, float and rod-tip bending;
  the bite indicator; the fight's two bars (tension, line out) with reeling
  on a held button or key; shadows drawn in the water from what the server
  says is there for this player; sounds; the catch in chat and the bag.
- **Skills to 99**: `Progress` takes its cap from what it's counting
  (characters 60, skills 99).

## Steps

Each step ends playable or testable, and is committed.

1. **Done: water that looks right.** The spiky water edges along rivers
   get fixed first: fishing puts them in front of the player's eyes. (Soft
   shores from depth baked into the water mesh, and rivers covering the low
   ground beside them; see "Water" in `textures.md`.)
2. **Done: rules and data.** Species, bait, lures, rods and reels as
   content (`content/items/fish.ron` and `fishing.ron`, with placeholder
   icons); attraction, bites, hooking and landing as tested rules
   (`venture_rpg::fishing`); skills to 99; water kinds in the bake.
3. **The loop, plainly shown.** Server stages, protocol, and a client that
   shows it with simple indicators (text and bars, a float as a marker):
   cast, bite, strike, fight, land. Enough to answer the slice's first
   questions (does the strike timing read? does the fight make sense?)
   before any polish.
4. **Shadows.** What's in the water for each player, and how it's drawn
   (solid, faint), moving and gathering round the bait.
5. **Looks and feel.** The line and float, the rod bending, splashes, fight
   animation, sounds, icons for each fish (the placeholder icon tool).
6. **Traps.** Placing, soaking (saved), harvesting, bycatch.
7. **Content and vendor.** The slice's fish, bait and gear; the starter kit;
   the tackle vendor in town; the lure static profiles.
8. **Playtest tools.** Logging catches per hour, XP per hour and losses by
   cause; admin commands (set a skill's level, give gear); every number in
   content, so it can be tuned without code.

## What the slice answers

From `fishing_slice.md`: does the strike timing read (and is it fair over
the network)? Is the fight understandable (tension against the spool)? Do
faint shadows motivate? Do players get trait matching? Is the trap loop
worth it? Is level 20 in about two to two and a half hours?
