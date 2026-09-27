# Fishing — Vertical Slice (levels 1–20)

> Implementation spec for the first playable prototype. Companion to `fishing.md` (the full design) — this document copies in everything needed to build and playtest **levels 1–20**, and points at `fishing.md` for anything deferred. Numbers are placeholder-tunable.

## Purpose

A self-contained fishing loop a playtester can pick up cold: create character → get starter kit → fish the river, lake and shore → hit level 20. It must exercise every fundamental mechanic of the design: shadows, bite indicators, hook timing, the fight, trait-based attraction, line/reel gating, and passive traps.

## Scope

**In the slice:**

| Content | Detail |
|---|---|
| Methods | Ledgering (lv 1), Cage traps (lv 1), Float fishing (lv 5), Lure fishing (lv 15) |
| Mechanics | Full rod loop: cast → shadows → indicator → hook → fight → land |
| Gear | T1–T2 rods, line & reel T1–T2, hooks, one float, one weight, bait, 2 static lures, cage trap |
| Fish | ~26 species across 3 biomes (temperate river, temperate lake, temperate coast) |
| Economy | Starter kit + vendor (sells gear & bait, buys fish) |

**Deferred** (see `fishing.md`): cast nets (25), fly fishing (45), trawling (55), deep-sea pots (70), random lure rolls (static lures instead), day/night cycle (all fish always active), events & seasonal runs, gear crafting (vendor instead), line/reel T3+, all 70+ content.

## Playtest Goals

The slice exists to answer these questions:

1. **Hook timing feel** — is the nibble→strike window readable? Too punishing or too trivial for ledger/float manual hooking?
2. **Fight readability** — do players understand tension vs spool-out? Do they learn to stop reeling when the fish runs?
3. **Shadows as progression** — do players notice faint shadows and understand "not yet"? Does the shadow reveal feel rewarding?
4. **Trait matching** — do players grasp "this lure attracts aggressive midwater predators"? Do they self-select lure vs bait fishing?
5. **Trap pacing** — does the passive trap loop (bait → set → harvest) feel worthwhile next to active fishing?
6. **Time-to-20** — target ~2–2.5 hours. Too fast, too slow?

---

## XP System

Same curve as the main design — XP to go from `level` to next: `round(100 × level^1.5)`.

| Level | XP for prev level | Cumulative | | Level | XP for prev level | Cumulative |
|---:|---:|---:|---|---:|---:|---:|
| 2 | 100 | 100 | | 12 | 3,648 | 17,916 |
| 3 | 283 | 383 | | 13 | 4,157 | 22,073 |
| 4 | 520 | 903 | | 14 | 4,687 | 26,760 |
| 5 | 800 | 1,703 | | 15 | 5,238 | 31,998 |
| 6 | 1,118 | 2,821 | | 16 | 5,809 | 37,807 |
| 7 | 1,470 | 4,291 | | 17 | 6,400 | 44,207 |
| 8 | 1,852 | 6,143 | | 18 | 7,009 | 51,216 |
| 9 | 2,263 | 8,406 | | 19 | 7,637 | 58,853 |
| 10 | 2,700 | 11,106 | | 20 | 8,282 | **67,135** |
| 11 | 3,162 | 14,268 | | | | |

**Total: 67,135 XP to reach level 20.** Single XP pool — XP is a property of each fish; no method multipliers.

**Pacing:** main design targets ~19k XP/h for band 1→11 and ~36k/h for 11→26. At those rates level 20 lands at ~2–2.5 hours (dips at slow-bite fish like carp). Tier-2 gear at 18 and the lure unlock at 15 are the mid-slice motivation peaks.

---

## The Map

One temperate river valley, walkable end to end in ~1 minute:

| Spot | Water | Methods | Notes |
|---|---|---|---|
| **River stretch** | River (fresh) | Ledger, Float, Lure, river Cages | The classic leveling water; starts at the vendor, runs upstream |
| **Lake** | Lake (fresh) | Ledger, Float, Lure | A short walk from the river; stillwater species |
| **Shore** | Sea (salt) | Ledger, Float, coast Cages | Downstream end; pier + beach; cast-net shoals NOT in slice |
| **Vendor stall** | — | — | Spawn point. Sells gear/bait, buys fish |

---

## Core Loop

Cast → shadows wander → indicator → hook → fight → land → XP.

### Shadows

Fish appear in the water as shadows, visibility by player level vs the fish's unlock level:

| Player level vs fish unlock | Shadow | Catchable? |
|---|---|---|
| At or above unlock | **Solid** | Yes |
| 1–10 levels below unlock | **Faint** — translucent, smaller (gap tunable) | No — preview |
| >10 below | **Invisible** | — |

Starter fish (lv 1–11 unlocks) are solid from day one — a new player always has catchable shadows in every unlocked biome. Since XP scales with unlock level, richer shadows mark better spots. Shadow size hints fish size; shadow cluster density hints shoaling.

### Bite Indicators & Hooking

