use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Row, params};
use uuid::Uuid;

use crate::error::StoreError;
use crate::model::{
    CapturedSession, Rule, SessionFilter, SessionId, SessionKind, SessionState, SessionSummary,
};

#[async_trait::async_trait]
pub trait SessionStore: Send + Sync + 'static {
    async fn insert_session(&self, session: &CapturedSession) -> Result<(), StoreError>;
    async fn update_session(&self, session: &CapturedSession) -> Result<(), StoreError>;
    async fn get_session(&self, id: SessionId) -> Result<Option<CapturedSession>, StoreError>;
    async fn list_sessions(
        &self,
        filter: SessionFilter,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<SessionSummary>, StoreError>;
    async fn get_sessions_by_ids(
        &self,
        ids: &[SessionId],
    ) -> Result<Vec<CapturedSession>, StoreError>;
    async fn clear_sessions(&self) -> Result<(), StoreError>;
}

#[derive(Clone)]
pub struct SqliteStore {
    db_path: PathBuf,
    conn_lock: Arc<Mutex<()>>,
}

impl SqliteStore {
    pub async fn new(db_path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let store = Self {
            db_path: db_path.as_ref().to_path_buf(),
            conn_lock: Arc::new(Mutex::new(())),
        };
        store.initialize()?;
        Ok(store)
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub async fn list_rules(&self) -> Result<Vec<Rule>, StoreError> {
        self.with_conn(|conn| load_rules(conn))
    }

    pub async fn save_rule(&self, rule: &Rule) -> Result<(), StoreError> {
        let rule = rule.clone();
        self.with_conn(move |conn| upsert_rule(conn, &rule))
    }

    pub async fn delete_rule(&self, id: &str) -> Result<(), StoreError> {
        let id = id.to_string();
        self.with_conn(move |conn| {
            conn.execute("DELETE FROM rules WHERE id = ?1", params![id])
                .map_err(db_err)?;
            Ok(())
        })
    }

    pub async fn replace_rules(&self, rules: &[Rule]) -> Result<(), StoreError> {
        let rules = rules.to_vec();
        self.with_conn(move |conn| {
            conn.execute("DELETE FROM rules", []).map_err(db_err)?;
            for rule in &rules {
                upsert_rule(conn, rule)?;
            }
            Ok(())
        })
    }

    fn initialize(&self) -> Result<(), StoreError> {
        let conn = Connection::open(&self.db_path).map_err(db_err)?;
        init_schema(&conn)?;
        Ok(())
    }

    fn with_conn<T, F>(&self, f: F) -> Result<T, StoreError>
    where
        F: FnOnce(&Connection) -> Result<T, StoreError>,
    {
        let _guard = self
            .conn_lock
            .lock()
            .map_err(|e| StoreError::Database(e.to_string()))?;
        let conn = Connection::open(&self.db_path).map_err(db_err)?;
        init_schema(&conn)?;
        f(&conn)
    }
}

#[async_trait::async_trait]
impl SessionStore for SqliteStore {
    async fn insert_session(&self, session: &CapturedSession) -> Result<(), StoreError> {
        let session = session.clone();
        self.with_conn(move |conn| upsert_session(conn, &session))
    }

    async fn update_session(&self, session: &CapturedSession) -> Result<(), StoreError> {
        let session = session.clone();
        self.with_conn(move |conn| upsert_session(conn, &session))
    }

    async fn get_session(&self, id: SessionId) -> Result<Option<CapturedSession>, StoreError> {
        self.with_conn(move |conn| load_session(conn, id))
    }

