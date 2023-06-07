use async_trait::async_trait;
use axum::http::StatusCode;
use binary_stream::{
    futures::{BinaryReader, BinaryWriter, Decodable, Encodable},
    Endian, Options,
};
use futures::io::{AsyncRead, AsyncSeek, AsyncWrite};
use snow::{HandshakeState, TransportState};
use std::io::Result;

pub(crate) fn encoding_error(
    e: impl std::error::Error + Send + Sync + 'static,
) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::Other, e)
}

mod types {
    pub const HANDSHAKE_TYPE_SERVER: u8 = 1;
    pub const HANDSHAKE_TYPE_PEER: u8 = 2;

    pub const NOOP: u8 = 0;
    pub const ERROR: u8 = 1;
    pub const HANDSHAKE_INITIATOR: u8 = 2;
    pub const HANDSHAKE_RESPONDER: u8 = 3;
    pub const RELAY_PEER: u8 = 4;
}

/// Default binary encoding options.
fn encoding_options() -> Options {
    Options {
        endian: Endian::Little,
        max_buffer_size: Some(1024 * 32),
    }
}

/// Encode to a binary buffer.
pub async fn encode(encodable: &impl Encodable) -> Result<Vec<u8>> {
    Ok(
        binary_stream::futures::encode(encodable, encoding_options())
            .await?,
    )
}

/// Decode from a binary buffer.
pub async fn decode<T: Decodable + Default>(
    buffer: impl AsRef<[u8]>,
) -> Result<T> {
    Ok(
        binary_stream::futures::decode(
            buffer.as_ref(),
            encoding_options(),
        )
        .await?,
    )
}

/// Types of noise protocol handshakes.
#[derive(Debug, Default)]
pub enum HandshakeType {
    /// Server handshake.
    #[default]
    Server,
    /// Peer handshake.
    Peer,
}

impl From<&HandshakeType> for u8 {
    fn from(value: &HandshakeType) -> Self {
        match value {
            HandshakeType::Server => types::HANDSHAKE_TYPE_SERVER,
            HandshakeType::Peer => types::HANDSHAKE_TYPE_PEER,
        }
    }
}

#[cfg_attr(target_arch="wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl Encodable for HandshakeType {
    async fn encode<W: AsyncWrite + AsyncSeek + Unpin + Send>(
        &self,
        writer: &mut BinaryWriter<W>,
    ) -> Result<()> {
        let id: u8 = self.into();
        writer.write_u8(id).await?;
        Ok(())
    }
}

#[cfg_attr(target_arch="wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl Decodable for HandshakeType {
    async fn decode<R: AsyncRead + AsyncSeek + Unpin + Send>(
        &mut self,
        reader: &mut BinaryReader<R>,
    ) -> Result<()> {
        let id = reader.read_u8().await?;
        match id {
            types::HANDSHAKE_TYPE_SERVER => {
                *self = HandshakeType::Server;
            }
            types::HANDSHAKE_TYPE_PEER => {
                *self = HandshakeType::Peer;
            }
            _ => {
                return Err(encoding_error(crate::Error::MessageKind(id)))
            }
        }
        Ok(())
    }
}

/// Enumeration of protocol states.
pub enum ProtocolState {
    /// Noise handshake state.
    Handshake(HandshakeState),
    /// Noise transport state.
    Transport(TransportState),
}

/// Request messages from the client.
#[derive(Default, Debug)]
pub enum RequestMessage {
    #[default]
    Noop,
    /// Initiate a handshake.
    HandshakeInitiator(HandshakeType, usize, Vec<u8>),
    /// Relay a message to a peer.
    ///
    /// The peer must have already performed a
    /// handshake with the server.
    RelayPeer {
        /// Public key of the receiver.
        public_key: Vec<u8>,
        /// Message payload.
        message: Vec<u8>,
    },
}

impl From<&RequestMessage> for u8 {
    fn from(value: &RequestMessage) -> Self {
        match value {
            RequestMessage::Noop => types::NOOP,
            RequestMessage::HandshakeInitiator(_, _, _) => {
                types::HANDSHAKE_INITIATOR
            }
            RequestMessage::RelayPeer { .. } => types::RELAY_PEER,
        }
    }
}

#[cfg_attr(target_arch="wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl Encodable for RequestMessage {
    async fn encode<W: AsyncWrite + AsyncSeek + Unpin + Send>(
        &self,
        writer: &mut BinaryWriter<W>,
    ) -> Result<()> {
        let id: u8 = self.into();
        writer.write_u8(id).await?;
        match self {
            Self::HandshakeInitiator(kind, len, buf) => {
                kind.encode(&mut *writer).await?;
                writer.write_usize(len).await?;
                writer.write_u32(buf.len() as u32).await?;
                writer.write_bytes(buf).await?;
            }
            Self::RelayPeer {
                public_key,
                message,
            } => {
                writer.write_u32(public_key.len() as u32).await?;
                writer.write_bytes(public_key).await?;
                writer.write_u32(message.len() as u32).await?;
                writer.write_bytes(message).await?;
            }
            Self::Noop => unreachable!(),
        }
        Ok(())
    }
}

