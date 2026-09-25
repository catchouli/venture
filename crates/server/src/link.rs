//! Connections to clients, independent of how they're carried.
//!
//! A transport (the WebSocket server in [`crate::http`], or an in-memory
//! connection in the desktop client's `--local` mode) turns each client
//! connection into [`LinkEvent`]s, and delivers bytes the game sends back.
//! The game only ever sees this interface, so it runs the same whatever the
//! transport is.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use bevy::prelude::*;
use venture_protocol::ServerMessage;

/// How many messages may wait to be sent to one client. A client that falls
/// this far behind is disconnected rather than using ever more memory.
const OUTBOX_CAPACITY: usize = 1024;

/// Identifies one connection (not a player: a player is only known after
/// logging in).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ConnectionId(pub u64);

/// Something that happened on a connection.
pub enum LinkEvent {
    /// A client connected. Bytes for it should be sent to `outbox`.
    Connected {
        /// The new connection.
        id: ConnectionId,
        /// Where to send encoded messages for this client. Dropping it closes the connection.
        outbox: async_channel::Sender<Vec<u8>>,
    },
    /// A client sent a message (still encoded, and not yet trusted).
    Received {
        /// Who sent it.
        id: ConnectionId,
        /// The raw message.
        bytes: Vec<u8>,
    },
    /// A client disconnected.
    Disconnected {
        /// Who.
        id: ConnectionId,
    },
}

/// The transport's end of the link: reports connection events to the game.
#[derive(Clone)]
pub struct LinkSender {
    events: async_channel::Sender<LinkEvent>,
    next_id: std::sync::Arc<AtomicU64>,
}

impl LinkSender {
    /// A fresh id for a new connection.
    pub fn next_connection_id(&self) -> ConnectionId {
        ConnectionId(self.next_id.fetch_add(1, Ordering::Relaxed))
    }

    /// Reports a new connection, returning its id and the channel to read its outgoing bytes from.
    pub fn connect(&self) -> (ConnectionId, async_channel::Receiver<Vec<u8>>) {
        let id = self.next_connection_id();
        let (outbox, outgoing) = async_channel::bounded(OUTBOX_CAPACITY);
        self.report(LinkEvent::Connected { id, outbox });
        (id, outgoing)
    }

    /// Reports an event to the game. Returns `false` if the game has stopped.
    pub fn report(&self, event: LinkEvent) -> bool {
        self.events.try_send(event).is_ok()
    }
}

/// The game's end of the link.
#[derive(Resource)]
pub struct Link {
    events: async_channel::Receiver<LinkEvent>,
    outboxes: HashMap<ConnectionId, async_channel::Sender<Vec<u8>>>,
}

impl Link {
    /// Creates a link: the game keeps the [`Link`], transports get the [`LinkSender`].
    pub fn new() -> (LinkSender, Link) {
        let (events_in, events_out) = async_channel::unbounded();
        let sender = LinkSender { events: events_in, next_id: std::sync::Arc::new(AtomicU64::new(1)) };
        (sender, Link { events: events_out, outboxes: HashMap::new() })
    }

    /// The next event from any connection, if there is one. New connections'
    /// outboxes are recorded, and closed ones forgotten, as events pass through.
    pub fn next_event(&mut self) -> Option<LinkEvent> {
        let event = self.events.try_recv().ok()?;
        match &event {
            LinkEvent::Connected { id, outbox } => {
                self.outboxes.insert(*id, outbox.clone());
            }
            LinkEvent::Disconnected { id } => {
                self.outboxes.remove(id);
            }
            LinkEvent::Received { .. } => {}
        }
        Some(event)
    }

    /// Sends a message to one connection. A client whose outbox is full is disconnected.
    pub fn send(&mut self, to: ConnectionId, message: &ServerMessage) {
        self.send_bytes(to, message.encode());
    }

    /// Sends the same message to several connections, encoding it once.
    pub fn broadcast(&mut self, to: impl IntoIterator<Item = ConnectionId>, message: &ServerMessage) {
        let bytes = message.encode();
        for id in to {
            self.send_bytes(id, bytes.clone());
        }
    }

    /// Closes a connection from the server's side.
    pub fn close(&mut self, id: ConnectionId) {
        // Dropping the outbox tells the transport to close the connection.
        self.outboxes.remove(&id);
    }

    fn send_bytes(&mut self, to: ConnectionId, bytes: Vec<u8>) {
        let Some(outbox) = self.outboxes.get(&to) else { return };
        if outbox.try_send(bytes).is_err() {
            warn!("Connection {to:?} isn't keeping up; closing it.");
            self.close(to);
        }
    }
}
