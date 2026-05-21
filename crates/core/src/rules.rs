use std::sync::Arc;

use rhai::serde::{from_dynamic, to_dynamic};
use rhai::{Array, Dynamic, Engine, Map, Scope};
use tokio::sync::RwLock;
use tokio::time::Duration;

use crate::error::RuleError;
use crate::model::{
    CapturedSession, HeaderPair, RequestContext, ResponseContext, Rule, RuleAction,
};

#[derive(Clone, Default)]
pub struct RuleEngine {
    rules: Arc<RwLock<Vec<Rule>>>,
}

#[derive(Debug, Clone, Default)]
pub struct RuleResult {
    pub matched_rule_ids: Vec<String>,
    pub rewrite_request_headers: Vec<HeaderPair>,
    pub rewrite_response_headers: Vec<HeaderPair>,
    pub rewrite_request_trailers: Vec<HeaderPair>,
    pub rewrite_response_trailers: Vec<HeaderPair>,
    pub mock_response: Option<(u16, Vec<HeaderPair>, String)>,
    pub delay_ms: Option<u64>,
}

impl RuleEngine {
    pub async fn set_rules(&self, mut rules: Vec<Rule>) {
        rules.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.id.cmp(&b.id)));
        *self.rules.write().await = rules;
    }

    pub async fn get_rules(&self) -> Vec<Rule> {
        self.rules.read().await.clone()
    }

    pub async fn validate_rule(rule: &Rule) -> Result<(), RuleError> {
        if rule.id.trim().is_empty() {
            return Err(RuleError::InvalidRule("id cannot be empty".into()));
        }
        if rule.name.trim().is_empty() {
            return Err(RuleError::InvalidRule("name cannot be empty".into()));
        }
        if let RuleAction::Script { script } = &rule.action {
            if script.trim().is_empty() {
                return Err(RuleError::InvalidRule("script cannot be empty".into()));
            }
            script_engine()
                .compile(script)
                .map_err(|err| RuleError::InvalidRule(format!("script compile error: {err}")))?;
        }
        Ok(())
    }

    pub async fn apply_request(&self, ctx: &mut RequestContext) -> RuleResult {
        let rules = self.rules.read().await.clone();
        let mut result = RuleResult::default();
        for rule in rules.into_iter().filter(|r| r.enabled) {
            if !matches_request(&rule, &ctx.session) {
                continue;
            }
            result.matched_rule_ids.push(rule.id.clone());
            ctx.matched_rule_ids.push(rule.id.clone());
            match rule.action {
                RuleAction::RewriteRequestHeaders { headers } => {
                    result.rewrite_request_headers.extend(headers);
                }
                RuleAction::RewriteResponseHeaders { .. } => {}
                RuleAction::MockResponse {
                    status,
                    headers,
                    body,
                } => {
                    result.mock_response = Some((status, headers, body));
                }
                RuleAction::Delay { millis } => {
                    result.delay_ms = Some(millis);
                }
                RuleAction::Script { script } => {
                    if let Ok(script_result) = run_script(
                        &script,
                        &ctx.session,
                        &ctx.request_headers,
                        &ctx.request_trailers,
                        true,
                    ) {
                        if let Some(headers) = script_result.request_headers {
                            result.rewrite_request_headers.extend(headers);
                        }
                        if let Some(headers) = script_result.response_headers {
                            result.rewrite_response_headers.extend(headers);
                        }
                        if let Some(trailers) = script_result.request_trailers {
                            result.rewrite_request_trailers.extend(trailers);
                        }
                        if let Some(trailers) = script_result.response_trailers {
                            result.rewrite_response_trailers.extend(trailers);
                        }
                        if let Some(mock) = script_result.mock_response {
                            result.mock_response = Some(mock);
                        }
                        if let Some(delay_ms) = script_result.delay_ms {
                            result.delay_ms = Some(delay_ms);
                        }
                    }
                }
            }
        }
        result
    }

    pub async fn apply_response(&self, ctx: &mut ResponseContext) -> RuleResult {
        let rules = self.rules.read().await.clone();
        let mut result = RuleResult::default();
        for rule in rules.into_iter().filter(|r| r.enabled) {
            if !matches_request(&rule, &ctx.session) {
                continue;
            }
            result.matched_rule_ids.push(rule.id.clone());
            ctx.matched_rule_ids.push(rule.id.clone());
            match rule.action {
                RuleAction::RewriteResponseHeaders { headers } => {
                    result.rewrite_response_headers.extend(headers);
                }
                RuleAction::RewriteRequestHeaders { .. } => {}
                RuleAction::MockResponse { .. } => {}
                RuleAction::Delay { millis } => {
                    result.delay_ms = Some(millis);
                }
                RuleAction::Script { script } => {
                    if let Ok(script_result) = run_script(
                        &script,
                        &ctx.session,
                        &ctx.response_headers,
                        &ctx.response_trailers,
                        false,
                    ) {
                        if let Some(headers) = script_result.request_headers {
                            result.rewrite_request_headers.extend(headers);
                        }
                        if let Some(headers) = script_result.response_headers {
                            result.rewrite_response_headers.extend(headers);
                        }
                        if let Some(trailers) = script_result.request_trailers {
                            result.rewrite_request_trailers.extend(trailers);
                        }
                        if let Some(trailers) = script_result.response_trailers {
                            result.rewrite_response_trailers.extend(trailers);
                        }
                        if let Some(mock) = script_result.mock_response {
                            result.mock_response = Some(mock);
                        }
                        if let Some(delay_ms) = script_result.delay_ms {
                            result.delay_ms = Some(delay_ms);
                        }
                    }
                }
            }
        }
        result
    }

    pub async fn maybe_delay(delay_ms: Option<u64>) {
        if let Some(delay_ms) = delay_ms {
            if delay_ms > 0 {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            }
        }
    }
}