| Method | Indicator | Hooking |
|---|---|---|
| Ledgering | Rod tip — slight tug on a nibble | Manual press; too early or too late misses (bait lost, fish flees) |
| Float fishing | Zoomed-in float — dips on a bite | Manual press; timing window wider than ledgering |
| Lure fishing | Rod pulled hard on strike | Automatic — no timing input |

Missed hook = fish gone + bait gone: reel in, re-bait, cast again.

### The Fight

After hooking, the fish fights:

- **Reeling** raises **tension** — past the line's limit → **snap**, fish and bait lost.
- **Not reeling** gives slack — the fish runs; past the reel's capacity → **spool-out**, fish lost.
- The fish **tires** in bursts (tension demand drops between runs) — the landing window.
- Landed fish → inventory + its XP.

Two failure axes, two gear counters: **line strength** (tension), **reel size** (spool). Fish size class gates both (below).

### Attraction (bait & lures)

What bites depends on trait matching:

- **Bait** maps to **diet** — worms → insect-eaters, sweetcorn → herbivores, shrimp bits → crustacean/crayfish traps.
- **Lures** carry a fixed trait profile (slice uses static items; random rolls come later):

| Static lure | Profile | Effect |
|---|---|---|
| Spinner (flashy) | Fish-eater · Aggressive · Midwater · Flashy | Attracts perch, rainbow trout, (coast: sea bass — above slice max for most) |
| Plug (natural) | Fish-eater · Aggressive · Midwater · Natural | Same species set but fewer strikes, bigger average fish (comparison item for playtest) |

- **Depth band** rules: ledger = bottom, float = any depth, lure = midwater retrieves, (fly = surface — deferred).
- **Temperament** gates strike commitment: aggressive fish strike lures readily; skittish fish need patience (longer wait between bites on float/ledger).

### Cage Traps (passive)

Set on the riverbank (freshwater catch table) or shore (coastal catch table):

1. Player places trap with bait; soak timer starts (~20 min placeholder).
2. Trap fills up to **capacity 3** with catches drawn from the biome trap table (bait biases the draws).
3. Harvest: interact → contents to inventory, XP for each catch.
4. Bycatch: ~25% of slots roll the table's bycatch line (junk or low-XP critter).

Traps earn less XP/hour than active fishing (by design) but run while the player does something else. One trap per player in the slice.

---

## Gear

### Rods (cast distance only)

| Rod | Tier 1 | Tier 2 |
|---|---:|---:|
| Ledger rod | lv 1 | lv 18 |
| Float rod | lv 5 | — |
| Lure rod | lv 15 | — |

Higher tier = visibly longer cast — reach better shadows. No speed or catch-rate bonus.

### Line & Reel (shared across methods)

Both T1 at level 1; T2 at level 18. **Both** must be T2 to reliably land Med fish.

| Tier | Level | Line lands… | Reel survives… |
|---:|---:|---|---|
| T1 | 1 | Tiny & Small fish | short runs |
| T2 | 18 | Med fish | medium runs |

Failure rules: too-weak **line** = hooked fish snaps you (tension); too-small **reel** = fish spools you (runs past capacity) even on strong line. Cast-net/trap content has no line — traps bypass tiers.

### Hooks, Float, Weight

- **Hooks**: small and medium sizes in slice. Size gates bait size and hookable fish size (small hook = small fish mostly, rare larger; medium hook = mostly Med, small rarely). No level gate.
- **Float**: one universal float (float fishing).
- **Weight**: one universal weight (ledgering).

### Bait (consumable, one per cast)

| Bait | Diet match | Used for |
|---|---|---|
| Worm | Insect-eater | Ledger & float — river fish |
| Sweetcorn | Herbivore | Ledger & float — mullet, carp-adjacent |
| Shrimp bits | Crustacean-eater | Trap bait — crayfish & crabs |
| Live shrimp | Fish-eater | Trap bait — predators (higher tier, vendor) |

### Traps

| Trap | Location | Contents |
|---|---|---|
| Cage trap | Riverbank | freshwater trap table |
| Cage trap | Shore | coastal trap table |

### Acquisition

Starter kit (given at spawn): ledger rod T1, line T1, reel T1, small hooks ×20, weight ×1, worm ×20, cage trap ×1, shrimp bits ×10. Everything else from the **vendor** (see Economy).

---

## Fish Tables

Traits per fish: **Diet** (Insect/Fish/Bottom/Crust/Plank/Herb/Scav) · **Temp** (Aggr/Opp/Skitt) · **Depth** (Bot/Mid/Surf) · **Size** — sizes drive hook size and the line/reel tier needed. Night/activity: deferred, all fish active always. XP placeholder-tunable.

### Temperate river (fresh)

