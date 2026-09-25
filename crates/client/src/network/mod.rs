//! The client's end of the protocol.
//!
//! The client's world is only a *view* of the server's. Data flows one way
//! through here in each direction:
//!
//! - **In:** [`receive_from_server`] reads the connection and publishes each
//!   server event as a [`FromServer`] message. View modules (the player,
//!   other players, chat, and so on) read those and update what's shown. This
//!   module also moves the [`ClientState`] along (connecting, choosing a name,
//!   playing, disconnected).
//! - **Out:** input modules write [`ToServer`] commands, and
//!   [`send_to_server`] sends them. Nothing else touches the connection.

pub mod connection;
pub mod launch;
#[cfg(not(target_arch = "wasm32"))]
pub mod local_server;
pub mod token;

use bevy::prelude::*;
use venture_protocol::{ClientMessage, PROTOCOL_VERSION, ServerMessage};

use self::connection::{Connection, ConnectionEvent};
use self::launch::{ServerChoice, websocket_url};
use crate::dev::settings::DevSettings;

/// Seconds between attempts to reconnect.
const RETRY_SECONDS: f32 = 3.0;

/// Connects to the server and passes messages both ways.
pub struct NetworkPlugin;

impl Plugin for NetworkPlugin {
    fn build(&self, app: &mut App) {
        // In `--local` mode, start the server inside the client first.
        #[cfg(not(target_arch = "wasm32"))]
        app.add_plugins(local_server::LocalServerPlugin);

        let choice = app.world().resource::<ServerChoice>();
        let profile = app.world().resource::<DevSettings>().profile.clone();
        let slot = profile.or_else(|| choice.is_local().then(|| "local".to_string()));
        app.init_state::<ClientState>()
            .insert_resource(Session::new(token::load_or_create(slot.as_deref())))
            .insert_non_send(ActiveConnection(None))
            .add_message::<FromServer>()
            .add_message::<ToServer>()
            .add_systems(PreUpdate, (reconnect, receive_from_server).chain().in_set(NetworkSystems))
            .add_systems(PostUpdate, send_to_server);
    }
}

/// The systems that receive from the server (in `PreUpdate`). Anything that
/// must see server events before state changes take effect runs after these.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct NetworkSystems;

/// Where the client is in its connection to the game.
#[derive(States, Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientState {
    /// Connecting and logging in.
    #[default]
    Connecting,
    /// A new player, choosing a name.
    ChoosingName,
    /// In the game.
    Playing,
    /// Lost (or refused) the connection; retrying.
    Disconnected,
}

/// An event from the server, for view modules to show.
#[derive(Message, Clone, Debug)]
pub struct FromServer(pub ServerMessage);

/// A command for the server, from an input module.
#[derive(Message, Clone, Debug)]
pub struct ToServer(pub ClientMessage);

/// The session with the server.
#[derive(Resource)]
pub struct Session {
    /// This player's login token.
    token: String,
    /// Why the connection was last lost, to show the player.
    pub disconnect_reason: Option<String>,
    retry: Timer,
}

impl Session {
    fn new(token: String) -> Self {
        // Finished to start with, so the first attempt happens at once.
        let mut retry = Timer::from_seconds(RETRY_SECONDS, TimerMode::Once);
        retry.tick(retry.duration());
        Self { token, disconnect_reason: None, retry }
    }
}

/// The current connection, if any. Not `Send` on the web, so it's a non-send resource.
struct ActiveConnection(Option<Connection>);

/// A server running inside this client (`--local` mode), to connect to instead of the network.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Resource)]
pub struct LocalServerLink(pub venture_server::LinkSender);

/// Opens a connection when there isn't one and it's time to try.
fn reconnect(
    time: Res<Time>,
    choice: Res<ServerChoice>,
    #[cfg(not(target_arch = "wasm32"))] local: Option<Res<LocalServerLink>>,
    mut session: ResMut<Session>,
    mut connection: NonSendMut<ActiveConnection>,
) {
    if connection.0.is_some() || !session.retry.tick(time.delta()).is_finished() {
        return;
    }
    session.retry.reset();
    let opened = match &*choice {
        ServerChoice::Remote { base_url } => Connection::open_websocket(&websocket_url(base_url)),
        #[cfg(not(target_arch = "wasm32"))]
        ServerChoice::Local { .. } => match local {
            Some(local) => Ok(Connection::open_local(&local.0)),
            None => Err("The local server isn't running.".to_string()),
        },
    };
    match opened {
        Ok(opened) => connection.0 = Some(opened),
        Err(error) => session.disconnect_reason = Some(error),
    }
}

/// Reads the connection, publishes server events, and moves the client state along.
fn receive_from_server(
    mut session: ResMut<Session>,
    mut connection: NonSendMut<ActiveConnection>,
    state: Res<State<ClientState>>,
    mut next_state: ResMut<NextState<ClientState>>,
    mut events: MessageWriter<FromServer>,
) {
    let Some(open) = connection.0.as_mut() else { return };
    for event in open.poll() {
        match event {
            ConnectionEvent::Opened => {
                session.disconnect_reason = None;
                open.send(&ClientMessage::Hello { protocol_version: PROTOCOL_VERSION, token: session.token.clone() });
            }
            ConnectionEvent::Message(message) => {
                match &message {
                    ServerMessage::NameNeeded { .. } => next_state.set(ClientState::ChoosingName),
                    ServerMessage::Welcome { .. } => {
                        session.disconnect_reason = None;
                        next_state.set(ClientState::Playing);
                    }
                    ServerMessage::Rejected { reason } => session.disconnect_reason = Some(reason.clone()),
                    _ => {}
                }
                events.write(FromServer(message));
            }
            ConnectionEvent::Closed(reason) => {
                // Keep a more specific reason (such as a rejection) if there is one.
                session.disconnect_reason.get_or_insert(reason);
                connection.0 = None;
                if *state.get() != ClientState::Disconnected {
                    next_state.set(ClientState::Disconnected);
                }
                return;
            }
        }
    }
}

/// Sends queued commands, if connected.
fn send_to_server(mut commands: MessageReader<ToServer>, mut connection: NonSendMut<ActiveConnection>) {
    let Some(open) = connection.0.as_mut() else {
        commands.clear();
        return;
    };
    for ToServer(message) in commands.read() {
        open.send(message);
    }
}
