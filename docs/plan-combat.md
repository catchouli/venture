# Plan: characters and combat (milestone 4)

Combat, and the character behind it: classes, levels, stats, skills,
equipment, creatures and fighting them. Non-combat skills come after this.

Each step below is playable on its own and ends with a commit. Everything
follows the existing rules: the server decides, content lives in the content
pack, and all the RPG rules live in one engine-free crate.

## The RPG rules: one crate, `venture_rpg`

Every number lives in one place: stats, modifiers, levels, health, hit chances,
damage, attack speed and point awards. The crate is plain Rust, with no Bevy
and no networking, and every formula has unit tests. The server uses it to
decide, and the client uses it to show numbers (such as stats in the stats
tab) that match.

**Basis: the 5th edition System Reference Document (SRD 5.1), adapted for
real time.**
- **Licence:** Wizards of the Coast released the SRD 5.1 under Creative
  Commons Attribution 4.0 (CC BY 4.0). We can use it freely, commercially
  too, as long as we credit it. (Game mechanics themselves generally aren't
  copyrightable anyway.) We take the mechanics only, not text, names or
  settings, and add the attribution to the README and credits.
- **Fit:** its abilities are our five stats (STR, DEX, CON, INT, WIS; we
  leave out CHA). It's well known and thoroughly play-tested, and its maths is
  small and easy to read.
- **What we take from it:**
  - The ability modifier: `floor((score − 10) / 2)`.
  - Classes with a hit die: Fighter d10, Mage d6.
  - Health: full hit die + CON modifier at level 1, then the die's average
    + CON modifier per level.
  - Attack rolls: d20 + ability modifier + proficiency, against Armour Class.
    A natural 20 is a critical hit (damage dice twice); a natural 1 misses.
  - Damage dice for weapons and spells.
- **What's ours**, because a tabletop game is turn-based and levels slowly:
  - **Attack speed (ASPD):** attacks per second = the weapon's base speed ×
    (1 + 5% per DEX modifier), capped.
  - **Mana:** a class base, plus INT/WIS modifier, per level.
  - **The experience curve:** smooth and quick early on, for example
    `xp to next level = round(100 × level^1.5)`.
  - **Points per level:** 2 stat points per level, and 1 skill point per
    level plus 1 extra every 5 levels.

  Each is a single, documented function, easy to retune.
- **Combat level** is the character's level. It's shown under the name when
  selected.

## Step 1: classes, levels and stats (the foundation, done)

- **The `venture_rpg` crate**, covering everything above.
- **Classes in the content pack** (`classes/*.ron`): name, description, hit
  die and mana, and starting stats. Fighter and Mage to begin with. Starting
  skills, starting equipment and class skill lists came in step 2.
