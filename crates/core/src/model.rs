use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub type SessionId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SessionKind {
    Http,
    ConnectTunnel,
    WebSocket,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SessionState {
    Pending,
    Completed,
    Failed,
    Mocked,
    Tunneling,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HeaderPair {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct BodyPreview {
    pub content_type: Option<String>,
    #[serde(default)]
    pub truncated: bool,
    pub size: u64,
    #[serde(default)]
    pub encoding: Option<String>,
    #[serde(default)]
    pub pretty: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub base64: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TrailersPreview {
    pub headers: Vec<HeaderPair>,
}

impl BodyPreview {
    pub fn empty() -> Self {
        Self {
            content_type: None,
            truncated: false,
            size: 0,
            encoding: None,
            pretty: None,
            text: None,
            base64: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TimelineEntry {
    pub name: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebSocketFramePreview {
    pub direction: String,
    pub opcode: String,
    pub size: u64,
    pub text: Option<String>,
    pub base64: Option<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedSession {
    pub id: SessionId,
    pub kind: SessionKind,
    pub state: SessionState,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub duration_ms: Option<u64>,
    pub method: Option<String>,
    pub url: Option<String>,
    pub scheme: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub path: Option<String>,
    pub status: Option<u16>,
    pub request_headers: Vec<HeaderPair>,
    pub response_headers: Vec<HeaderPair>,
    pub request_body: BodyPreview,
    pub response_body: BodyPreview,
    #[serde(default)]
    pub grpc_request_metadata: Vec<HeaderPair>,
    #[serde(default)]
    pub grpc_response_metadata: Vec<HeaderPair>,
    #[serde(default)]
    pub grpc_request_trailers: TrailersPreview,
    #[serde(default)]
    pub grpc_response_trailers: TrailersPreview,
    #[serde(default)]
    pub timeline: Vec<TimelineEntry>,
    #[serde(default)]
    pub websocket_frames: Vec<WebSocketFramePreview>,
    pub bytes_up: u64,
    pub bytes_down: u64,
    pub error: Option<String>,
    pub matched_rule_ids: Vec<String>,
}

impl CapturedSession {
    pub fn new(kind: SessionKind) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            kind,
            state: SessionState::Pending,
            started_at: now,
            ended_at: None,
            duration_ms: None,
            method: None,
            url: None,
            scheme: None,
            host: None,
            port: None,
            path: None,
            status: None,
            request_headers: Vec::new(),
            response_headers: Vec::new(),
            request_body: BodyPreview::empty(),
            response_body: BodyPreview::empty(),
            grpc_request_metadata: Vec::new(),
            grpc_response_metadata: Vec::new(),
            grpc_request_trailers: TrailersPreview::default(),
            grpc_response_trailers: TrailersPreview::default(),
            timeline: Vec::new(),
            websocket_frames: Vec::new(),
            bytes_up: 0,
            bytes_down: 0,
            error: None,
            matched_rule_ids: Vec::new(),
        }
    }

    pub fn finish(&mut self, state: SessionState) {
        let now = Utc::now();
        self.ended_at = Some(now);
        self.duration_ms = Some((now - self.started_at).num_milliseconds().max(0) as u64);
        self.state = state;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProxyStatus {
    pub running: bool,
    pub listen_addr: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub active_sessions: usize,
    pub https_mitm: HttpsMitmStatus,
}

impl ProxyStatus {
    pub fn stopped() -> Self {
        Self {
            running: false,
            listen_addr: None,
            started_at: None,
            active_sessions: 0,
            https_mitm: HttpsMitmStatus::disabled(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RootCaInfo {
    pub generated_at: DateTime<Utc>,
    pub cert_path: String,
    pub fingerprint_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HttpsMitmStatus {
    pub enabled: bool,
    pub ready: bool,
    pub local_only: bool,
    pub default_off: bool,
    pub root_ca: Option<RootCaInfo>,
    pub install_hint: String,
}

impl HttpsMitmStatus {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ready: false,
            local_only: true,
            default_off: true,
            root_ca: None,
            install_hint: "HTTPS decrypt is disabled by default. Install the local Root CA only on machines you control.".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionFilter {
    pub keyword: Option<String>,
    pub method: Option<String>,
    pub status: Option<u16>,
    pub host: Option<String>,
    pub only_failed: bool,
    pub only_mocked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: SessionId,
    pub kind: SessionKind,
    pub state: SessionState,
    pub started_at: DateTime<Utc>,
    pub method: Option<String>,
    pub url: Option<String>,
    pub host: Option<String>,
    pub status: Option<u16>,
    pub duration_ms: Option<u64>,
    pub bytes_up: u64,
    pub bytes_down: u64,
    pub matched_rule_ids: Vec<String>,
}

impl SessionSummary {
    pub fn from_session(session: &CapturedSession) -> Self {
        Self {
            id: session.id,
            kind: session.kind.clone(),
            state: session.state.clone(),
            started_at: session.started_at,
            method: session.method.clone(),
            url: session.url.clone(),
            host: session.host.clone(),
            status: session.status,
            duration_ms: session.duration_ms,
            bytes_up: session.bytes_up,
            bytes_down: session.bytes_down,
            matched_rule_ids: session.matched_rule_ids.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuleMatch {
    pub url_contains: Option<String>,
    pub method: Option<String>,
    pub host: Option<String>,
    pub status: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuleAction {
    RewriteRequestHeaders {
        headers: Vec<HeaderPair>,
    },
    RewriteResponseHeaders {
        headers: Vec<HeaderPair>,
    },
    MockResponse {
        status: u16,
        headers: Vec<HeaderPair>,
        body: String,
    },
    Delay {
        millis: u64,
    },
    Script {
        script: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CollectionItem {
    pub id: String,
    pub name: String,
    pub session_id: Option<SessionId>,
    pub method: Option<String>,
    pub url: Option<String>,
    pub headers: Vec<HeaderPair>,
    pub body: BodyPreview,
    pub created_at: DateTime<Utc>,
}

impl CollectionItem {
    pub fn from_session(session: &CapturedSession) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            name: session
                .url
                .clone()
                .or_else(|| session.host.clone())
                .unwrap_or_else(|| "Captured request".into()),
            session_id: Some(session.id),
            method: session.method.clone(),
            url: session.url.clone(),
            headers: session.request_headers.clone(),
            body: session.request_body.clone(),
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RequestCollection {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub items: Vec<CollectionItem>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl RequestCollection {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: id.into(),
            name: name.into(),
            description: None,
            items: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn add_session(&mut self, session: &CapturedSession) {
        self.items.push(CollectionItem::from_session(session));
        self.updated_at = Utc::now();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Rule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub priority: i64,
    #[serde(rename = "match")]
    pub match_: RuleMatch,
    pub action: RuleAction,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Rule {
    pub fn new(id: impl Into<String>, name: impl Into<String>, action: RuleAction) -> Self {
        let now = Utc::now();
        Self {
            id: id.into(),
            name: name.into(),
            enabled: true,
            priority: 100,
            match_: RuleMatch {
                url_contains: None,
                method: None,
                host: None,
                status: None,
            },
            action,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestContext {
    pub session: CapturedSession,
    pub request_headers: Vec<HeaderPair>,
    pub request_body: Vec<u8>,
    pub request_trailers: Vec<HeaderPair>,
    pub should_mock: bool,
    pub delay_ms: Option<u64>,
    pub rewrite_request_headers: Vec<HeaderPair>,
    pub rewrite_request_trailers: Vec<HeaderPair>,
    pub matched_rule_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseContext {
    pub session: CapturedSession,
    pub response_headers: Vec<HeaderPair>,
    pub response_body: Vec<u8>,
    pub response_trailers: Vec<HeaderPair>,
    pub delay_ms: Option<u64>,
    pub rewrite_response_headers: Vec<HeaderPair>,
    pub rewrite_response_trailers: Vec<HeaderPair>,
    pub matched_rule_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureEvent {
    pub session: CapturedSession,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct TrafficOverview {
    pub total_sessions: u64,
    pub total_bytes_up: u64,
    pub total_bytes_down: u64,
    pub average_duration_ms: Option<u64>,
    pub p95_duration_ms: Option<u64>,
    pub by_host: Vec<TrafficBucket>,
    pub by_method: Vec<TrafficBucket>,
    pub by_status: Vec<TrafficBucket>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrafficBucket {
    pub key: String,
    pub count: u64,
    pub bytes_up: u64,
    pub bytes_down: u64,
    pub average_duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportBundle {
    pub format: String,
    pub exported_at: DateTime<Utc>,
    pub sessions: Vec<CapturedSession>,
}