    async fn list_sessions(
        &self,
        filter: SessionFilter,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<SessionSummary>, StoreError> {
        self.with_conn(move |conn| load_sessions(conn, filter, limit, offset))
    }

    async fn get_sessions_by_ids(
        &self,
        ids: &[SessionId],
    ) -> Result<Vec<CapturedSession>, StoreError> {
        let ids = ids.to_vec();
        self.with_conn(move |conn| {
            let mut out = Vec::new();
            for id in ids {
                if let Some(session) = load_session(conn, id)? {
                    out.push(session);
                }
            }
            Ok(out)
        })
    }

    async fn clear_sessions(&self) -> Result<(), StoreError> {
        self.with_conn(|conn| {
            conn.execute("DELETE FROM session_details", [])
                .map_err(db_err)?;
            conn.execute("DELETE FROM sessions", []).map_err(db_err)?;
            Ok(())
        })
    }
}

fn db_err(err: rusqlite::Error) -> StoreError {
    StoreError::Database(err.to_string())
}

fn init_schema(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        r#"
        PRAGMA foreign_keys = ON;
        CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            state TEXT NOT NULL,
            started_at TEXT NOT NULL,
            ended_at TEXT,
            duration_ms INTEGER,
            method TEXT,
            url TEXT,
            scheme TEXT,
            host TEXT,
            port INTEGER,
            path TEXT,
            status INTEGER,
            bytes_up INTEGER NOT NULL DEFAULT 0,
            bytes_down INTEGER NOT NULL DEFAULT 0,
            error TEXT,
            matched_rule_ids TEXT NOT NULL DEFAULT '[]',
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS session_details (
            session_id TEXT PRIMARY KEY,
            request_headers_json TEXT NOT NULL,
            response_headers_json TEXT NOT NULL,
            request_body_json TEXT NOT NULL,
            response_body_json TEXT NOT NULL,
            raw_json TEXT NOT NULL,
            FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
        );
        CREATE TABLE IF NOT EXISTS rules (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            enabled INTEGER NOT NULL,
            priority INTEGER NOT NULL DEFAULT 100,
            rule_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_sessions_started_at ON sessions(started_at DESC);
        CREATE INDEX IF NOT EXISTS idx_sessions_host ON sessions(host);
        CREATE INDEX IF NOT EXISTS idx_sessions_method ON sessions(method);
        CREATE INDEX IF NOT EXISTS idx_sessions_status ON sessions(status);
        "#,
    )
    .map_err(db_err)?;
    Ok(())
}

fn upsert_session(conn: &Connection, session: &CapturedSession) -> Result<(), StoreError> {
    let raw_json =
        serde_json::to_string(session).map_err(|e| StoreError::Serialization(e.to_string()))?;
    let request_headers_json = serde_json::to_string(&session.request_headers)
        .map_err(|e| StoreError::Serialization(e.to_string()))?;
    let response_headers_json = serde_json::to_string(&session.response_headers)
        .map_err(|e| StoreError::Serialization(e.to_string()))?;
    let request_body_json = serde_json::to_string(&session.request_body)
        .map_err(|e| StoreError::Serialization(e.to_string()))?;
    let response_body_json = serde_json::to_string(&session.response_body)
        .map_err(|e| StoreError::Serialization(e.to_string()))?;
    conn.execute(
        r#"
        INSERT INTO sessions (
            id, kind, state, started_at, ended_at, duration_ms, method, url, scheme, host, port, path,
            status, bytes_up, bytes_down, error, matched_rule_ids, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
        ON CONFLICT(id) DO UPDATE SET
            kind=excluded.kind,
            state=excluded.state,
            started_at=excluded.started_at,
            ended_at=excluded.ended_at,
            duration_ms=excluded.duration_ms,
            method=excluded.method,
            url=excluded.url,
            scheme=excluded.scheme,
            host=excluded.host,
            port=excluded.port,
            path=excluded.path,
            status=excluded.status,
            bytes_up=excluded.bytes_up,
            bytes_down=excluded.bytes_down,
            error=excluded.error,
            matched_rule_ids=excluded.matched_rule_ids,
            created_at=excluded.created_at
        "#,
        params![
            session.id.to_string(),
            format!("{:?}", session.kind),
            format!("{:?}", session.state),
            session.started_at.to_rfc3339(),
            session.ended_at.map(|v| v.to_rfc3339()),
            session.duration_ms.map(|v| v as i64),
            session.method.clone(),
            session.url.clone(),
            session.scheme.clone(),
            session.host.clone(),
            session.port.map(|v| v as i64),
            session.path.clone(),
            session.status.map(|v| v as i64),
            session.bytes_up as i64,
            session.bytes_down as i64,
            session.error.clone(),
            serde_json::to_string(&session.matched_rule_ids)
                .map_err(|e| StoreError::Serialization(e.to_string()))?,
            Utc::now().to_rfc3339(),
        ],
    )
    .map_err(db_err)?;
    conn.execute(
        r#"
        INSERT INTO session_details (
            session_id, request_headers_json, response_headers_json, request_body_json, response_body_json, raw_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT(session_id) DO UPDATE SET
            request_headers_json=excluded.request_headers_json,
            response_headers_json=excluded.response_headers_json,
            request_body_json=excluded.request_body_json,
            response_body_json=excluded.response_body_json,
            raw_json=excluded.raw_json
        "#,
        params![
            session.id.to_string(),
            request_headers_json,
            response_headers_json,
            request_body_json,
            response_body_json,
            raw_json,
        ],
    )
    .map_err(db_err)?;
    Ok(())
}

fn load_session(conn: &Connection, id: SessionId) -> Result<Option<CapturedSession>, StoreError> {
    let row = conn
        .query_row(
            r#"
            SELECT id, kind, state, started_at, ended_at, duration_ms, method, url, scheme, host, port,
                   path, status, bytes_up, bytes_down, error, matched_rule_ids
            FROM sessions WHERE id = ?1
            "#,
            params![id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, Option<i64>>(12)?,
                    row.get::<_, i64>(13)?,
                    row.get::<_, i64>(14)?,
                    row.get::<_, Option<String>>(15)?,
                    row.get::<_, String>(16)?,
                ))
            },
        )
        .optional()
        .map_err(db_err)?;
    let Some((
        id,
        kind,
        state,
        started_at,
        ended_at,
        duration_ms,
        method,
        url,
        scheme,
        host,
        port,
        path,
        status,
        bytes_up,
        bytes_down,
        error,
        matched,
    )) = row
    else {
        return Ok(None);
    };
    let details = conn
        .query_row(
            "SELECT request_headers_json, response_headers_json, request_body_json, response_body_json FROM session_details WHERE session_id = ?1",
            params![id.clone()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()
        .map_err(db_err)?;
    let (request_headers_json, response_headers_json, request_body_json, response_body_json) =
        details.unwrap_or_else(|| ("[]".into(), "[]".into(), "{}".into(), "{}".into()));
    let session = CapturedSession {
        id: Uuid::parse_str(&id).map_err(|e| StoreError::Serialization(e.to_string()))?,
        kind: parse_kind(&kind)?,
        state: parse_state(&state)?,
        started_at: DateTime::parse_from_rfc3339(&started_at)
            .map_err(|e| StoreError::Serialization(e.to_string()))?
            .with_timezone(&Utc),
        ended_at: ended_at
            .map(|v| {
                DateTime::parse_from_rfc3339(&v)
                    .map(|dt| dt.with_timezone(&Utc))
                    .map_err(|e| StoreError::Serialization(e.to_string()))
            })
            .transpose()?,
        duration_ms: duration_ms.map(|v| v as u64),
        method,
        url,
        scheme,
        host,
        port: port.map(|v| v as u16),
        path,
        status: status.map(|v| v as u16),
        request_headers: serde_json::from_str(&request_headers_json)
            .map_err(|e| StoreError::Serialization(e.to_string()))?,
        response_headers: serde_json::from_str(&response_headers_json)
            .map_err(|e| StoreError::Serialization(e.to_string()))?,
        request_body: serde_json::from_str(&request_body_json)
            .map_err(|e| StoreError::Serialization(e.to_string()))?,
        response_body: serde_json::from_str(&response_body_json)
            .map_err(|e| StoreError::Serialization(e.to_string()))?,
        bytes_up: bytes_up.max(0) as u64,
        bytes_down: bytes_down.max(0) as u64,
        error,
        matched_rule_ids: serde_json::from_str(&matched).unwrap_or_default(),
    };
    Ok(Some(session))
}

fn load_sessions(
    conn: &Connection,
    filter: SessionFilter,
    limit: u32,
    offset: u32,
) -> Result<Vec<SessionSummary>, StoreError> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT id, kind, state, started_at, method, url, host, status, duration_ms, bytes_up, bytes_down, matched_rule_ids
            FROM sessions ORDER BY started_at DESC LIMIT ?1 OFFSET ?2
            "#,
        )
        .map_err(db_err)?;
    let rows = stmt
        .query_map(params![limit as i64, offset as i64], |row| {
            read_summary(row)
        })
        .map_err(db_err)?;
    let mut out = Vec::new();
    for row in rows {
        let (
            id,
            kind,
            state,
            started_at,
            method,
            url,
            host,
            status,
            duration_ms,
            bytes_up,
            bytes_down,
            matched,
        ) = row.map_err(db_err)?;
        let summary = SessionSummary {
            id: Uuid::parse_str(&id).map_err(|e| StoreError::Serialization(e.to_string()))?,
            kind: parse_kind(&kind)?,
            state: parse_state(&state)?,
            started_at: DateTime::parse_from_rfc3339(&started_at)
                .map_err(|e| StoreError::Serialization(e.to_string()))?
                .with_timezone(&Utc),
            method,
            url,
            host,
            status: status.map(|v| v as u16),
            duration_ms: duration_ms.map(|v| v as u64),
            bytes_up: bytes_up.max(0) as u64,
            bytes_down: bytes_down.max(0) as u64,
            matched_rule_ids: serde_json::from_str(&matched).unwrap_or_default(),
        };
        if session_matches_filter(&summary, &filter) {
            out.push(summary);
        }
    }
    Ok(out)
}

fn read_summary(
    row: &Row<'_>,
) -> rusqlite::Result<(
    String,
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<i64>,
    i64,
    i64,
    String,
)> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
        row.get(11)?,
    ))
}

