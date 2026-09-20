use mechcore_protocol::{
    Busy, Claim, EVICTED_CODE, Evicted, Hello, Operation, PROTOCOL, Request, Response,
};
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixStream, unix::OwnedWriteHalf};

const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

pub struct Client {
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: OwnedWriteHalf,
    next_id: u64,
}

/// Why an endpoint could not be turned into a live client.
///
/// The variants map onto the states in `docs/spec/mechcore/session.md`; callers must keep
/// `Busy` and `Unresponsive` distinct, because only the latter indicates a
/// wedged adapter.
pub enum ConnectError {
    /// No listener: the endpoint is absent or stale.
    Unavailable(String),
    /// A client of at least this level holds the game.
    ///
    /// `evicting` means this claim won and the game is being handed back: the
    /// endpoint is worth waiting for rather than giving up on.
    Busy { holder_level: u8, evicting: bool },
    /// Connected, but no greeting arrived.
    Unresponsive(String),
    /// Greeting arrived but did not match this build's contract.
    Protocol(String),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(message) => {
                write!(formatter, "cannot connect to adapter: {message}")
            }
            Self::Busy {
                holder_level,
                evicting,
            } => {
                if *evicting {
                    write!(
                        formatter,
                        "adapter is taking the game back from its level {holder_level} client"
                    )
                } else {
                    write!(
                        formatter,
                        "adapter is serving a client at level {holder_level}"
                    )
                }
            }
            Self::Unresponsive(message) => {
                write!(formatter, "adapter sent no greeting: {message}")
            }
            Self::Protocol(message) => formatter.write_str(message),
        }
    }
}

pub struct RequestError {
    message: String,
    fatal: bool,
    evicted: bool,
}

