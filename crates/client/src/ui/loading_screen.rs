//! The loading screen, and the overlay when the connection is lost.
//!
//! From starting up until the world is ready to show, a full-screen loading
//! screen with a progress bar covers everything: connecting, downloading the
//! content (by bytes), preparing its textures and sky, and loading the land
//! around the character. Then it fades away. (On the web, the page shows its
//! own bar while the game itself downloads; this removes it once it's up.)
//!
//! When the connection is lost, the game fades out under a red message
//! while the client keeps trying to reconnect.

use bevy::prelude::*;

use crate::content::{ContentProgress, ContentStore};
use crate::network::{ClientState, Session};
use crate::world::lighting::SkyState;
use crate::world::source::WorldLoadError;
use crate::world::streaming::StreamingProgress;
use crate::world::terrain_material::GroundTextures;

/// How quickly the screen fades in and out (per second).
const FADE_RATE: f32 = 3.0;
/// How long to wait for the land and looks once in the game, in seconds,
/// before showing the game anyway (so one missing chunk can't keep a player
/// out).
const MOST_WAIT: f32 = 30.0;
/// How much of the bar each step fills: connecting, the content, its
/// textures and sky, and the land.
const STEPS: [f32; 4] = [0.05, 0.7, 0.1, 0.15];
/// The loading screen's background.
const BACKGROUND: Color = Color::srgb(0.106, 0.102, 0.09);
/// The bar's colour.
const BAR: Color = Color::srgb(0.79, 0.65, 0.35);

/// Shows the loading screen and the connection overlay.
pub struct LoadingScreenPlugin;

impl Plugin for LoadingScreenPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Loading>()
            .add_systems(Startup, spawn_screen)
            .add_systems(Update, (measure_loading, update_screen, remove_page_loader).chain());
    }
}

/// How far loading has got.
#[derive(Resource, Default)]
struct Loading {
    /// What's happening, to show.
    stage: &'static str,
    /// The detail (such as megabytes), to show.
    detail: String,
    /// How much of the bar is full, 0 to 1.
    fraction: f32,
    /// Whether everything's ready (or waited for long enough).
    done: bool,
    /// The most chunks seen waiting, for the land's share of the bar.
    most_waiting: usize,
    /// Seconds spent in the game waiting for the land and looks.
    waited: f32,
}

#[derive(Component)]
struct Screen;

#[derive(Component)]
struct Headline;

#[derive(Component)]
struct Detail;

#[derive(Component)]
struct Track;

#[derive(Component)]
struct Fill;

fn spawn_screen(mut commands: Commands) {
    commands
        .spawn((
            Screen,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(12),
                ..default()
            },
            BackgroundColor(BACKGROUND),
            // Drawn above the game's other interface.
            GlobalZIndex(10),
        ))
        .with_children(|screen| {
            screen.spawn((Headline, Text::default(), TextFont { font_size: FontSize::Px(26.0), ..default() }));
            screen
                .spawn((
                    Track,
                    Node {
                        width: px(420),
                        max_width: percent(80),
                        height: px(10),
                        border: UiRect::all(px(1)),
                        ..default()
                    },
                    BorderColor::all(Color::srgb(0.42, 0.35, 0.23)),
                    BackgroundColor(Color::srgb(0.165, 0.153, 0.137)),
                ))
                .with_child((
                    Fill,
                    Node { width: percent(0), height: percent(100), ..default() },
                    BackgroundColor(BAR),
                ));
            screen.spawn((
                Detail,
                Text::default(),
                TextFont { font_size: FontSize::Px(15.0), ..default() },
                TextColor(Color::srgb(0.66, 0.61, 0.53)),
            ));
        });
}

