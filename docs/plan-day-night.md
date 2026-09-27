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
3. **Keyframe skies:** photographs for day (DaySkyHDRI070B), evening
   (EveningSkyHDRI047B, which is dawn too, turned to the rising sun) and
   night (NightSkyHDRI008), imported with their suns and a night sky's
   trails painted out (see [textures.md](textures.md)). The game draws the
   sun and moon itself, where they really are, so there's never a second
   sun. `content/skies/cycle.ron` sets which sky shows when, and how
   bright: night until 01:30, dawn (the evening sky) at 03:00 as the sun
   rises, day from 05:00 to 19:00, evening at 20:48 and night from 22:18.
4. **Blending** (`world/sky.rs`, `shaders/sky.wgsl`): the sky is drawn on
   the inside of a big sphere round the camera (in place of Bevy's
   one-cubemap skybox), sampling the two keyframe skies either side of the
   time, weighted smoothly, each turned its own way (following the sun, or
   for the stars, round the pole star a turn a day), with the sun's disc and
   glow and the moon's disc (with darker seas) where they are. The skies'
   baked light (about 140 000 RGB9E5 texels each) is blended on the CPU and
   uploaded again whenever the blend has moved on by 1%: cheap,
   WebGL2-friendly, and needing no changes to Bevy. All three skies are
   loaded (about 75 MB of GPU memory for their pictures; a lower fidelity
   tier could keep only the two in use). Clouds from different photographs
   cross-fade, which reads as the weather changing.
5. **Exposure and haze by time** (`world/daylight.rs`): the sky and the
   light it casts dim through dusk to a night level, the exposure opens
   about two and a half stops at night, and the haze takes the horizon's
   colour, warmed by a low sun.

## Phases

1. **Done:** the clock, the sun's path and colour, the moon, sky
   brightness, exposure and haze by time, `/time` (with the one day sky,
   turned so its sun's glow follows the sun).
2. **Done:** sun removal and trail painting in `import_sky`; the evening
   and night skies (dawn uses the evening sky turned east; a morning HDRI
   would be an easy upgrade).
3. **Done:** the two-sky blend, blended sky light, sun and moon discs,
   turning stars, haze from the blended horizons.
4. Tuning, and hooks for play: night-only monsters, NPC schedules, lit
   windows and torches.

## Trying it

In the game, admins set the time with `/time 19:30` (or `/time 19.5`). For
screenshots: `VENTURE_SAY="/time 20" cargo run -p venture_client -- --local --admin`.