#[derive(Default)]
struct ScriptOutcome {
    request_headers: Option<Vec<HeaderPair>>,
    response_headers: Option<Vec<HeaderPair>>,
    request_trailers: Option<Vec<HeaderPair>>,
    response_trailers: Option<Vec<HeaderPair>>,
    mock_response: Option<(u16, Vec<HeaderPair>, String)>,
    delay_ms: Option<u64>,
}

fn script_engine() -> Engine {
    let mut engine = Engine::new();
    engine.set_max_operations(20_000);
    engine.set_max_call_levels(32);
    engine.register_fn("header_map", |headers: Array| {
        let mut map = Map::new();
        for entry in headers {
            if let Ok(pair) = from_dynamic::<HeaderPair>(&entry) {
                map.insert(pair.name.into(), pair.value.into());
            }
        }
        map
    });
    engine.register_fn("header_list", |map: Map| {
        map.into_iter()
            .map(|(name, value)| HeaderPair {
                name: name.into(),
                value: value.try_cast::<String>().unwrap_or_default(),
            })
            .collect::<Vec<_>>()
    });
    engine
}

fn run_script(
    script: &str,
    session: &CapturedSession,
    headers: &[HeaderPair],
    trailers: &[HeaderPair],
    is_request: bool,
) -> Result<ScriptOutcome, RuleError> {
    let engine = script_engine();
    let mut scope = Scope::new();
    scope.push_dynamic("session", to_dynamic(session).map_err(script_err)?);
    scope.push_dynamic("headers", to_dynamic(headers.to_vec()).map_err(script_err)?);
    scope.push_dynamic(
        "trailers",
        to_dynamic(trailers.to_vec()).map_err(script_err)?,
    );
    scope.push("is_request", is_request);
    let result = engine
        .eval_with_scope::<Dynamic>(&mut scope, script)
        .map_err(script_err)?;
    let mut outcome = ScriptOutcome::default();
    if result.is_unit() {
        return Ok(outcome);
    }
    if let Some(map) = result.try_cast::<Map>() {
        if let Some(value) = map.get("request_headers") {
            outcome.request_headers = Some(from_dynamic(value).map_err(script_err)?);
        }
        if let Some(value) = map.get("response_headers") {
            outcome.response_headers = Some(from_dynamic(value).map_err(script_err)?);
        }
        if let Some(value) = map.get("request_trailers") {
            outcome.request_trailers = Some(from_dynamic(value).map_err(script_err)?);
        }
        if let Some(value) = map.get("response_trailers") {
            outcome.response_trailers = Some(from_dynamic(value).map_err(script_err)?);
        }
        if let Some(value) = map.get("delay_ms") {
            outcome.delay_ms = value.clone().try_cast::<u64>();
        }
        if let Some(value) = map.get("mock_response") {
            if let Some(mock_map) = value.clone().try_cast::<Map>() {
                let status = mock_map
                    .get("status")
                    .and_then(|v| v.clone().try_cast::<u64>())
                    .unwrap_or(200) as u16;
                let body = mock_map
                    .get("body")
                    .and_then(|v| v.clone().try_cast::<String>())
                    .unwrap_or_default();
                let headers = mock_map
                    .get("headers")
                    .and_then(|v| v.clone().try_cast::<Array>())
                    .map(|values| {
                        values
                            .into_iter()
                            .filter_map(|value| from_dynamic::<HeaderPair>(&value).ok())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                outcome.mock_response = Some((status, headers, body));
            }
        }
    }
    Ok(outcome)
}

fn script_err(err: impl std::fmt::Display) -> RuleError {
    RuleError::InvalidRule(format!("script error: {err}"))
}

fn matches_request(rule: &Rule, session: &CapturedSession) -> bool {
    let matcher = &rule.match_;
    if let Some(method) = &matcher.method {
        if session
            .method
            .as_deref()
            .map(|m| !m.eq_ignore_ascii_case(method))
            .unwrap_or(true)
        {
            return false;
        }
    }
    if let Some(host) = &matcher.host {
        if session
            .host
            .as_deref()
            .map(|h| !h.contains(host))
            .unwrap_or(true)
        {
            return false;
        }
    }
    if let Some(url_contains) = &matcher.url_contains {
        if session
            .url
            .as_deref()
            .map(|u| !u.contains(url_contains))
            .unwrap_or(true)
        {
            return false;
        }
    }
    if let Some(status) = matcher.status {
        if session.status != Some(status) {
            return false;
        }
    }
    if let Some(host_contains) = &matcher.host {
        if session
            .host
            .as_deref()
            .map(|h| !h.contains(host_contains))
            .unwrap_or(true)
        {
            return false;
        }
    }
    true
}

pub fn merge_headers(base: &mut Vec<HeaderPair>, rewrites: &[HeaderPair]) {
    for rewrite in rewrites {
        if let Some(existing) = base
            .iter_mut()
            .find(|header| header.name.eq_ignore_ascii_case(&rewrite.name))
        {
            existing.value = rewrite.value.clone();
        } else {
            base.push(rewrite.clone());
        }
    }
}
