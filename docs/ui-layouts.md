# Interface layouts

Every window and frame in the interface is built from a **layout file** in
the content pack, so designers can move, resize, restyle and redecorate it
without touching code. Layouts are data, like the rest of the content:

```
content/ui/
  layouts/*.ron   one file per window: where its parts go and how they look
  styles.ron      named colours, text styles (font, size, colour) and bar colours
  theme.ron       the pictures for each theme piece (frames, buttons, slots, bars)
  *.png, *.ttf    pictures and fonts
```

Save a layout (or `styles.ron`) while the game is running and the window is
rebuilt within a second or so. `cargo run -p venture_content --features scripts --bin check`
reports mistakes (a misspelt style, a missing picture, an element the game
needs that's gone), and a layout with a problem never breaks the game: the
server keeps the last good content, and the game has built-in copies of
every layout to fall back on.

So far the **player frame** (`player_frame.ron`), the **hotbar**
(`hotbar.ron`), the **party frame** (`party_frame.ron`: the other members of
the player's party, under the player frame) and the **party invitation**
(`party_invite.ron`) are layouts; the other windows are moving over.

## Elements

A layout is one element (the window) with elements inside it. Every element
is a box, placed and sized the way CSS flexbox does it. All its properties
are optional:

```ron
(
    id: "player_frame",
    position: Absolute, left: 10, top: 10, width: 220,
    direction: Column, gap: 5, padding: (7, 10),
    piece: "panel", layer: 5,
    children: [
        (text: "{name}  Level {level}", style: "frame_name"),
        (bar: "health", height: 14),
    ],
)
```

**Placing it**

| Property | What it does |
| --- | --- |
| `position` | `Relative` (the default: in line with its siblings) or `Absolute` (at a fixed place in its parent) |
| `left`, `right`, `top`, `bottom` | where an `Absolute` element sits, from its parent's edges |
| `width`, `height` | its size |
| `min_width`, `min_height`, `max_width`, `max_height` | limits on its size |
| `margin` | space outside it |
| `grow` | how much of the spare room in its parent it takes (0 for none) |
| `align_self` | how it lines up in its parent, if not as the parent says |

**Arranging what's inside it**

| Property | What it does |
| --- | --- |
| `direction` | `Row` (the default: left to right) or `Column` (top to bottom) |
| `gap` | space between the children |
| `padding` | space between its edge and its children |
| `align` | how children line up across the direction: `Stretch` (the default), `Start`, `Center` or `End` |
| `justify` | how children spread along the direction: `Start` (the default), `Center`, `End`, `SpaceBetween` or `SpaceAround` |
| `wrap` | `true` to let children wrap onto more rows |
| `children` | the elements inside it |

**How it looks**

| Property | What it does |
| --- | --- |
| `piece` | a theme piece to draw it with, such as `"panel"`, `"slot"` or `"button"` (the pictures are in `theme.ron`; see "The interface's look" in [content-authoring.md](content-authoring.md)) |
| `image` | a picture from `ui/` to fill it with (for decoration) |
| `icon` | `true` if the game puts a picture here (an item or ability icon) |
| `colour`, `border`, `border_colour`, `corners` | the plain look, used where the theme has no picture |
| `text`, `style` | text to show, and its style from `styles.ron` (see below) |
| `bar` | makes it a bar (see below) |
| `tooltip` | a tooltip (`"Title\nMore about it"`), which can show the game's data like `text` does |
| `button` | `true` if it can be clicked (it lights up under the mouse) |
| `layer` | draws it above other things: windows use 5, so names over characters' heads stay underneath |
| `hidden` | `true` if it starts hidden (the game shows it when it's needed) |

**For the game**

| Property | What it does |
| --- | --- |
| `id` | a name the game finds the element by (see "What the game needs") |
| `template` | `true` if the game makes copies of it (one per hotbar slot, say) rather than showing it as it is |

**Sizes** are pixels (`220`), a share of the parent (`"50%"`), or `"auto"`.
**Edges** (`padding`, `margin`) are one number for all four sides,
`(vertical, horizontal)`, or `(top, right, bottom, left)`, as in CSS.
**Colours** are a name from `styles.ron`, `(red, green, blue)` or
`(red, green, blue, opacity)`, each from 0 to 1.

## Text and the game's data

Text can show the game's data by name in braces: `"{health} / {health_max}"`.
What the game provides:

| Name | What it is |
| --- | --- |
| `name` | the player's character's name |
| `level` | the player's level |
| `health` | the player's health |
| `health_max` | the player's most health |
| `mana` | the player's mana |
| `mana_max` | the player's most mana |
| `experience` | the player's experience towards the next level |
| `experience_max` | the experience the next level takes |
| `invite_from` | who invited the player to a party |
| `invite_party` | the name of the party they're invited to |
| `invite_seconds` | whole seconds left to answer the invitation |
| `invite_time`, `invite_time_max` | time left to answer, out of the time it was open for (for a bar) |
| `member_name`, `member_class`, `member_level` | a party member's name, class and level |
| `member_health`, `member_health_max`, `member_mana`, `member_mana_max` | a party member's health and mana |

The `member_` names only mean something inside the party frame's `member`
template: the game makes a copy of it for each other member, and each copy
shows its own member's data (see "Lists and rows" below).

## Bars

`bar: "health"` makes an element a bar showing `health` out of `health_max`,
in the bar colours called `health` in `styles.ron` (or `bar_style`'s). It's
drawn with the theme's `bar_back`, `bar_fill` and `bar_frame`, and its
children are drawn over it (for numbers, say).

## Styles

`styles.ron` names the colours, text styles and bar colours the layouts use:

```ron
(
    colours: { "title": (0.96, 0.9, 0.75), "cooldown": (0.0, 0.0, 0.0, 0.6) },
    text: {
        "frame_name": (size: 14, colour: "title"),
        "bar_numbers": (size: 11, colour: "white", shadow: true, align: Center, font: "fancy.ttf"),
    },
    bars: { "health": (fill: (0.75, 0.15, 0.12), back: (0.24, 0.05, 0.04)) },
)
```

A text style's `font` is a TTF or OTF in `ui/`. Without one, the theme's font
(or the built-in one) is used.

## What the game needs

The game gives some elements behaviour by their `id`: it fills them in, or
copies them. Each layout must keep these ids (anything else is free to move,
restyle, add or remove):

| Layout | Element ids the game needs |
| --- | --- |
| `player_frame` | `player_frame` |
| `hotbar` | `hotbar`, `slots`, `slot`, `icon`, `label`, `cooldown_shade`, `cooldown_text`, `key`, `count` |
| `party_frame` | `party_frame`, `member` |
| `party_invite` | `party_invite`, `invite_accept`, `invite_decline` (the buttons: the same as `/accept` and `/decline`) |

For example, the hotbar copies its `slot` template ten times into `slots`,
and fills in each copy's `icon`, `label`, `key`, `cooldown_shade` and
`cooldown_text`, wherever they are and however they look.

## For developers: connecting behaviour

Code says what a layout shows and does; the layout says where and how.

- **Showing data:** put values in `UiData` (`UiData::set(&mut data, "health", 12)`)
  and add each name, with a description, to `UI_DATA` in
  `crates/content/src/layout.rs`. Layouts show it with `{name}` or `bar:`.
- **Giving elements behaviour:** listen for `LayoutBuilt` (sent whenever a
  layout is built or rebuilt) and add your components to its elements by id.
  List the ids your code needs in `LAYOUTS`, so the content check catches a
  layout that loses one.
- **Lists and rows:** mark an element `template: true` in the layout and copy
  it with `Ui::copy(layout, id, count)`, which returns each copy's elements
  by id. Add components through `Ui::commands()`, so they come after the
  copies are made. For a list where each entry shows its own data, copy it
  with `Ui::copy_scoped(layout, id, scopes)`: in the copy for scope `"7"`,
  `{member_name}` (and `bar: "member_health"`) show the data called
  `member_name@7` (`scoped_name("member_name", "7")`), which code sets as
  usual. The party frame (`crates/client/src/ui/party.rs`) does this, with
  each member's entity id as the scope.
- **Showing and hiding a window:** set the `Visibility` of the element with
  the window's `UiRoot`.
- **A new window:** write its behaviour, add a first plain layout to
  `content/ui/layouts/`, and add it to `BUILT_IN_LAYOUTS` in
  `crates/client/src/ui/layout.rs` (the copy the game falls back on). A
  designer can then take a pass over it.

`crates/client/src/ui/vitals.rs` (data only) and `crates/client/src/ui/hotbar.rs`
(a template, and elements filled in by id) are the examples.
