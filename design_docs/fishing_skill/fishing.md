# Fishing Skill — Design Document

> Runescape-style gathering skill. Max level 99; this document designs **levels 1–70**, with 71–99 reserved for future growth on the same curve.

## Design Pillars

- Simple to start, long tail to master.
- Each method has a distinct feel: where you fish, what you catch, how active you are.
- Environments double as progression: rivers → lakes → estuary → coast → open sea.

---

## XP System

XP needed to go from `level` to the next:

```rust
pub fn experience_to_next_level(level: u32) -> u64 {
    (100.0 * f64::from(level.max(1)).powf(1.5)).round() as u64
}
```

- Level 1→2 costs 100 XP, 2→3 costs 283, 10→11 costs 3,162.
- Total XP to reach level 70 ≈ **1.67M**; level 99 ≈ **3.85M**.

### XP Table (milestone levels)

| Level reached | XP for previous level | Cumulative XP to reach it |
|---:|---:|---:|
| 2 | 100 | 100 |
| 6 | 1,118 | 2,821 |
| 10 | 2,700 | 11,106 |
| 11 | 3,162 | 14,268 |
| 16 | 5,809 | 37,807 |
| 26 | 12,500 | 131,309 |
| 46 | 30,187 | 558,537 |
| 56 | 40,789 | 917,843 |
| 66 | 52,405 | 1,388,822 |
| 71 | 58,566 | 1,669,239 |
| 99 | — | ~3,851,616 |

Levels **71–99**: reserved for future growth; same formula applies, no new content designed yet.

---

## Methods & Unlocks

| Method | Gear | Unlock | Entry difficulty | Throughput | Style |
|---|---|---:|---|---|---|
| Ledgering | Rod, line, reel, hooks, weights, bait | 1 | Easiest | Low | Active-ish |
| Cage traps | Small cages (coast & riverbank) | 1 | Easy | Low | Passive |
| Float fishing | Rod, line, reel, hooks, float, bait | 5 | Easy | Low | Active |
| Lures | Rod, line, reel, lures | 15 | Easy start, high skill ceiling | Medium | Very active |
| Cast/small nets | Small nets (thrown by hand) | 25 | Medium | High | Active, twitch-based |
| Fly fishing | Fly rod, line, reel, flies | 45 | Hardest entry | High | Active, technical |
| Trawling | Boat + big net | 55 | Medium | Very high | Crew of 2–4, boat |
| Deep-sea pots | Big pots + boat | 70 | Medium | Medium | Passive + boat haul |

---

## Rod & Fly Fishing Mechanics

> Applies to ledgering, float, lures, and fly — the casting methods. Nets and traps key off shadows too, but have their own loops.

### Where You Can Fish

| Method | Rivers | Lakes | Sea |
|---|:-:|:-:|:-:|
| Ledgering | ✓ | ✓ | ✓ |
| Cage traps | ✓ | ✓ | ✓ (coast & riverbank) |
| Float fishing | ✓ | ✓ | ✓ |
| Lure fishing | ✓ | ✓ | ✓ |
| Fly fishing | ✓ | — | — |
| Cast nets | — | — | ✓ (sea shoals) |
| Trawling | — | — | ✓ (open sea — boat, crew) |
| Deep-sea pots | — | — | ✓ (deep sea — boat) |

### Lures & Flies

Lures and flies are **player-crafted and randomly generated** — every one that comes off the crafting bench is slightly different:

- **Stats roll on craft:** each lure or fly attracts a different set of fish (a group like "predators" or "surface shoalers", sometimes just one species) and carries technique requirements — retrieve speed, twitch timing, depth band. The same recipe can produce a dud or a gem.
- **Technique matters:** a lure that attracts the right fish is useless in the wrong hands — using it effectively demands its technique, so mastery depth comes from both the item and the player.
- **Player-driven value:** because rolls vary, the market decides what's worth money. Crafters flood the market with average lures; anglers pay premiums for god-rolls on the fish they chase. Trading and price knowledge becomes part of the endgame.
- **Still never wear out** (see Gear & Economy) — which is exactly why a great roll is a permanent asset and a dud is landfill.
- **Specialist flies** (one specific fish, strongly attracted) remain the prestige end: quest loot or jackpot-tier craft rolls.

Attraction is expressed in **trait tags**, not fish names: a rolled lure profile like *fish-eater · aggressive · midwater · flashy* automatically attracts everything matching it (pike, zander, bass…) — see **Fish Traits & Attraction** below. Roll ranges are a balancing-pass TBD.

### Fish Shadows

Fish appear in the water as **shadows**. What you see depends on your level versus each fish's unlock level — low-XP fish are easy to spot everywhere, high-XP fish fade in as you earn them:

| Your level vs fish unlock | Shadow | Catchable? |
|---|---|---|
| At or above unlock | **Solid** — full-size, distinct | Yes |
| Within 10 levels below unlock | **Faint** — translucent, smaller (gap tunable) | No — a preview of what the spot holds |
| More than 10 below | **Invisible** | — |

