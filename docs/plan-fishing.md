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
3. **Done: the loop, plainly shown.** Server stages, protocol, and a client
   that shows it with simple indicators (text and bars, a float as a
   marker): cast, bite, strike, fight, land. Enough to answer the slice's
   first questions (does the strike timing read? does the fight make
   sense?) before any polish. To try it: `/give ledger_rod`, `/give
   wooden_reel`, `/give worm 20` (admins), take the rod in hand from the
   bag, and click the water. The client runs the fight ahead, so reeling
   shows at once; the server replays changes of reeling from the tick they
   were made (up to half a second late). (Others see your line and float:
   done with step 5.)
4. **Done: shadows.** What's in the water for each player, and how it's
   drawn (solid, faint), moving and gathering round the bait. The water is
   divided into 12-metre regions; which fish shoal in each, and how many,
   changes with the hour of the day, and is worked out from the world's
   seed (`venture_simulation::fishing::shoals`) by both the server and the
   client, so it takes no messages and each client shows its own player's
   view. A line cast into a region where a fish shoals makes that fish three
   times as likely to bite (and bite sooner): shadows show where to cast.
5. **Looks and feel.** The line and float, the rod bending, splashes, fight
   animation, sounds, icons for each fish (the placeholder icon tool).
6. **Done: traps.** Placing, soaking (saved), harvesting, bycatch. Use a
   cage trap from the bag at the water's edge (it takes shrimp bits, or
   other bait crayfish and crabs go for, if you carry some); it's saved with
   you and soaks by the real clock (20 minutes to fill its 3 catches), and
   everyone sees it while you're in the game. Select it and click it again
   to collect: catches are drawn from what traps catch in that water at
   your level, a quarter of them bycatch, those the bait draws three times
   as likely; the trap comes back to the bag. One trap a player.
7. **Content and vendor.** The slice's fish, bait and gear; the starter kit;
   the tackle vendor in town; the lure static profiles. (Done for rods:
   new characters start with a ledger rod, a reel and worms, and Wendel,
   by the town square, sells the slice's tackle, buys fish and explains
   how to fish. The kit has a cage trap and shrimp bits too.)
8. **Playtest tools.** Logging catches per hour, XP per hour and losses by
   cause; admin commands (set a skill's level, give gear); every number in
   content, so it can be tuned without code. (Done: the server logs a line
   for each outcome, such as `Fishing: Wynra (Ledger, worm, level 10):
   landed dace 21.9 s after casting, fought 6.9 s.`, so `grep Fishing:`
   gives catches, losses by cause, waits and fights; `/setskill fishing 20`
   and `/give ITEM [COUNT]`. Not yet: the rules' numbers (bite waits, strike
   windows, fight) in `server.ron` or content.)

## What the slice answers

From `fishing_slice.md`: does the strike timing read (and is it fair over
the network)? Is the fight understandable (tension against the spool)? Do
faint shadows motivate? Do players get trait matching? Is the trap loop
worth it? Is level 20 in about two to two and a half hours?
