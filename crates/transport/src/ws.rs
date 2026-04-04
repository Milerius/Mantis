//! WebSocket connection management with TLS and reconnection.

use std::net::TcpStream;
use std::time::Duration;

use tracing::{debug, info};
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

/// Configuration for a WebSocket connection.
#[derive(Clone, Debug)]
pub struct WsConfig {
    /// WebSocket URL (e.g., `wss://ws-subscriptions-clob.polymarket.com/ws/market`).
    pub url: String,
    /// Subscription message to send after connecting (JSON string).
    pub subscribe_msg: Option<String>,
    /// Text heartbeat interval. `Some(10s)` sends `"PING"` every 10s (Polymarket).
    /// `None` disables text pings (Binance — uses WS-level ping/pong automatically).
    pub ping_interval: Option<Duration>,
    /// TCP read timeout. `None` means block indefinitely.
    pub read_timeout: Option<Duration>,
}

impl WsConfig {
    /// Create a config with Polymarket defaults (10s ping).
    #[must_use]
    pub fn polymarket(url: &str) -> Self {
        Self {
            url: url.to_owned(),
            subscribe_msg: None,
            ping_interval: Some(Duration::from_secs(10)),
            read_timeout: Some(Duration::from_secs(15)),
        }
    }

    /// Create a config with Binance defaults (no ping required, URL-based subscription).
    #[must_use]
    pub fn binance(url: &str) -> Self {
        Self {
            url: url.to_owned(),
            subscribe_msg: None,
            ping_interval: None,
            read_timeout: Some(Duration::from_secs(35)),
        }
    }
}

/// A managed WebSocket connection with heartbeat support.
pub struct WsConnection {
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    config: WsConfig,
    last_ping: std::time::Instant,
}

impl WsConnection {
    /// Connect to the WebSocket endpoint, send subscription message if configured.
    ///
    /// # Errors
    ///
    /// Returns an error if TCP connect, TLS handshake, or WS upgrade fails.
    pub fn connect(config: &WsConfig) -> Result<Self, WsError> {
        let request = config
            .url
            .as_str()
            .into_client_request()
            .map_err(|e| WsError::Connect(format!("invalid URL: {e}")))?;

        info!(url = %config.url, "connecting to WebSocket");

        let (ws, response) =
            tungstenite::connect(request).map_err(|e| WsError::Connect(format!("{e}")))?;

        debug!(
            status = %response.status(),
            "WebSocket connected"
        );

        // Set TCP read timeout and nodelay on the underlying TCP stream
        let tcp_result = match ws.get_ref() {
            MaybeTlsStream::Plain(tcp) => Some(tcp),
            MaybeTlsStream::Rustls(tls) => Some(tls.get_ref()),
            _ => None,
        };
        if let Some(tcp) = tcp_result {
            tcp.set_read_timeout(config.read_timeout)
                .map_err(|e| WsError::Connect(format!("set_read_timeout: {e}")))?;
            tcp.set_nodelay(true)
                .map_err(|e| WsError::Connect(format!("set_nodelay: {e}")))?;
        }

        let mut conn = Self {
            ws,
            config: config.clone(),
            last_ping: std::time::Instant::now(),
        };

        // Send subscription message
        if let Some(ref sub) = config.subscribe_msg {
            conn.ws
                .send(Message::Text(sub.clone().into()))
                .map_err(|e| WsError::Send(format!("subscribe: {e}")))?;
            debug!("subscription message sent");
        }

        Ok(conn)
    }

    /// Read the next text message from the WebSocket.
    ///
    /// Handles ping/pong internally. Sends periodic PING heartbeats.
    /// Returns `None` for non-text messages (binary, pong, close).
    ///
    /// # Errors
    ///
    /// Returns `WsError::Read` on read failure or `WsError::Closed` if
    /// the connection was closed by the server.
    pub fn read_text(&mut self) -> Result<Option<String>, WsError> {
        // Send heartbeat if due
        self.maybe_send_ping()?;

        match self.ws.read() {
            Ok(Message::Text(text)) => {
                let text = text.to_string();
                if text == "PONG" {
                    return Ok(None);
                }
                Ok(Some(text))
            }
            Ok(Message::Ping(data)) => {
                self.ws
                    .send(Message::Pong(data))
                    .map_err(|e| WsError::Send(format!("pong: {e}")))?;
                Ok(None)
            }
            Ok(Message::Pong(_) | Message::Binary(_) | Message::Frame(_)) => Ok(None),
            Ok(Message::Close(frame)) => {
                debug!(?frame, "server sent close");
                Err(WsError::Closed)
            }
            Err(tungstenite::Error::Io(ref e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                // Read timeout — send text ping if venue requires it
                if self.config.ping_interval.is_some() {
                    self.send_ping()?;
                }
                Ok(None)
            }
            Err(e) => Err(WsError::Read(format!("{e}"))),
        }
    }

    /// Get a mutable reference to the underlying tungstenite socket.
    ///
    /// Useful for sending messages (e.g., dynamic subscription updates).
    pub fn inner_mut(&mut self) -> &mut WebSocket<MaybeTlsStream<TcpStream>> {
        &mut self.ws
    }

    fn maybe_send_ping(&mut self) -> Result<(), WsError> {
        if let Some(interval) = self.config.ping_interval
            && self.last_ping.elapsed() >= interval
        {
            self.send_ping()?;
        }
        Ok(())
    }

    fn send_ping(&mut self) -> Result<(), WsError> {
        self.ws
            .send(Message::Text("PING".into()))
            .map_err(|e| WsError::Send(format!("ping: {e}")))?;
        self.last_ping = std::time::Instant::now();
        Ok(())
    }
}

/// Errors from WebSocket operations.
#[derive(Debug)]
pub enum WsError {
    /// Connection or handshake failed.
    Connect(String),
    /// Read from socket failed.
    Read(String),
    /// Send to socket failed.
    Send(String),
    /// Server closed the connection.
    Closed,
}

impl std::fmt::Display for WsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connect(msg) => write!(f, "connect: {msg}"),
            Self::Read(msg) => write!(f, "read: {msg}"),
            Self::Send(msg) => write!(f, "send: {msg}"),
            Self::Closed => write!(f, "connection closed"),
        }
    }
}

impl std::error::Error for WsError {}