A fresh level-1 character therefore always sees their biome's starter fish (lv 1–11 unlocks are solid from day one) — there is always something to fish — while a legendary pool looks nearly empty to them and fills in as they level. Since XP scales with unlock level in every biome, richer shadows literally mark better spots.

- **Casting methods:** shadows tell you where to cast.
- **Cast nets:** shadow *clusters* are the target — an accurate throw into a dense shoal is what makes the method fast.
- **Traps & pots:** shadow clusters inform placement.
- **Fly fishing:** the fish are much easier to scare off — approach, presentation, and shadow awareness matter more than anywhere else.

### Gear Components

| Component | What it affects |
|---|---|
| Rod | How far you can cast |
| Bait | Which fish are attracted — **consumable, one per cast**; bait maps to diet groups (worms → insect-eaters, live fish → fish-eaters, sweetcorn → herbivores) |
| Hook size | Max bait size that fits + what size fish can be hooked — small hook: mostly small fish, rare chance of larger; big hook: only larger fish |
| Line strength | Biggest fish that can be pulled in — too weak and a hooked fish snaps the line before landing (5 tiers — see Gear Tiers) |
| Reel size | How much line is stored — more line = a running fish can go further before the spool runs out (5 tiers — see Gear Tiers) |
| Float | Lets you set the depth of the hook — one universal float, no tiers |
| Weight | Sinks the bait for ledgering — one universal weight, no tiers |
| Lure type | What is attracted, and how the lure is used — **lures and flies never wear out**; each is a unique randomly-rolled craft (see Lures & Flies) |

### Loadouts

- **Ledgering:** rod, line, reel, hooks, weights, bait
- **Float fishing:** rod, line, reel, hooks, float, bait
- **Lure fishing:** rod, line, reel, lure — no hooks, no bait
- **Fly fishing:** fly rod, line, reel, flies — no hooks, no bait. Line strength and reel capacity apply exactly like the other rod methods: a running surface fish can spool you out.

### Gear Tiers

Rods, lines and reels each have **5 tiers** (placeholder names Tier 1–5), gated on Fishing level, evenly spaced to 70.

**Rods** — one ladder per method. Tier 1 requires the method's unlock level. Tiers improve **cast distance only** — pacing comes from fish choice, not gear speed.

| Rod | T1 | T2 | T3 | T4 | T5 |
|---|---:|---:|---:|---:|---:|
| Ledger rod | 1 | 18 | 36 | 53 | 70 |
| Float rod | 5 | 21 | 38 | 54 | 70 |
| Lure rod | 15 | 29 | 43 | 56 | 70 |
| Fly rod | 45 | 51 | 58 | 64 | 70 |

**Line & reel** — shared ladders across all rod methods (not per-method). They gate the same goal from two angles, and you need **both** at tier X to reliably land size-X fish:

| Tier | Level | Line strength lands… | Reel capacity survives… |
|---:|---:|---|---|
| T1 | 1 | Tiny & Small fish | short runs |
| T2 | 18 | Med fish | medium runs |
| T3 | 36 | Large fish | long runs |
| T4 | 53 | Trophy fish | long, hard runs |
| T5 | 70 | **Legend fish** (the 2,000+ XP Trophies) | everything |

- Too-weak **line**: the fish can be hooked but snaps you before landing — a *tension* failure.
- Too-small **reel**: the fish runs past your spool and escapes even on strong line — a *spool-out* failure.
- **Nets, traps and pots have no line** — they bypass these tiers entirely; their gate is the method level (net/pot tiers reserved for 71–99).
- **Hooks, floats and weights are not level-gated.** Hooks come in sizes (tiny → big) — pick the size that fits your bait and the fish you're after. There is exactly **one float** and **one weight** — universal utility items with no tiers and no choices to make.

### Gear & Economy

All fishing gear — rods, lines, reels, hooks, floats, weights, lures, flies, nets, traps, pots and boats — is **player-crafted** through other skills (woodworking, smithing, etc.). Recipes and tier costs live with those skills, not in this document. Lures and flies never wear out; they're simply priced as premium items to balance their permanence.

### Bite Indicators & Hooking

The player casts; the hook (bait / lure / float) flies out and lands in the water. Fish shadows wander underneath.

| Method | Indicator | Hooking |
|---|---|---|
| Ledgering | End of the rod — slight tug on a nibble | Manual press/click; too early or too late misses |
| Float fishing | Zoomed-in float — tugs on a bite | Manual press/click; too early or too late misses |
| Lures | End of rod pulled hard when a fish strikes | Automatic — if the fish goes for the lure, it's hooked |
| Fly | Fly sits on top of the water — rod pulled hard when a fish strikes | Automatic — same as lures |

A missed hook (too early or too late) means the fish runs away **and the bait is gone** — reel in, attach new bait, cast again.