#[cfg_attr(target_arch="wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl Decodable for RequestMessage {
    async fn decode<R: AsyncRead + AsyncSeek + Unpin + Send>(
        &mut self,
        reader: &mut BinaryReader<R>,
    ) -> Result<()> {
        let id = reader.read_u8().await?;
        match id {
            types::HANDSHAKE_INITIATOR => {
                let mut kind: HandshakeType = Default::default();
                kind.decode(&mut *reader).await?;
                let len = reader.read_usize().await?;
                let size = reader.read_u32().await?;
                let buf = reader.read_bytes(size as usize).await?;
                *self = RequestMessage::HandshakeInitiator(kind, len, buf);
            }
            types::RELAY_PEER => {
                let size = reader.read_u32().await?;
                let public_key = reader.read_bytes(size as usize).await?;
                let size = reader.read_u32().await?;
                let message = reader.read_bytes(size as usize).await?;
                *self = RequestMessage::RelayPeer {
                    public_key,
                    message,
                };
            }
            _ => {
                return Err(encoding_error(crate::Error::MessageKind(id)))
            }
        }
        Ok(())
    }
}

/// Response messages from the server.
#[derive(Default, Debug)]
pub enum ResponseMessage {
    #[default]
    Noop,
    /// Return an error message to the client.
    Error(StatusCode, String),
    /// Respond to a handshake initiation.
    HandshakeResponder(HandshakeType, usize, Vec<u8>),
    /// Message being relayed from another peer.
    RelayPeer {
        /// Public key of the sender.
        public_key: Vec<u8>,
        /// Message payload.
        message: Vec<u8>,
    },
}

impl From<&ResponseMessage> for u8 {
    fn from(value: &ResponseMessage) -> Self {
        match value {
            ResponseMessage::Noop => types::NOOP,
            ResponseMessage::Error(_, _) => types::ERROR,
            ResponseMessage::HandshakeResponder(_, _, _) => {
                types::HANDSHAKE_RESPONDER
            }
            ResponseMessage::RelayPeer { .. } => types::RELAY_PEER,
        }
    }
}

#[cfg_attr(target_arch="wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl Encodable for ResponseMessage {
    async fn encode<W: AsyncWrite + AsyncSeek + Unpin + Send>(
        &self,
        writer: &mut BinaryWriter<W>,
    ) -> Result<()> {
        let id: u8 = self.into();
        writer.write_u8(id).await?;
        match self {
            Self::Error(code, message) => {
                let code: u16 = (*code).into();
                writer.write_u16(code).await?;
                writer.write_string(message).await?;
            }
            Self::HandshakeResponder(kind, len, buf) => {
                kind.encode(&mut *writer).await?;
                writer.write_usize(len).await?;
                writer.write_u32(buf.len() as u32).await?;
                writer.write_bytes(buf).await?;
            }
            Self::RelayPeer {
                public_key,
                message,
            } => {
                writer.write_u32(public_key.len() as u32).await?;
                writer.write_bytes(public_key).await?;
                writer.write_u32(message.len() as u32).await?;
                writer.write_bytes(message).await?;
            }
            Self::Noop => unreachable!(),
        }
        Ok(())
    }
}

#[cfg_attr(target_arch="wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
impl Decodable for ResponseMessage {
    async fn decode<R: AsyncRead + AsyncSeek + Unpin + Send>(
        &mut self,
        reader: &mut BinaryReader<R>,
    ) -> Result<()> {
        let id = reader.read_u8().await?;
        match id {
            types::ERROR => {
                let code = reader
                    .read_u16()
                    .await?
                    .try_into()
                    .map_err(encoding_error)?;
                let message = reader.read_string().await?;
                *self = ResponseMessage::Error(code, message);
            }
            types::HANDSHAKE_RESPONDER => {
                let mut kind: HandshakeType = Default::default();
                kind.decode(&mut *reader).await?;
                let len = reader.read_usize().await?;
                let size = reader.read_u32().await?;
                let buf = reader.read_bytes(size as usize).await?;
                *self = ResponseMessage::HandshakeResponder(kind, len, buf);
            }
            types::RELAY_PEER => {
                let size = reader.read_u32().await?;
                let public_key = reader.read_bytes(size as usize).await?;
                let size = reader.read_u32().await?;
                let message = reader.read_bytes(size as usize).await?;
                *self = ResponseMessage::RelayPeer {
                    public_key,
                    message,
                };
            }
            _ => {
                return Err(encoding_error(crate::Error::MessageKind(id)))
            }
        }
        Ok(())
    }
}
