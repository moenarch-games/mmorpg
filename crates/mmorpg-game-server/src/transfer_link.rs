//! Host-to-host transfer link over a trusted LAN (#145, ADR 0003 trust mode).
//!
//! Plain TCP between zone hosts: no authentication or encryption, so it is
//! only for development and trusted LAN hosting and never for the internet.
//! The link carries requests; it decides nothing. Every request is
//! idempotent per transfer ID, so a source may resend after a timeout, and
//! the control plane's handoff record settles the outcome (see
//! [`crate::handoff`]).
//!
//! Frames are `length u32 · tag u8 · body`, big-endian, at most
//! [`MAX_LINK_FRAME_BYTES`]. Requests: `1` offer (body: an encoded character
//! transfer), `2` activate (body: transfer ID u64). Replies: `0` ok, `1`
//! refused (body: UTF-8 reason).

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use mmorpg_core::{CharacterTransfer, TransferId};
use mmorpg_protocol::{decode_character_transfer, encode_character_transfer};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, ToSocketAddrs};

/// Far above any transfer; bounds what a peer can make a host buffer.
pub const MAX_LINK_FRAME_BYTES: usize = 128 * 1024;

const OFFER: u8 = 1;
const ACTIVATE: u8 = 2;
const OK: u8 = 0;
const REFUSED: u8 = 1;

/// What a destination host does with link requests. Both run off the zone
/// tick and must be idempotent per transfer ID.
pub trait TransferLinkHandler: Send + Sync + 'static {
    /// Stage and accept `transfer` ([`crate::handoff::accept_inbound`]).
    fn offer(&self, transfer: CharacterTransfer) -> Result<(), String>;
    /// Make an accepted transfer live ([`crate::handoff::activate_inbound`]).
    fn activate(&self, transfer_id: TransferId) -> Result<(), String>;
}

#[derive(Debug, Eq, PartialEq)]
pub enum LinkError {
    Io(String),
    TimedOut,
    /// The destination refused the request.
    Refused(String),
    /// The peer sent something that is not a link frame.
    Protocol(String),
}

impl fmt::Display for LinkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) => write!(formatter, "transfer link failed: {message}"),
            Self::TimedOut => formatter.write_str("transfer link timed out"),
            Self::Refused(reason) => write!(formatter, "destination refused: {reason}"),
            Self::Protocol(message) => write!(formatter, "transfer link protocol: {message}"),
        }
    }
}

impl std::error::Error for LinkError {}

fn io(error: std::io::Error) -> LinkError {
    LinkError::Io(error.to_string())
}

async fn write_frame<W: AsyncWrite + Unpin>(
    writer: &mut W,
    tag: u8,
    body: &[u8],
) -> Result<(), LinkError> {
    let length = u32::try_from(body.len() + 1)
        .ok()
        .filter(|length| *length as usize <= MAX_LINK_FRAME_BYTES)
        .ok_or_else(|| LinkError::Protocol("frame is too large".into()))?;
    let mut frame = Vec::with_capacity(5 + body.len());
    frame.extend_from_slice(&length.to_be_bytes());
    frame.push(tag);
    frame.extend_from_slice(body);
    writer.write_all(&frame).await.map_err(io)?;
    writer.flush().await.map_err(io)
}

/// The next frame, or `None` at a clean end of stream.
async fn read_frame<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<Option<(u8, Vec<u8>)>, LinkError> {
    let mut length = [0; 4];
    match reader.read_exact(&mut length).await {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(io(error)),
    }
    let length = u32::from_be_bytes(length) as usize;
    if length == 0 || length > MAX_LINK_FRAME_BYTES {
        return Err(LinkError::Protocol("frame length is out of range".into()));
    }
    let mut frame = vec![0; length];
    reader.read_exact(&mut frame).await.map_err(io)?;
    let tag = frame.remove(0);
    Ok(Some((tag, frame)))
}