### The Fight

Once hooked, the fish fights and tries to run:

- **Reeling** pulls the fish in but raises tension — too much and the **line snaps**.
- **Not reeling** (giving slack) lets the fish swim further — if it exceeds the line on the reel, the fish gets away.
- The fish **tires** eventually — that's the moment to capitalize and reel it in.
- Once close enough, the fish is landed: into inventory, XP awarded.

### XP

A **single fishing XP pool**. XP is a property of each fish — catch a fish, gain its XP. Methods shape XP/hour by changing *which* fish you can reach, not by multiplying XP (see Fish & Biomes for every value).

---

## Method Notes

> Ledgering, float, lures, and fly share one core loop — cast → indicator → hook → fight → land. See **Rod & Fly Fishing Mechanics** above.

### Ledgering (lv 1)
Bait on the bottom with a weight — the "training method." No fuss, any water deep enough for a sinker.

### Float fishing (lv 5)
Same rod but a float lets you dangle bait at *any* depth — opens fish that live midwater or near the surface.

### Lures (lv 15)
Not hard to start, but a high skill ceiling: retrieve speed, twitch timing, and lure selection matter. The rod method with the most mastery depth.

### Cast / small nets (lv 25)
Thrown by hand into the water. Requires an **accurate cast** and **lots of fish in the vicinity** — wrong spot, wrong timing, no catch. In return: a lot of fish very quickly.

### Fly fishing (lv 45)
Plays like lure fishing, but the fly is presented **on top of the water**. Fish are much easier to scare off and harder to attract — stealth, approach, and fly selection matter. **Rivers only.** Late unlock; strongest per-catch rewards.

### Trawling (lv 55)
Big nets on boats. Slow-moving, very high yield. **Crew of 2–4 required** — cannot be done solo; hauling and sorting is a light co-op minigame (not designed yet).

### Cage traps (lv 1)
Small cages set along the shore **or on riverbanks** — the low-level **passive** method. Low effort: bait, set, come back later. The catch grows with you — shrimp, shore crabs and whelks on the coast; river snails, crayfish, river crabs, prawns and mussels in rivers; velvet and edible crabs as you level.

### Deep-sea pots (lv 70)
Large pots dropped into deeper water. Requires a boat to place and pull back up; bridges into open-sea fishing.

---

## Leveling Pace

Target: a focused player reaches **level 70 in ~30 hours** (1,669,239 XP, ~55k XP/h average).

| Band | XP needed | Hours | XP/h target | Main methods |
|---|---:|---:|---:|---|
| 1→11 | 14,268 | 0.75 | ~19k | Ledgering rivers + traps (passive) |
| 11→26 | 117,041 | 3.25 | ~36k | Float, first lures, cast nets (25+) |
| 26→46 | 427,228 | 9.0 | ~47k | Lure & cast nets — coast |
| 46→56 | 359,306 | 5.5 | ~65k | Fly, first trawling |
| 56→71 | 751,396 | 11.5 | ~65k | Trawling, late lure/fly, traps |

**Catch cadence assumptions** (to tune in playtest):

| Method | Cadence |
|---|---|
| Ledgering / float | ~140 catches/h |
| Lure | ~85 catches/h |
| Cast net | ~50 throws/h × 4–8 fish per throw |
| Fly | ~100 catches/h |
| Trawling | ~7.5 hauls/h × 15–25 fish per haul |
| Cages / pots | ~3 harvests/h (passive) |

**Derivation rule:** fish XP ≈ band XP/h ÷ method catches/h.

**Balancing notes**
- XP is a property of each fish; there are no method multipliers.
- Bigger fish bite less often — a 750 XP carp might bite ~50 times/hour, not 140. XP/h roughly evens out across tiers.
- Passive methods (cages, pots) earn less per hour by design — they run unattended while the player does something else.

---

## Fish & Biomes

> Every value here is placeholder-tunable. **Size** drives hook size (what can be hooked) and — for rod-caught fish — the minimum line & reel tier (see Gear Tiers). Every fish also carries the trait tags from **Fish Traits & Attraction** (diet, temperament, depth, shape, schooling, activity, color). Water types are simple: **freshwater** = rivers & lakes, **saltwater** = sea — nothing lives in both. The full species pool lives in `fish_reference.md`.

### Fish Traits & Attraction

Every fish carries a small set of trait tags — the shared vocabulary that bait, lures, flies, cast nets and traps all "read":

