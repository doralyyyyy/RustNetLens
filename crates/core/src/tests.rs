#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use crate::capture::redact_header;
    use crate::export::{export_bundle_json, write_export_file};
    use crate::export::{export_sessions_har_like, export_sessions_json};
    use crate::model::{
        BodyPreview, CapturedSession, HeaderPair, Rule, RuleAction, SessionKind, SessionState,
    };
    use crate::rules::{RuleEngine, merge_headers};
    use crate::store::{SessionStore, SqliteStore};

    #[test]
    fn redacts_sensitive_headers() {
        assert_eq!(redact_header("Authorization", "secret"), "<redacted>");
        assert_eq!(redact_header("X-Api-Key", "secret"), "<redacted>");
        assert_eq!(redact_header("Accept", "json"), "json");
    }

    #[test]
    fn merges_headers_case_insensitively() {
        let mut headers = vec![HeaderPair {
            name: "X-Test".into(),
            value: "old".into(),
        }];
        merge_headers(
            &mut headers,
            &[HeaderPair {
                name: "x-test".into(),
                value: "new".into(),
            }],
        );
        assert_eq!(headers[0].value, "new");
    }

    #[tokio::test]
    async fn rule_engine_matches_mock() {
        let engine = RuleEngine::default();
        engine
            .set_rules(vec![Rule {
                id: "rule-1".into(),
                name: "mock".into(),
                enabled: true,
                priority: 1,
                match_: crate::model::RuleMatch {
                    url_contains: Some("/api/user".into()),
                    method: Some("GET".into()),
                    host: None,
                    status: None,
                },
                action: RuleAction::MockResponse {
                    status: 200,
                    headers: vec![HeaderPair {
                        name: "content-type".into(),
                        value: "application/json".into(),
                    }],
                    body: "{\"ok\":true}".into(),
                },
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            }])
            .await;
        let mut session = CapturedSession::new(SessionKind::Http);
        session.method = Some("GET".into());
        session.url = Some("http://localhost/api/user".into());
        let mut ctx = crate::model::RequestContext {
            session,
            request_headers: vec![],
            request_body: vec![],
            should_mock: false,
            delay_ms: None,
            rewrite_request_headers: vec![],
            matched_rule_ids: vec![],
        };
        let result = engine.apply_request(&mut ctx).await;
        assert!(result.mock_response.is_some());
        assert_eq!(ctx.matched_rule_ids, vec!["rule-1"]);
    }

    #[tokio::test]
    async fn sqlite_store_roundtrip() {
        let dir = tempdir().unwrap();
        let store = SqliteStore::new(dir.path().join("data.sqlite3"))
            .await
            .unwrap();
        let mut session = CapturedSession::new(SessionKind::Http);
        session.state = SessionState::Completed;
        session.method = Some("GET".into());
        session.url = Some("http://example.com".into());
        session.host = Some("example.com".into());
        session.request_headers = vec![HeaderPair {
            name: "Accept".into(),
            value: "text/plain".into(),
        }];
        session.request_body = BodyPreview {
            content_type: Some("text/plain".into()),
            truncated: false,
            size: 4,
            text: Some("demo".into()),
            base64: None,
        };
        store.insert_session(&session).await.unwrap();
        let loaded = store.get_session(session.id).await.unwrap().unwrap();
        assert_eq!(loaded.url, session.url);
        assert_eq!(loaded.request_body.text.as_deref(), Some("demo"));
    }

    #[test]
    fn export_json_and_har_like_work() {
        let mut session = CapturedSession::new(SessionKind::Http);
        session.state = SessionState::Completed;
        session.method = Some("GET".into());
        session.url = Some("http://example.com".into());
        let json = export_sessions_json(vec![session.clone()]).unwrap();
        let har = export_sessions_har_like(vec![session]).unwrap();
        assert!(json.contains("example.com"));
        assert!(har.contains("\"log\""));
    }

    #[test]
    fn export_bundle_contains_metadata() {
        let session = CapturedSession::new(SessionKind::Http);
        let json = export_bundle_json(vec![session]).unwrap();
        assert!(json.contains("rustnetlens-export-v1"));
    }

    #[test]
    fn write_export_file_creates_file() {
        let dir = tempdir().unwrap();
        let path = write_export_file(dir.path(), "payload", "json").unwrap();
        assert!(path.exists());
    }

    #[tokio::test]
    async fn session_filtering_works() {
        let dir = tempdir().unwrap();
        let store = SqliteStore::new(dir.path().join("filter.sqlite3"))
            .await
            .unwrap();
        let mut session = CapturedSession::new(SessionKind::Http);
        session.state = SessionState::Mocked;
        session.method = Some("POST".into());
        session.url = Some("http://example.com/api/user".into());
        session.host = Some("example.com".into());
        session.status = Some(200);
        store.insert_session(&session).await.unwrap();
        let sessions = store
            .list_sessions(
                crate::model::SessionFilter {
                    keyword: Some("api/user".into()),
                    method: Some("POST".into()),
                    status: Some(200),
                    host: Some("example.com".into()),
                    only_failed: false,
                    only_mocked: true,
                },
                10,
                0,
            )
            .await
            .unwrap();
        assert_eq!(sessions.len(), 1);
    }
}