impl RequestError {
    fn local(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            fatal: false,
            evicted: false,
        }
    }

    fn fatal(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            fatal: true,
            evicted: false,
        }
    }

    /// The game went to a higher claim. Fatal for this connection, and the one
    /// disconnection that must not be answered by shutting the game down.
    fn evicted(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            fatal: true,
            evicted: true,
        }
    }

    pub fn is_fatal(&self) -> bool {
        self.fatal
    }

    pub fn is_evicted(&self) -> bool {
        self.evicted
    }
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Client {
    /// Connect and claim the game at `level`.
    ///
    /// The claim goes first, because the adapter cannot answer a connection
    /// until it knows what the connection is worth: it greets a client that
    /// outranks the one it is serving, and refuses one that does not.
    pub async fn connect(path: &Path, level: u8) -> Result<Self, ConnectError> {
        let stream = UnixStream::connect(path)
            .await
            .map_err(|error| ConnectError::Unavailable(error.to_string()))?;
        let (reader, writer) = stream.into_split();
        let mut client = Self {
            reader: BufReader::new(reader),
            writer,
            next_id: 1,
        };
        let mut claim = serde_json::to_vec(&Claim::current(level))
            .map_err(|error| ConnectError::Protocol(format!("cannot encode the claim: {error}")))?;
        claim.push(b'\n');
        client.writer.write_all(&claim).await.map_err(|error| {
            ConnectError::Unavailable(format!("cannot claim the game: {error}"))
        })?;
        client.writer.flush().await.map_err(|error| {
            ConnectError::Unavailable(format!("cannot flush the claim: {error}"))
        })?;
        let greeting: Value = client
            .read_line()
            .await
            .map_err(ConnectError::Unresponsive)?;
        let kind = greeting.get("kind").and_then(Value::as_str).unwrap_or("");
        if kind == "busy" {
            let busy: Busy = serde_json::from_value(greeting).map_err(|error| {
                ConnectError::Protocol(format!("cannot decode the refusal: {error}"))
            })?;
            return Err(ConnectError::Busy {
                holder_level: busy.holder_level,
                evicting: busy.evicting,
            });
        }
        if kind == "refused" {
            let reason = greeting
                .get("reason")
                .and_then(Value::as_str)
                .unwrap_or("the adapter refused the claim");
            return Err(ConnectError::Protocol(reason.into()));
        }
        let hello: Hello = serde_json::from_value(greeting)
            .map_err(|error| ConnectError::Protocol(format!("cannot decode greeting: {error}")))?;
        if hello.kind != "hello" {
            return Err(ConnectError::Protocol(format!(
                "adapter sent unexpected greeting kind {:?}",
                hello.kind
            )));
        }
        if hello.protocol != PROTOCOL {
            return Err(ConnectError::Protocol(format!(
                "adapter protocol mismatch: expected {PROTOCOL}, got {}",
                hello.protocol
            )));
        }
        if hello.capabilities != Operation::ALL {
            return Err(ConnectError::Protocol(format!(
                "adapter capability mismatch: expected {:?}, got {:?}",
                Operation::ALL,
                hello.capabilities
            )));
        }
        Ok(client)
    }

    pub async fn request(
        &mut self,
        operation: Operation,
        arguments: Value,
    ) -> Result<Value, RequestError> {
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| RequestError::local("adapter request identifier overflowed"))?;
        let mut encoded = serde_json::to_vec(&Request {
            id,
            operation,
            arguments,
        })
        .map_err(|error| RequestError::local(format!("cannot encode adapter request: {error}")))?;
        if encoded.len() + 1 > MAX_MESSAGE_BYTES {
            return Err(RequestError::local("adapter request exceeds 1 MiB"));
        }
        encoded.push(b'\n');
        self.writer.write_all(&encoded).await.map_err(|error| {
            RequestError::fatal(format!("cannot write adapter request: {error}"))
        })?;
        self.writer.flush().await.map_err(|error| {
            RequestError::fatal(format!("cannot flush adapter request: {error}"))
        })?;

        let message: Value = self.read_line().await.map_err(RequestError::fatal)?;
        if message.get("kind").and_then(Value::as_str) == Some("evicted") {
            let notice: Evicted = serde_json::from_value(message).map_err(|error| {
                RequestError::fatal(format!("cannot decode the eviction notice: {error}"))
            })?;
            return Err(RequestError::evicted(format!(
                "the game went to a level {} client",
                notice.by_level
            )));
        }
        let response: Response<Value> = serde_json::from_value(message)
            .map_err(|error| RequestError::fatal(format!("cannot decode the response: {error}")))?;
        if response.kind != "response" || response.id != id {
            return Err(RequestError::fatal(format!(
                "adapter response mismatch: expected response {id}, got {} {}",
                response.kind, response.id
            )));
        }
        if response.ok {
            response
                .result
                .ok_or_else(|| RequestError::fatal("adapter success response omitted result"))
        } else {
            let error = response
                .error
                .ok_or_else(|| RequestError::fatal("adapter failure response omitted error"))?;
            let detail = format!("{}: {}", error.code, error.message);
            if error.code == EVICTED_CODE {
                return Err(RequestError::evicted(detail));
            }
            Err(RequestError::local(detail))
        }
    }

    async fn read_line<T: for<'de> Deserialize<'de>>(&mut self) -> Result<T, String> {
        read_json_line(&mut self.reader).await
    }
}

async fn read_json_line<R, T>(reader: &mut BufReader<R>) -> Result<T, String>
where
    R: tokio::io::AsyncRead + Unpin,
    T: for<'de> Deserialize<'de>,
{
    let mut bytes = Vec::new();
    let read = reader
        .take((MAX_MESSAGE_BYTES + 1) as u64)
        .read_until(b'\n', &mut bytes)
        .await
        .map_err(|error| format!("cannot read adapter message: {error}"))?;
    if read == 0 {
        return Err("adapter disconnected".into());
    }
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err("adapter message exceeds 1 MiB".into());
    }
    serde_json::from_slice(&bytes).map_err(|error| format!("adapter sent invalid JSON: {error}"))
}