| Trait | Groups | What it drives |
|---|---|---|
| Diet | Insect-eater · Fish-eater · Bottom-feeder · Crustacean-eater · Plankton-feeder · Herbivore · Scavenger | Bait choice; lure & fly attraction |
| Temperament | Aggressive · Opportunist · Skittish | How hard a fish commits to a lure; how much stealth/presentation matters |
| Depth band | Bottom · Midwater · Surface | What can physically reach it — ledger bottom, float any depth, lure midwater retrieves, fly surface |
| Size | Tiny → Trophy (existing classes) | Matching lure/fly/hook size — big fish ignore tiny presentations |
| Shape | Torpedo · Deep-bodied · Eel-like · Flat | Fight flavor: torpedo = long runs (reel stress), deep-bodied = violent head-shakes (line stress) |
| Schooling | Shoaling · Loose groups · Solitary | Cast nets need shoaling fish; drives shadow clusters |
| Activity | Day · Night · Dawn/dusk | When the fish plays — night-only fish exist |
| Color response | Hits flashy/bright · Prefers natural/muted | Lure finish selection — aggressive fish chase flash, skittish fish spook from it |

**What each method reads:**

| Method | Traits it keys on |
|---|---|
| Bait (ledger & float) | Diet — what you offer · depth band · activity window |
| Lures | Diet · temperament · depth band · size match · color response |
| Flies | Diet (surface insects) · temperament — skittish fish demand perfect presentation |
| Cast nets | Schooling — shoaling fish only · size vs mesh |
| Traps & pots | Scavenger / crustacean-eater · activity window |

**All fish, tagged** — one row per species; sizes live in the biome tables.

Codes follow the Groups column above: Insect · Fish · Bottom · Crust · Plank · Herb · Scav — Aggr · Opp · Skitt — Bot · Mid · Surf — Torp · Deep · Eel · Flat — Shoal · Loose · Sol — Day · Night · Dawn/Dusk — Flash · Nat · —

