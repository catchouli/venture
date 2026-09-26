//! The connection to the server: the only code that touches the transport.
//!
//! Two transports sit behind one interface: a WebSocket (browser and desktop,
//! through `ewebsock`), and an in-memory link to a server running inside the
//! desktop client (`--local` mode). Either way the bytes are the same
//! protocol messages, so the rest of the client can't tell the difference.

use venture_protocol::{ClientMessage, MAX_CLIENT_MESSAGE_BYTES, ServerMessage};

/// Something that happened on the connection.
pub enum ConnectionEvent {
    /// The connection is open; messages can be sent.
    Opened,
    /// A message from the server.
    Message(ServerMessage),
    /// The connection closed or failed, for this reason.
    Closed(String),
}

/// An open (or opening) connection.
pub enum Connection {
    /// A WebSocket to a server over the network.
    #[allow(missing_docs)]
    WebSocket { sender: ewebsock::WsSender, receiver: ewebsock::WsReceiver },
    /// An in-memory link to a server inside this client.
    #[cfg(not(target_arch = "wasm32"))]
    #[allow(missing_docs)]
    Local {
        link: venture_server::LinkSender,
        id: venture_server::ConnectionId,
        outgoing: async_channel::Receiver<Vec<u8>>,
        opened: bool,
    },
}

impl Connection {
    /// Starts connecting to a server's WebSocket address.
    pub fn open_websocket(url: &str) -> Result<Self, String> {
        let options = ewebsock::Options::default();
        let (sender, receiver) = ewebsock::connect(url, options)?;
        Ok(Self::WebSocket { sender, receiver })
    }

    /// Connects to a server running inside this client.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open_local(link: &venture_server::LinkSender) -> Self {
        let (id, outgoing) = link.connect();
        Self::Local { link: link.clone(), id, outgoing, opened: false }
    }

    /// Sends a message to the server.
    pub fn send(&mut self, message: &ClientMessage) {
        let bytes = message.encode();
        debug_assert!(bytes.len() <= MAX_CLIENT_MESSAGE_BYTES, "client messages must fit the server's limit");
        match self {
            Self::WebSocket { sender, .. } => sender.send(ewebsock::WsMessage::Binary(bytes)),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Local { link, id, .. } => {
                link.report(venture_server::LinkEvent::Received { id: *id, bytes });
            }
        }
    }

    /// Everything that has happened since the last call.
    pub fn poll(&mut self) -> Vec<ConnectionEvent> {
        match self {
            Self::WebSocket { receiver, .. } => {
                std::iter::from_fn(|| receiver.try_recv()).filter_map(websocket_event).collect()
            }
            #[cfg(not(target_arch = "wasm32"))]
            Self::Local { outgoing, opened, .. } => {
                let mut events = Vec::new();
                if !*opened {
                    *opened = true;
                    events.push(ConnectionEvent::Opened);
                }
                events.extend(std::iter::from_fn(|| outgoing.try_recv().ok()).map(decode));
                if outgoing.is_closed() && outgoing.is_empty() {
                    events.push(ConnectionEvent::Closed("The local server closed the connection.".to_string()));
                }
                events
            }
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Self::Local { link, id, .. } = self {
            link.report(venture_server::LinkEvent::Disconnected { id: *id });
        }
    }
}

fn websocket_event(event: ewebsock::WsEvent) -> Option<ConnectionEvent> {
    match event {
        ewebsock::WsEvent::Opened => Some(ConnectionEvent::Opened),
        ewebsock::WsEvent::Message(ewebsock::WsMessage::Binary(bytes)) => Some(decode(bytes)),
        ewebsock::WsEvent::Message(_) => None,
        ewebsock::WsEvent::Error(error) => Some(ConnectionEvent::Closed(error)),
        ewebsock::WsEvent::Closed => Some(ConnectionEvent::Closed("The server closed the connection.".to_string())),
    }
}

fn decode(bytes: Vec<u8>) -> ConnectionEvent {
    match ServerMessage::decode(&bytes) {
        Ok(message) => ConnectionEvent::Message(message),
        Err(error) => ConnectionEvent::Closed(format!("The server sent something unreadable ({error}).")),
    }
}
