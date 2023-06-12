//! Relay client using the noise protocol for E2EE designed
//! for MPC/TSS applications.

#![deny(missing_docs)]
#![cfg_attr(all(doc, CHANNEL_NIGHTLY), feature(doc_auto_cfg))]

mod error;
mod event_loop;

pub use event_loop::{Event, JsonMessage};

#[cfg(all(
    not(all(target_arch = "wasm32", target_os = "unknown")),
    feature = "native"
))]
mod native;

#[cfg(all(
    not(all(target_arch = "wasm32", target_os = "unknown")),
    feature = "native"
))]
pub use native::{NativeClient, NativeEventLoop};

#[cfg(all(
    all(target_arch = "wasm32", target_os = "unknown"),
    feature = "web"
))]
mod web;

#[cfg(all(
    all(target_arch = "wasm32", target_os = "unknown"),
    feature = "web"
))]
pub use web::WebClient;

use mpc_relay_protocol::{
    snow, Encoding, OpaqueMessage, ProtocolState, RequestMessage,
    SealedEnvelope, SessionId, TAGLEN,
};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

pub(crate) type Peers = Arc<RwLock<HashMap<Vec<u8>, ProtocolState>>>;
pub(crate) type Server = Arc<RwLock<Option<ProtocolState>>>;

/// Options used to create a new websocket client.
pub struct ClientOptions {
    /// Client static keypair.
    pub keypair: snow::Keypair,
    /// Public key for the server to connect to.
    pub server_public_key: Vec<u8>,
}

pub use error::Error;

/// Result type for the relay client.
pub type Result<T> = std::result::Result<T, Error>;

/// Encrypt a message to send to a peer.
///
/// The protocol must be in transport mode.
async fn encrypt_peer_channel(
    public_key: impl AsRef<[u8]>,
    peer: &mut ProtocolState,
    payload: &[u8],
    encoding: Encoding,
    broadcast: bool,
    session_id: Option<SessionId>,
) -> Result<RequestMessage> {
    match peer {
        ProtocolState::Transport(transport) => {
            let mut contents = vec![0; payload.len() + TAGLEN];
            let length =
                transport.write_message(payload, &mut contents)?;
            let envelope = SealedEnvelope {
                length,
                encoding,
                payload: contents,
                broadcast,
            };

            let request =
                RequestMessage::Opaque(OpaqueMessage::PeerMessage {
                    public_key: public_key.as_ref().to_vec(),
                    session_id,
                    envelope,
                });

            Ok(request)
        }
        _ => Err(Error::NotTransportState),
    }
}

/// Decrypt a message received from a peer.
///
/// The protocol must be in transport mode.
async fn decrypt_peer_channel(
    peer: &mut ProtocolState,
    envelope: &SealedEnvelope,
) -> Result<Vec<u8>> {
    match peer {
        ProtocolState::Transport(transport) => {
            let mut contents = vec![0; envelope.length];
            transport.read_message(
                &envelope.payload[..envelope.length],
                &mut contents,
            )?;
            let new_length = contents.len() - TAGLEN;
            contents.truncate(new_length);
            Ok(contents)
        }
        _ => Err(Error::NotTransportState),
    }
}