fn session_matches_filter(summary: &SessionSummary, filter: &SessionFilter) -> bool {
    if let Some(host) = &filter.host {
        if summary
            .host
            .as_deref()
            .map(|value| !value.contains(host))
            .unwrap_or(true)
        {
            return false;
        }
    }
    if let Some(method) = &filter.method {
        if summary
            .method
            .as_deref()
            .map(|value| !value.eq_ignore_ascii_case(method))
            .unwrap_or(true)
        {
            return false;
        }
    }
    if let Some(status) = filter.status {
        if summary.status != Some(status) {
            return false;
        }
    }
    if filter.only_failed && summary.state != SessionState::Failed {
        return false;
    }
    if filter.only_mocked && summary.state != SessionState::Mocked {
        return false;
    }
    if let Some(keyword) = &filter.keyword {
        let keyword = keyword.to_ascii_lowercase();
        let target = format!(
            "{} {} {}",
            summary.method.clone().unwrap_or_default(),
            summary.url.clone().unwrap_or_default(),
            summary.host.clone().unwrap_or_default()
        )
        .to_ascii_lowercase();
        if !target.contains(&keyword) {
            return false;
        }
    }
    true
}

fn parse_kind(kind: &str) -> Result<SessionKind, StoreError> {
    match kind {
        "Http" => Ok(SessionKind::Http),
        "ConnectTunnel" => Ok(SessionKind::ConnectTunnel),
        "WebSocket" => Ok(SessionKind::WebSocket),
        other => Err(StoreError::Serialization(format!(
            "unknown session kind: {other}"
        ))),
    }
}