- **Character creation** asks for a name, a class and a body type
  (Masculine, Feminine or Androgynous; it changes the model's build), with a
  turning preview of the character. **Names are unique**, ignoring case.
- **Profiles** gain class, body type, level, experience, stats, unspent stat
  and skill points, and **Miasma**.
- **Miasma** is money kept on the character, not an item in the bag. Existing
  coins are converted, and shops take and pay Miasma.
- **The bag** grows to 50 slots and scrolls, with a "12/50" count and the
  character's Miasma along the bottom.
- The level shows under names in the target frame.
- **Admins** (listed by name in `server.ron`) have `/setlevel LEVEL`,
  `/reset` and `/setclass CLASS`, with a level-up effect everyone sees.

## Step 2: the character window (done)

One window with tabs, each with its own key:

| Tab | Key | What it has |
| --- | --- | --- |
| Stats | C | Name, class, level and experience bar. STR, DEX, CON, INT, WIS, with + buttons to spend stat points. Secondary stats: health, mana, armour class, attack bonus, damage, attack speed. |
| Skills | K | **Actions** every character has (Attack, Talk, Follow). **Skills** from the class: learned ones to use, and locked ones with the level they unlock at and a button to learn them with skill points. |
| Inventory | I | The bag (from step 1). |
| Equipment | G | Weapon and body slots to begin with. Equip from the bag, unequip back into it. |
| Quests | J | The journal (as now). |
| Emotes | O | Gestures to click (Wave first; `/wave` in chat does the same). Last, as the least important. |

- **Equipment** is items with a slot and bonuses (weapon: damage dice, speed,
  the stat it scales with; armour: armour class).
  - **Slots:** weapon, body and legs to begin with (head, hands, feet later).
    An item can fill more than one slot: a robe fills body and legs.
  - **Worn equipment shows on the character.** Each equippable item says how
    it looks in the content pack: a cut such as plates or a robe, and its
    colours. Weapons are held in the hand. These are placeholder shapes until
    real models; the class outfits from step 1 become the first items'
    looks.
  - The Fighter starts with a Wooden Chestguard and Wooden Legguards
    (planks strapped on with leather) and a Training Sword.
  - The Mage starts with Plain Robes (body and legs) and a Wooden Staff.
  - Worn equipment shows on the character (placeholder shapes until real
    models).
- **Every character starts with Rest** (sit down and recover faster). The
  Fighter also starts with **Whack**, the Mage with **Air Blast**.

## Step 3: creatures and spawners

- **Creatures** in the content pack (`creatures/*.ron`):
  - Name, level, appearance, stats, health, armour class and attack.
  - Aggressive or not, with an aggro radius.
  - Wander radius and experience reward.
- **Spawners** in the content pack: a creature, a placement (relative to a
  settlement, like NPCs), a radius, and a respawn time.
  - Each spawner keeps one creature alive. When it dies, the spawner waits
    and spawns another somewhere in its radius.
  - Hot reload works as it does for NPCs.
- **The first two creatures:**
  - **Rabbit**, level 1, not aggressive: several spawners around the town.
  - **Goblin**, level 2, aggressive: a little way out, easy to find.
- Creatures are a third kind of entity (`Creature { appearance, level }`).
  Selecting one shows its health bar in the target frame, which already exists
  for this.

## Step 4: fighting

- **The Attack action** (click a selected creature again, `/attack`, or the
  skills tab). It works like following: the character runs into range, then
  auto-attacks at their attack speed until the target dies, they give another
  order, or the target gets away.
- **The server resolves every hit** with `venture_rpg`: hit or miss, damage
  and crits. Health goes down, and everyone nearby sees it.
- **Creatures fight back** when hit. Aggressive ones start fights themselves,
  and give up and go home if led too far away (a leash).
- **Death:** creatures drop out and the spawner restarts its timer. The killer
  gets experience, and may level up (full health, points to spend, a notice
  and a flash).
- **Player death:** the player comes back at the **nearest respawn point**,
  with full health and no loss. Respawn points are a master list in the
  content pack (`respawn_points.ron`), one or more per town, each placed
  relative to its settlement. The server picks the one closest to where the
  player died.
- **Health comes back** slowly out of combat, and faster while resting.
- **What you see:** floating damage numbers, "miss", health bars over hurt
  creatures, a swing animation, and a hit flash.
- **Rough numbers:**
  - A Fighter with the Training Sword kills a rabbit in one hit. Unarmed takes
    two.
  - A Mage takes two hits with the staff, or one Air Blast.
  - A goblin takes about two Air Blasts, and hits back hard enough to matter at
    level 1.

## Step 5: skills and the hotbar

- **Using skills** on the current target, from the skills tab or the
  **hotbar** (keys 1 to 0). Drag skills and actions from the skills tab onto
  it; the layout is saved with the character.
- **Skills are defined in the content pack** (`skills/*.ron`): name, icon,
  mana cost, cooldown, range, the effect (melee hit, projectile, rest), its
  damage in dice, and which visual effect and sound it uses.
- **Whack:** a heavy melee hit with a flash. **Air Blast:** a slow white
  projectile. **Rest:** sit down; health and mana come back faster until you
  move.
- **Cooldowns and mana**, checked by the server and shown on the icons.
- **Effects and sounds are content** (client files, like icons). For now they
  are placeholders: effects are procedural shapes and flashes with simple
  settings, and sounds are short clips made by a script. Real ones can replace
  them file for file later.

## After this milestone: class changes

At some level, characters advance into a second class: Fighter becomes
**Warrior** or **Rogue**, and Mage becomes **Wizard** or **Sage**. Classes in
the content pack will say which class they advance from, and at what level,
so the class tree is content too.

## Later (not in this milestone)

Loot and drops, more classes and skills, player-vs-player, parties, buffs and
debuffs, ranged weapons, creature abilities, and death penalties.