fn handle(handler: &dyn TransferLinkHandler, tag: u8, body: &[u8]) -> Result<(), String> {
    match tag {
        OFFER => handler.offer(decode_character_transfer(body).map_err(|error| error.to_string())?),
        ACTIVATE => {
            let id = <[u8; 8]>::try_from(body)
                .map_err(|_| "activate needs exactly a transfer ID".to_owned())?;
            let id = TransferId::new(u64::from_be_bytes(id)).map_err(|error| error.to_string())?;
            handler.activate(id)
        }
        _ => Err("unknown transfer link request".into()),
    }
}

async fn serve_connection(
    mut stream: TcpStream,
    handler: Arc<dyn TransferLinkHandler>,
) -> Result<(), LinkError> {
    while let Some((tag, body)) = read_frame(&mut stream).await? {
        let handler = Arc::clone(&handler);
        // Handlers do storage I/O; keep it off the async workers.
        let reply = tokio::task::spawn_blocking(move || handle(handler.as_ref(), tag, &body))
            .await
            .map_err(|error| LinkError::Io(error.to_string()))?;
        match reply {
            Ok(()) => write_frame(&mut stream, OK, &[]).await?,
            Err(reason) => write_frame(&mut stream, REFUSED, reason.as_bytes()).await?,
        }
    }
    Ok(())
}

/// Accepts transfer link connections until the listener fails. Each
/// connection is served independently; a broken peer only ends its own.
pub async fn serve_transfer_link(
    listener: TcpListener,
    handler: Arc<dyn TransferLinkHandler>,
) -> Result<(), LinkError> {
    loop {
        let (stream, _) = listener.accept().await.map_err(io)?;
        let handler = Arc::clone(&handler);
        tokio::spawn(async move {
            // A failed connection is the peer's problem; the source retries
            // or reconciles from the handoff record.
            let _ = serve_connection(stream, handler).await;
        });
    }
}

/// A source host's connection to one destination host.
pub struct TransferLink {
    stream: TcpStream,
    timeout: Duration,
}

impl TransferLink {
    /// Connects to a destination's transfer link; every request, including
    /// this connection, gives up after `timeout`.
    pub async fn connect(
        address: impl ToSocketAddrs,
        timeout: Duration,
    ) -> Result<Self, LinkError> {
        let stream = tokio::time::timeout(timeout, TcpStream::connect(address))
            .await
            .map_err(|_| LinkError::TimedOut)?
            .map_err(io)?;
        stream.set_nodelay(true).map_err(io)?;
        Ok(Self { stream, timeout })
    }

    async fn request(&mut self, tag: u8, body: &[u8]) -> Result<(), LinkError> {
        let exchange = async {
            write_frame(&mut self.stream, tag, body).await?;
            read_frame(&mut self.stream)
                .await?
                .ok_or_else(|| LinkError::Io("the destination closed the link".into()))
        };
        let (reply, body) = tokio::time::timeout(self.timeout, exchange)
            .await
            .map_err(|_| LinkError::TimedOut)??;
        match reply {
            OK if body.is_empty() => Ok(()),
            REFUSED => Err(LinkError::Refused(
                String::from_utf8_lossy(&body).into_owned(),
            )),
            _ => Err(LinkError::Protocol("unknown transfer link reply".into())),
        }
    }

    /// Offers a frozen transfer; `Ok` means the destination accepted it.
    pub async fn offer(&mut self, transfer: &CharacterTransfer) -> Result<(), LinkError> {
        let body = encode_character_transfer(transfer)
            .map_err(|error| LinkError::Protocol(error.to_string()))?;
        self.request(OFFER, &body).await
    }

    /// Asks the destination to make a committed transfer live.
    pub async fn activate(&mut self, transfer_id: TransferId) -> Result<(), LinkError> {
        self.request(ACTIVATE, &transfer_id.get().to_be_bytes())
            .await
    }
}