| Fish | Diet | Temp. | Depth | Shape | School | Activity | Color |
|---|---|---|---|---|---|---|---|
| **Temperate rivers** | | | | | | | |
| Dace | Insect | Skitt | Mid | Torp | Shoal | Day | Nat |
| Gudgeon | Bottom | Opp | Bot | Torp | Loose | Day | Nat |
| River snail | Scav | Opp | Bot | — | Loose | Night | — |
| White-clawed crayfish | Scav | Opp | Bot | — | Sol | Night | — |
| Freshwater mussel | Plank | Opp | Bot | — | Shoal | Day | — |
| Roach | Insect | Skitt | Mid | Deep | Shoal | Day | Nat |
| Signal crayfish | Scav | Opp | Bot | — | Sol | Night | — |
| Chub | Insect | Opp | Mid | Torp | Loose | Day | Nat |
| Red swamp crayfish | Scav | Opp | Bot | — | Loose | Night | — |
| Perch | Crust | Aggr | Mid | Deep | Loose | Day | Flash |
| Noble crayfish | Scav | Opp | Bot | — | Sol | Night | — |
| Bream | Bottom | Opp | Bot | Deep | Shoal | Night | Nat |
| Eel | Scav | Opp | Bot | Eel | Sol | Night | — |
| Chinese mitten crab | Scav | Opp | Bot | — | Shoal | Night | — |
| Zander | Fish | Aggr | Mid | Torp | Loose | Night | Nat |
| Barbel | Bottom | Opp | Bot | Torp | Loose | Day | Nat |
| Pike | Fish | Aggr | Mid | Torp | Sol | Day | Flash |
| Brown trout | Insect | Skitt | Mid | Torp | Sol | Dawn/Dusk | Nat |
| Grayling | Insect | Skitt | Mid | Torp | Loose | Day | Nat |
| Sea trout | Fish | Skitt | Mid | Torp | Loose | Night | Nat |
| Salmon | Fish | Aggr | Mid | Torp | Sol | Day | Flash |
| **Temperate lakes** | | | | | | | |
| Crucian carp | Bottom | Skitt | Bot | Deep | Shoal | Dawn/Dusk | Nat |
| Rainbow trout | Insect | Aggr | Mid | Torp | Loose | Day | Flash |
| Tench | Bottom | Skitt | Bot | Deep | Loose | Night | Nat |
| Common carp | Bottom | Skitt | Bot | Deep | Loose | Dawn/Dusk | Nat |
| Arctic charr | Insect | Opp | Mid | Torp | Loose | Day | Flash |
| Wels catfish | Fish | Aggr | Bot | Eel | Sol | Night | — |
| **Temperate coast & shallows** | | | | | | | |
| Grey mullet | Herb | Skitt | Surf | Deep | Shoal | Day | Nat |
| Mackerel | Plank | Aggr | Mid | Torp | Shoal | Day | Flash |
| Flounder | Scav | Opp | Bot | Flat | Sol | Day | Nat |
| Whitebait | Plank | Opp | Mid | Torp | Shoal | Day | — |
| Smelt | Plank | Opp | Mid | Torp | Shoal | Dawn/Dusk | — |
| Sprat | Plank | Opp | Mid | Torp | Shoal | Day | — |
| Garfish | Fish | Aggr | Surf | Torp | Shoal | Day | Flash |
| Sandeel | Plank | Opp | Mid | Torp | Shoal | Day | — |
| Sardine | Plank | Opp | Mid | Torp | Shoal | Day | Flash |
| Herring | Plank | Opp | Mid | Torp | Shoal | Day | — |
| Scad | Plank | Opp | Mid | Torp | Shoal | Night | — |
| Whiting | Scav | Opp | Bot | Torp | Shoal | Night | Nat |
| Plaice | Crust | Opp | Bot | Flat | Sol | Dawn/Dusk | Nat |
| Sea bass | Fish | Aggr | Mid | Torp | Loose | Day | Flash |
| Pollack | Fish | Aggr | Mid | Torp | Loose | Day | Flash |
| Ballan wrasse | Crust | Aggr | Bot | Deep | Sol | Day | Flash |
| Lesser spotted dogfish | Scav | Opp | Bot | Torp | Loose | Night | Nat |
| Cod | Scav | Opp | Bot | Torp | Loose | Dawn/Dusk | Nat |
| Conger eel | Scav | Aggr | Bot | Eel | Sol | Night | — |
| Brown shrimp | Scav | Opp | Bot | — | Shoal | Night | — |
| Shore crab | Scav | Opp | Bot | — | Sol | Night | — |
| Common whelk | Scav | Opp | Bot | — | Loose | Night | — |
| Velvet crab | Scav | Aggr | Bot | — | Sol | Night | — |
| Edible crab | Scav | Opp | Bot | — | Sol | Night | — |
| Common lobster | Scav | Opp | Bot | — | Sol | Night | — |
| **Northern rivers & lakes** | | | | | | | |
| Rusty crayfish | Scav | Opp | Bot | — | Sol | Night | — |
| Burbot | Scav | Opp | Bot | Eel | Sol | Night | — |
| Arctic grayling | Insect | Skitt | Mid | Torp | Loose | Day | Nat |
| Arctic char | Insect | Opp | Mid | Torp | Loose | Day | Flash |
| Lake trout | Fish | Aggr | Bot | Torp | Sol | Day | Nat |
| Inconnu | Fish | Aggr | Mid | Torp | Shoal | Day | Flash |
| Northern pike (giant) | Fish | Aggr | Mid | Torp | Sol | Day | Flash |
| Taimen | Fish | Aggr | Mid | Torp | Sol | Dawn/Dusk | Flash |
| **Northern seas** | | | | | | | |
| Capelin | Plank | Opp | Mid | Torp | Shoal | Night | — |
| Sea-run arctic char | Fish | Aggr | Mid | Torp | Shoal | Day | Flash |
| Pacific salmon | Fish | Aggr | Mid | Torp | Shoal | Day | Flash |
| Northern shrimp | Plank | Opp | Bot | — | Shoal | Night | — |
| Lumpfish | Scav | Opp | Bot | Deep | Loose | Day | — |
| Redfish | Plank | Opp | Mid | Deep | Shoal | Day | — |
| Wolffish | Crust | Aggr | Bot | Eel | Sol | Day | Nat |
| Greenland halibut | Scav | Opp | Bot | Flat | Sol | Day | — |
| Atlantic halibut | Fish | Aggr | Bot | Flat | Sol | Day | Nat |
| Snow crab | Scav | Opp | Bot | — | Shoal | Night | — |
| King crab | Scav | Opp | Bot | — | Loose | Night | — |
| **Tropical rivers** | | | | | | | |
| Rice-paddy crab | Scav | Opp | Bot | — | Loose | Night | — |
| Piranha | Fish | Aggr | Mid | Deep | Shoal | Day | Flash |
| African tigerfish | Fish | Aggr | Mid | Torp | Shoal | Day | Flash |
| Redtail catfish | Scav | Opp | Bot | Eel | Sol | Night | — |
| Arowana | Fish | Aggr | Surf | Torp | Sol | Day | Flash |
| Amazon river prawn | Scav | Opp | Bot | — | Shoal | Night | — |
| Peacock bass | Fish | Aggr | Mid | Deep | Loose | Day | Flash |
| Giant snakehead | Fish | Aggr | Mid | Torp | Sol | Day | Flash |
| Golden dorado | Fish | Aggr | Mid | Torp | Loose | Day | Flash |
| Nile perch | Fish | Aggr | Mid | Torp | Sol | Day | Nat |
| Mahseer | Herb | Opp | Mid | Torp | Loose | Day | Flash |
| Giant river prawn | Scav | Opp | Bot | — | Sol | Night | — |
| Electric eel | Fish | Aggr | Mid | Eel | Sol | Night | — |
| Goonch catfish | Fish | Aggr | Bot | Eel | Sol | Night | — |
| Goliath tigerfish | Fish | Aggr | Mid | Torp | Sol | Day | Flash |
| Arapaima | Fish | Aggr | Surf | Torp | Sol | Day | Nat |
| Mekong giant catfish | Plank | Opp | Bot | Eel | Sol | Day | Nat |
| **Tropical coast & reef** | | | | | | | |
| Anchovy | Plank | Opp | Mid | Torp | Shoal | Night | — |
| Sergeant major | Herb | Aggr | Mid | Deep | Shoal | Day | Flash |
| Mangrove snapper | Fish | Opp | Bot | Torp | Loose | Night | Nat |
| Great barracuda | Fish | Aggr | Mid | Torp | Loose | Day | Flash |
| Coral trout | Fish | Aggr | Mid | Deep | Sol | Day | Flash |
| Red snapper | Fish | Opp | Bot | Deep | Shoal | Dawn/Dusk | Nat |
| Grouper | Fish | Opp | Bot | Deep | Sol | Day | Nat |
| Giant trevally | Fish | Aggr | Mid | Deep | Loose | Day | Flash |
| Mud crab | Scav | Aggr | Bot | — | Sol | Night | — |
| Spiny lobster | Scav | Opp | Bot | — | Loose | Night | — |
| **Australasia** | | | | | | | |
| Australian bream | Crust | Opp | Bot | Deep | Loose | Dawn/Dusk | Nat |
| Australian bass | Fish | Aggr | Mid | Torp | Loose | Night | Flash |
| Golden perch | Fish | Opp | Mid | Deep | Loose | Night | Flash |
| Flathead | Fish | Aggr | Bot | Flat | Sol | Day | Nat |
| Barramundi | Fish | Aggr | Mid | Torp | Sol | Night | Nat |
| Mangrove jack | Fish | Aggr | Bot | Deep | Sol | Day | Flash |
| Snapper | Crust | Opp | Bot | Deep | Shoal | Dawn/Dusk | Nat |
| Murray cod | Fish | Aggr | Bot | Deep | Sol | Night | Nat |
| Mulloway | Fish | Opp | Bot | Torp | Loose | Night | Nat |
| Yellowtail kingfish | Fish | Aggr | Mid | Torp | Shoal | Day | Flash |
| NZ whitebait | Plank | Opp | Mid | Torp | Shoal | Day | — |
| Yabby | Scav | Opp | Bot | — | Loose | Night | — |
| Redclaw crayfish | Scav | Opp | Bot | — | Sol | Night | — |
| Spiny (Murray) crayfish | Scav | Opp | Bot | — | Sol | Night | — |
| Marron | Scav | Opp | Bot | — | Sol | Night | — |
| Southern rock lobster | Scav | Opp | Bot | — | Loose | Night | — |
| **Open ocean** | | | | | | | |
| Dab | Scav | Opp | Bot | Flat | Loose | Day | Nat |
| Haddock | Crust | Opp | Bot | Torp | Shoal | Day | Nat |
| Dover sole | Scav | Opp | Bot | Flat | Sol | Night | Nat |
| Turbot | Fish | Aggr | Bot | Flat | Sol | Day | Nat |
| Hake | Fish | Aggr | Mid | Torp | Shoal | Night | Nat |
| Ling | Crust | Opp | Bot | Eel | Sol | Day | Nat |
| Monkfish | Fish | Aggr | Bot | Flat | Sol | Night | — |
| Langoustine | Scav | Opp | Bot | — | Sol | Night | — |