fn parse_state(state: &str) -> Result<SessionState, StoreError> {
    match state {
        "Pending" => Ok(SessionState::Pending),
        "Completed" => Ok(SessionState::Completed),
        "Failed" => Ok(SessionState::Failed),
        "Mocked" => Ok(SessionState::Mocked),
        "Tunneling" => Ok(SessionState::Tunneling),
        other => Err(StoreError::Serialization(format!(
            "unknown session state: {other}"
        ))),
    }
}

fn load_rules(conn: &Connection) -> Result<Vec<Rule>, StoreError> {
    let mut stmt = conn
        .prepare("SELECT rule_json FROM rules ORDER BY priority ASC, created_at ASC")
        .map_err(db_err)?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(db_err)?;
    let mut out = Vec::new();
    for row in rows {
        let json = row.map_err(db_err)?;
        let rule: Rule =
            serde_json::from_str(&json).map_err(|e| StoreError::Serialization(e.to_string()))?;
        out.push(rule);
    }
    Ok(out)
}

fn upsert_rule(conn: &Connection, rule: &Rule) -> Result<(), StoreError> {
    let json = serde_json::to_string(rule).map_err(|e| StoreError::Serialization(e.to_string()))?;
    conn.execute(
        r#"
        INSERT INTO rules (id, name, enabled, priority, rule_json, created_at, updated_at)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
        ON CONFLICT(id) DO UPDATE SET
            name=excluded.name,
            enabled=excluded.enabled,
            priority=excluded.priority,
            rule_json=excluded.rule_json,
            created_at=excluded.created_at,
            updated_at=excluded.updated_at
        "#,
        params![
            rule.id,
            rule.name,
            rule.enabled as i64,
            rule.priority,
            json,
            rule.created_at.to_rfc3339(),
            rule.updated_at.to_rfc3339(),
        ],
    )
    .map_err(db_err)?;
    Ok(())
}