/// Works out how far loading has got.
fn measure_loading(
    time: Res<Time>,
    state: Res<State<ClientState>>,
    content: Res<ContentStore>,
    progress: Res<ContentProgress>,
    textures: Res<GroundTextures>,
    sky: Res<SkyState>,
    land: Res<StreamingProgress>,
    mut loading: ResMut<Loading>,
) {
    if loading.done {
        return;
    }
    let connected = *state.get() != ClientState::Connecting;
    let (arrived, wanted) = progress.bytes();
    let content_ready = content.version.is_some();
    let looks_ready = textures.settled() && sky.settled();
    loading.most_waiting = loading.most_waiting.max(land.waiting);
    let land_share = if land.measured { 1.0 - land.waiting as f32 / loading.most_waiting.max(1) as f32 } else { 0.0 };
    let content_share = match (content_ready, wanted) {
        (true, _) => 1.0,
        (false, 0) => 0.0,
        (false, wanted) => arrived as f32 / wanted as f32,
    };
    let looks_share = (textures.settled() as u8 + sky.settled() as u8) as f32 / 2.0;
    loading.fraction = [connected as u8 as f32, content_share, looks_share, land_share]
        .iter()
        .zip(STEPS)
        .map(|(share, weight)| share * weight)
        .sum();
    let megabytes = |bytes: u64| bytes as f32 / 1_048_576.0;
    (loading.stage, loading.detail) = if !connected {
        ("Connecting...", String::new())
    } else if !content_ready {
        let detail =
            if wanted > 0 { format!("{:.1} / {:.1} MB", megabytes(arrived), megabytes(wanted)) } else { String::new() };
        ("Downloading the world's content...", detail)
    } else if !looks_ready {
        ("Preparing the ground and sky...", String::new())
    } else {
        ("Loading the land around you...", String::new())
    };

    // A new player chooses their name as soon as the content (which their
    // choices need) is here; otherwise, the game shows once the land is.
    let land_ready = land.measured && land.waiting == 0;
    match state.get() {
        ClientState::ChoosingName => loading.done = content_ready,
        ClientState::Playing => {
            loading.waited += time.delta_secs();
            loading.done = content_ready && looks_ready && land_ready || loading.waited > MOST_WAIT;
        }
        _ => {}
    }
    if loading.done {
        info!("Loaded in {:.1} s.", time.elapsed_secs());
    }
}

/// Shows the loading screen while loading, or the connection overlay, and
/// fades between them and the game.
fn update_screen(
    time: Res<Time>,
    state: Res<State<ClientState>>,
    session: Res<Session>,
    loading: Res<Loading>,
    world_error: Option<Res<WorldLoadError>>,
    screen: Single<(&mut BackgroundColor, &mut Visibility), With<Screen>>,
    mut headline: Single<(&mut Text, &mut TextColor), (With<Headline>, Without<Detail>)>,
    mut detail: Single<&mut Text, (With<Detail>, Without<Headline>)>,
    mut track: Single<&mut Visibility, (With<Track>, Without<Screen>)>,
    mut fill: Single<&mut Node, With<Fill>>,
) {
    let red = Color::srgb(1.0, 0.35, 0.3);
    let (target, title, colour, text, bar) = match (state.get(), &world_error) {
        (_, Some(error)) => (0.9, "Can't load the world", red, error.0.clone(), false),
        (ClientState::Disconnected, _) => {
            let reason = session.disconnect_reason.clone().unwrap_or_default();
            (0.75, "Disconnected from the server. Reconnecting...", red, reason, false)
        }
        _ if !loading.done => {
            let reason = session.disconnect_reason.clone().filter(|_| *state.get() == ClientState::Connecting);
            let text = reason.unwrap_or_else(|| loading.detail.clone());
            (1.0, loading.stage, Color::srgb(0.91, 0.86, 0.75), text, true)
        }
        (ClientState::Connecting, _) => {
            let reason = session.disconnect_reason.clone().unwrap_or_default();
            (0.8, "Connecting...", Color::WHITE, reason, false)
        }
        (ClientState::ChoosingName, _) => (0.5, "", Color::WHITE, String::new(), false),
        _ => (0.0, "", Color::WHITE, String::new(), false),
    };

    let (mut background, mut visibility) = screen.into_inner();
    let alpha = background.0.alpha();
    let step = FADE_RATE * time.delta_secs();
    let alpha = if alpha < target { (alpha + step).min(target) } else { (alpha - step).max(target) };
    // (Opaque while loading, so a half-loaded world doesn't show; see-through
    // black over the game otherwise.)
    let base = if bar { BACKGROUND } else { Color::BLACK };
    background.0 = base.with_alpha(alpha);
    *visibility = if alpha > 0.0 { Visibility::Inherited } else { Visibility::Hidden };

    if headline.0.0 != title {
        headline.0.0 = title.to_string();
    }
    headline.1.0 = colour;
    if detail.0 != text {
        detail.0 = text;
    }
    **track = if bar { Visibility::Inherited } else { Visibility::Hidden };
    fill.width = percent(loading.fraction.clamp(0.0, 1.0) * 100.0);
}

/// On the web, removes the page's own loading bar once this screen has been
/// drawn (a few frames in), so there's no gap between them.
fn remove_page_loader(mut frames: Local<u32>) {
    if *frames > 3 {
        return;
    }
    *frames += 1;
    #[cfg(target_arch = "wasm32")]
    if *frames == 3
        && let Some(loader) = web_sys::window().and_then(|window| window.document()?.get_element_by_id("loading"))
    {
        loader.remove();
    }
}