One row per species — fish caught by two methods (plaice, cod) or two life phases (arctic char) list once. Trap content skips shape & color. Night dominates the catfish, cod family and snappers — this pass makes the game's **night-fishing** content visible for the first time.

### Temperate rivers
*The classic leveling river — European flavor.*

| Fish | Size | Method | lv | XP |
|---|---|---|---:|---:|
| Dace | Small | Ledger | 1 | 110 |
| Gudgeon | Tiny | Ledger | 3 | 125 |
| River snail | Tiny | Cage — bycatch | 3 | 60 |
| White-clawed crayfish | Small | Cage | 5 | 100 |
| Freshwater mussel | Small | Cage — bycatch (pearl mussel: quest) | 6 | 70 |
| Roach | Small | Float | 7 | 150 |
| Signal crayfish | Small | Cage | 8 | 150 |
| Chub | Small–Med | Float | 10 | 190 |
| Red swamp crayfish | Small–Med | Cage | 12 | 180 |
| Perch | Small–Med | Float / Lure | 13 | 260 |
| Noble crayfish | Small–Med | Cage | 18 | 250 |
| Bream | Med | Float | 18 | 330 |
| Eel | Med | Ledger / Float — night only | 28 | 380 |
| Chinese mitten crab | Small–Med | Cage — autumn run | 32 | 500 |
| Zander | Large | Lure — night | 36 | 550 |
| Barbel | Large | Ledger | 38 | 450 |
| Pike | Large–Trophy | Lure | 38 | 700 |
| Brown trout | Small–Large | Fly | 45 | 550 |
| Grayling | Med | Fly | 48 | 650 |
| Sea trout (run) | Med–Large | Fly / Lure | 52 | 900 |
| Salmon (run) | Large–Trophy | Fly | 60 | 1,300 |

