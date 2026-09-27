# Day and night

A day/night cycle with photographic skies: the sun crosses the sky, evenings
turn orange, nights are dark blue but playable, and the sky blends from one
photograph to the next.

## Constraints

- **The sky's light is baked.** WebGL2 has no compute shaders, so Bevy can't
  filter environment maps at run time: the light a sky casts (irradiance and
  radiance cubes) is baked when it's imported. A transition has to blend
  baked light, not refilter a changing sky.
- **Photographs have a sun painted in**, at one place. Cross-fading a day
  sky into an evening one naively shows two faded suns.
- **Real nights are too dark to play.** Moonlight is about 0.2 lux against
  the sun's 100 000, so nights are "film nights": darker and blue, with the
  camera's exposure opened up.

## The design

1. **A clock the server keeps** (`server/src/clock.rs`): the same time for
   everyone, running on from the real clock (so it carries on over
   restarts) at the pace set in `server.ron` (`day`: `day_minutes`,
   `sunrise`, `sunset`, `noon_height`; a two-hour day, dark from 21:00 to
   03:00 by default). It's part of the rules the client gets on joining;
   the server says the time when a player joins and when an admin sets it
   (`/time 19:30`), and the client runs it on (`world/time_of_day.rs`).
2. **An analytic sun and moon** (`venture_simulation::DayCycle`): the sun
   rises in the east-north-east, is highest in the south at midday
   (`noon_height`, 60°), sets in the west-north-west, and carries on round
   below the horizon; the moon is up while it's down. One shadow-casting
   light follows the sun, coloured and dimmed by the air its light passes
   through (Kasten and Young's air mass, with extinction strongest for
   blue: white at midday, orange to red at sunset). Below the horizon it
   hands over to the moon (dim and cool) in twilight, when both are faint,
   so shadows never jump.
3. **Keyframe skies** (to do): photographs for morning, day, evening and
   night, each imported with its sun painted out of the skybox (filled from
   the sky around it) and out of its light (as today), and with its level,
   horizon colour and sun direction recorded. The game draws the sun and
   moon discs itself, where they really are, so there's never a second sun.
4. **Blending** (to do): a small sky material samples the two skies either
   side of the time, weighted smoothly, and adds the sun, the moon and stars
   at night. Their baked light (about 140 000 RGB9E5 texels each) is
   blended on the CPU and uploaded again a few times a second during a
   transition: cheap, WebGL2-friendly, and needing no changes to Bevy. Only
   the two skies in use (and the next) are loaded. Clouds from different
   photographs cross-fade, which reads as the weather changing.
5. **Exposure and haze by time** (`world/daylight.rs`): the sky and the
   light it casts dim through dusk to a night level, the exposure opens
   about two and a half stops at night, and the haze takes the horizon's
   colour, warmed by a low sun.

## Phases

1. **Done:** the clock, the sun's path and colour, the moon, sky
   brightness, exposure and haze by time, `/time` (with the one day sky,
   turned so its sun's glow follows the sun).
2. Sun removal and metadata in `import_sky`; morning, evening and night
   skies. (Needs a night HDRI, and ideally a morning one, from ambientCG;
   without a morning sky, dawn uses the evening one turned east.)
3. The two-sky blend, blended sky light, sun and moon discs, stars, haze
   from the blended horizon.
4. Tuning, and hooks for play: night-only monsters, NPC schedules, lit
   windows and torches.

## Trying it

In the game, admins set the time with `/time 19:30` (or `/time 19.5`). For
screenshots: `VENTURE_SAY="/time 20" cargo run -p venture_client -- --local --admin`.