| Fish | Size | Method | lv | XP | Diet | Temp | Depth |
|---|---|---|---:|---:|---|---|---|
| Dace | Small | Ledger | 1 | 110 | Insect | Skitt | Mid |
| Gudgeon | Tiny | Ledger | 3 | 125 | Bottom | Opp | Bot |
| River snail | Tiny | **Cage** — bycatch | 3 | 60 | Scav | Opp | Bot |
| White-clawed crayfish | Small | **Cage** | 5 | 100 | Scav | Opp | Bot |
| Roach | Small | Float | 7 | 150 | Insect | Skitt | Mid |
| Signal crayfish | Small | **Cage** | 8 | 150 | Scav | Opp | Bot |
| Freshwater mussel | Small | **Cage** — bycatch | 6 | 70 | Plank | Opp | Bot |
| Chub | Small–Med | Float | 10 | 190 | Insect | Opp | Mid |
| Red swamp crayfish | Small–Med | **Cage** | 12 | 180 | Scav | Opp | Bot |
| Perch | Small–Med | Float / **Lure** | 13 | 260 | Crust | Aggr | Mid |
| Noble crayfish | Small–Med | **Cage** | 18 | 250 | Scav | Opp | Bot |
| Bream | Med | Float | 18 | 330 | Bottom | Opp | Bot |

River trap table draws (weighted): signal crayfish 30% · white-clawed 20% · red swamp 20% · snail 15% · mussel 15% (weights shift with bait; shrimp-bits bait zeroes snail/mussel).

### Temperate lake (fresh)

| Fish | Size | Method | lv | XP | Diet | Temp | Depth |
|---|---|---|---:|---:|---|---|---|
| Crucian carp | Small | Float | 10 | 210 | Bottom | Skitt | Bot |
| Rainbow trout (stocked) | Med | **Lure** | 20 | 400 | Insect | Aggr | Mid |

### Temperate shore (salt)

| Fish | Size | Method | lv | XP | Diet | Temp | Depth |
|---|---|---|---:|---:|---|---|---|
| Grey mullet | Med | Float / Ledger | 20 | 300 | Herb | Skitt | Surf |
| Brown shrimp | Tiny | **Cage** | 1 | 90 | Scav | Opp | Bot |
| Shore crab | Small | **Cage** — bycatch | 1 | 60 | Scav | Opp | Bot |
| Common whelk | Small | **Cage** — bycatch | 5 | 80 | Scav | Opp | Bot |

Coast trap table draws: brown shrimp 55% · shore crab 30% · whelk 15%.

**Progression milestones in the tables:** crayfish trio (5/8/12) carries trap leveling · roach (7) is the first float fish · perch (13) is the first lure fish — and the first Aggressive one · chub (10) and bream (18) are the Small→Med hook-size learning curve · bream + mullet (both 18–20) are the T2 line/reel payoff fish.

---

## Progression Path (intended journey)

| Level | Player learns |
|---:|---|
| 1 | Ledgering basics — hook timing, reeling, starter kit |
| 3–5 | Trap loop (crayfish & shore critters), first float rod |
| 7–12 | Float variety (roach, chub, crucian), trap growth (red swamp) |
| 13–15 | First lure (perch!) — trait matching begins |
| 18 | **T2 line & reel + ledger T2** — Med fish unlocked (bream), longer casts |
| 20 | Rainbow trout & grey mullet — lure & float mastery moments; slice complete |

---

## Economy (stub)

Vendor stall at spawn. Placeholder prices ≈ XP/2 per fish (tune freely):

- **Sells**: worm 5 / sweetcorn 5 / shrimp-bits 10 / live-shrimp 25 · hooks small 5, medium 10 · float 100 · weight 50 · float rod 250 · lure rod 750 · spinner lure 400 · plug lure 400 · line T2 1,500 · reel T2 1,500 · ledger rod T2 2,000 · cage trap 300
- **Buys**: all caught fish (price = XP/2, e.g. dace 55, perch 130, bream 165)

Coins have no other sink in the slice — watch the inflation speed during playtest (note for tuning).

---

## Deferred (from fishing.md)

| Feature | fishing.md section | Why deferred |
|---|---|---|
| Cast nets (lv 25) | Methods & Unlocks | Above level cap |
| Fly fishing (lv 45) | Bite Indicators & Hooking | Above level cap |
| Trawling, deep-sea pots | Method Notes | Above level cap |
| Random lure rolls & market | Lures & Flies | Slice uses 2 static lures; generation layer untested |
| Day/night cycle | Fish Traits (Activity) | All fish always active; trait data still recorded |
| Events, seasonal runs | Events | No run species in slice |
| Gear crafting | Gear & Economy | Vendor stands in for player-crafted gear |
| Line/reel T3–T5, Med+ fish beyond bream/mullet | Gear Tiers | Above level cap |
| Estuary/other biomes, 70+ content | Fish & Biomes | Above level cap |

---

## Open Playtest Questions

- Hook-timing window size per method (ms/tenths) — start ledger tight, float loose?
- Fight tuning: tension rise rate vs fish size class; tire timing per size class.
- Do faint shadows motivate or frustrate? Is the 10-level band right?
- Do players understand lure profiles from UI alone, or need tutorial hints?
- Trap soak timer (20 min?) and capacity (3) — right for a session-based playtest?
- Vendor price curve — does XP/2 keep bait affordable and T2 gear aspirational?
- Time-to-20 at playtest skill levels (target 2–2.5 h).
- Lure vs bait share of total catches — is lure fishing "better" once unlocked (intended) or dominant?