*Sea trout and salmon appear during seasonal river runs — see Events.*

### Temperate lakes
*Stillwater fishing — European flavor with NA sportfish.*

| Fish | Size | Method | lv | XP |
|---|---|---|---:|---:|
| Crucian carp | Small | Float | 10 | 210 |
| Rainbow trout (stocked) | Med | Lure | 20 | 400 |
| Tench | Med | Float / Ledger | 22 | 350 |
| Common carp | Large | Ledger — slow bites | 40 | 750 |
| Arctic charr | Small–Med | Lure — deep water | 50 | 800 |
| Wels catfish | Trophy | Ledger — live bait | 55 | 1,500 |

### Temperate coast & shallows
*Shore, piers and rock marks — European flavor.*

| Fish | Size | Method | lv | XP |
|---|---|---|---:|---:|
| Grey mullet | Med | Float / Ledger | 20 | 300 |
| Mackerel | Small–Med | Float | 26 | 330 |
| Flounder | Small–Med | Ledger | 24 | 300 |
| Whitebait | Tiny | Cast net | 25 | 120 |
| Smelt | Small | Cast net | 26 | 140 |
| Sprat | Tiny | Cast net | 28 | 140 |
| Garfish | Med | Float | 30 | 350 |
| Sandeel | Tiny–Small | Cast net | 31 | 130 |
| Sardine (pilchard) | Small | Cast net | 33 | 160 |
| Herring | Small–Med | Cast net | 36 | 180 |
| Scad (horse mackerel) | Small–Med | Cast net | 40 | 220 |
| Whiting | Small–Med | Ledger | 30 | 350 |
| Plaice | Small–Med | Ledger | 33 | 400 |
| Sea bass | Med–Large | Lure — roams up rivers | 22 | 450 |
| Pollack | Med–Large | Lure | 32 | 550 |
| Ballan wrasse | Med | Lure | 42 | 650 |
| Lesser spotted dogfish | Small–Med | Ledger | 40 | 480 |
| Cod (shore marks) | Large | Ledger | 52 | 700 |
| Conger eel | Trophy | Ledger — wrecks | 70 | 2,000 |
| Brown shrimp | Tiny | Cage | 1 | 90 |
| Shore crab | Small | Cage — bycatch | 1 | 60 |
| Common whelk | Small | Cage — bycatch | 5 | 80 |
| Velvet crab | Small | Cage | 30 | 200 |
| Edible crab | Med–Large | Cage / Pot | 45 | 450 |
| Common lobster | Large | Pot | 70 | 750 |

### Northern rivers & lakes
*Subarctic fresh water — North American / Siberian flavor.*

| Fish | Size | Method | lv | XP |
|---|---|---|---:|---:|
| Rusty crayfish | Small | Cage | 22 | 220 |
| Burbot | Med | Ledger — night, winter | 45 | 600 |
| Arctic grayling | Med | Fly | 50 | 700 |
| Arctic char | Med–Large | Fly / Lure | 50 | 800 |
| Lake trout | Large | Lure | 55 | 1,000 |
| Inconnu (sheefish) | Large | Lure | 58 | 1,200 |
| Northern pike (giant strain) | Trophy | Lure | 58 | 1,400 |
| Taimen | Trophy | Lure | 62 | 1,800 |

### Northern seas
*Cold open water — North Atlantic / North Pacific flavor.*

| Fish | Size | Method | lv | XP |
|---|---|---|---:|---:|
| Capelin | Tiny | Cast net | 30 | 130 |
| Sea-run arctic char | Med | Fly — river mouths | 52 | 900 |
| Pacific salmon (5 species) | Med–Large | Fly — rivers, seasonal runs | 55–65 | 900–1,800 |
| Northern shrimp | Tiny | Cage | 65 | 100 |
| Lumpfish | Small | Cage — bycatch | 65 | 150 |
| Redfish | Small–Med | Trawl | 62 | 500 |
| Wolffish | Large | Trawl / Ledger | 64 | 900 |
| Greenland halibut | Large | Trawl | 66 | 700 |
| Atlantic halibut | Trophy | Trawl | 69 | 2,500 |
| Snow crab | Med | Pot | 70 | 500 |
| King crab | Large–Trophy | Pot | 70 | 900 |

### Tropical rivers
*Jungle rivers — Amazon, Congo and Mekong flavor.*

