use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("failed to bind proxy address {addr}: {source}")]
    BindFailed {
        addr: std::net::SocketAddr,
        source: std::io::Error,
    },
    #[error("failed to connect upstream {target}: {source}")]
    UpstreamConnectFailed {
        target: String,
        source: std::io::Error,
    },
    #[error("invalid proxy request: {0}")]
    InvalidRequest(String),
    #[error("http error: {0}")]
    Http(String),
    #[error("store error: {0}")]
    Store(String),
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(String),
    #[error("serialization error: {0}")]
    Serialization(String),
    #[error("not found")]
    NotFound,
}

#[derive(Debug, Error)]
pub enum RuleError {
    #[error("invalid rule: {0}")]
    InvalidRule(String),
}

#[derive(Debug, Error)]
pub enum ReplayError {
    #[error("session not replayable")]
    NotReplayable,
    #[error("request execution failed: {0}")]
    Request(String),
    #[error("store error: {0}")]
    Store(String),
}