| Fish | Size | Method | lv | XP |
|---|---|---|---:|---:|
| Rice-paddy crab | Tiny | Cage | 10 | 120 |
| Piranha | Small | Lure | 15 | 220 |
| African tigerfish | Med | Lure | 30 | 550 |
| Redtail catfish | Large | Ledger | 36 | 550 |
| Arowana | Med | Lure — surface | 34 | 600 |
| Amazon river prawn | Small | Cage | 35 | 300 |
| Peacock bass | Med–Large | Lure | 36 | 800 |
| Giant snakehead | Large | Lure | 40 | 850 |
| Golden dorado | Large | Lure | 44 | 1,000 |
| Nile perch | Trophy | Lure | 54 | 1,100 |
| Mahseer | Large | Ledger / Lure | 50 | 1,200 |
| Giant river prawn | Large | Cage | 45 | 400 |
| Electric eel | Large | Ledger — shock hazard | 55 | 300 |
| Goonch catfish | Trophy | Ledger — live bait | 58 | 1,600 |
| Goliath tigerfish | Large–Trophy | Lure | 70 | 2,000 |
| Arapaima | Trophy | Ledger — live bait | 70 | 2,600 |
| Mekong giant catfish | Trophy | Ledger | 70 | 3,000 |

### Tropical coast & reef
*Warm shallows and coral — worldwide flavor.*

| Fish | Size | Method | lv | XP |
|---|---|---|---:|---:|
| Anchovy | Tiny | Cast net | 25 | 110 |
| Sergeant major & reef tiddlers | Tiny | Cast net | 30 | 120 |
| Mangrove snapper | Med | Lure | 42 | 650 |
| Great barracuda | Large | Lure | 45 | 750 |
| Coral trout | Med | Lure | 50 | 900 |
| Red snapper | Med–Large | Ledger — boat | 52 | 800 |
| Grouper | Large | Ledger — boat | 56 | 1,000 |
| Giant trevally | Large–Trophy | Lure | 60 | 1,500 |
| Mud crab | Large | Pot | 70 | 600 |
| Spiny lobster (crawfish) | Large | Pot | 70 | 700 |

### Australasia
*Rivers, coasts and reefs down under.*

| Fish | Size | Method | lv | XP |
|---|---|---|---:|---:|
| Australian bream | Small–Med | Float / Ledger | 28 | 400 |
| Australian bass | Med | Lure | 30 | 500 |
| Golden perch | Med | Lure | 35 | 650 |
| Flathead | Med–Large | Lure | 38 | 600 |
| Barramundi | Large | Lure | 45 | 1,000 |
| Mangrove jack | Med | Lure | 48 | 800 |
| Snapper | Med–Large | Ledger — boat | 50 | 750 |
| Murray cod | Trophy | Ledger / Lure | 58 | 1,500 |
| Mulloway | Large–Trophy | Ledger — surf | 60 | 1,400 |
| Yellowtail kingfish | Large | Lure — boat | 62 | 1,600 |
| NZ whitebait | Tiny | Cast net — seasonal run | 35 | 250 |
| Yabby | Small | Cage | 15 | 180 |
| Redclaw crayfish | Med | Cage | 25 | 200 |
| Spiny (Murray) crayfish | Med | Cage | 40 | 350 |
| Marron | Med–Large | Cage | 52 | 700 |
| Southern rock lobster | Large | Pot | 70 | 800 |

### Open ocean
*Offshore boat fishing — trawling grounds and deep ledger marks.*

| Fish | Size | Method | lv | XP |
|---|---|---|---:|---:|
| Dab | Small | Trawl | 55 | 300 |
| Plaice | Small–Med | Trawl | 55 | 400 |
| Haddock | Med | Trawl | 58 | 400 |
| Dover sole | Med | Trawl | 60 | 450 |
| Cod | Large | Trawl / Ledger — boat | 63 | 700 |
| Turbot | Large | Trawl | 66 | 800 |
| Hake | Med–Large | Trawl | 67 | 650 |
| Ling | Large | Trawl | 68 | 700 |
| Monkfish | Large | Trawl | 69 | 850 |
| Langoustine | Small–Med | Pot | 70 | 320 |

### The Deep (71–99 preview)
*Reserved growth space — no stats yet.*

- **Deep trawl:** orange roughy, black scabbardfish, roundnose grenadier, blue ling, alfonsino
- **Big-game trolling** (future method): blue/striped/black marlin, sailfish, swordfish, yellowfin & bluefin tuna, wahoo, mahi-mahi
- **Quest & no-catch:** coelacanth, oarfish (sighting event), giant squid, Greenland shark, great white shark, whale shark, manta ray, sawfish, beluga sturgeon

### Events (migratory & seasonal)

- **Salmon & sea trout runs** — temperate rivers, autumn
- **Pacific salmon runs** — northern rivers, seasonal
- **Whitebait runs** — NZ river mouths, spring
- **Eel nights** — temperate rivers & lakes, after dark
- **Mitten crab runs** — temperate rivers, autumn; river traps only
- **Catch-and-release quest fish** — beluga sturgeon, European sea sturgeon, giant barb

---

## Open Questions

- Trawling co-op minigame design (crew of 2–4)
- 71–99 content: deep-water trawling, big-game trolling (marlin/tuna), arctic tier?
- Catch-rate & XP tuning after playtest
