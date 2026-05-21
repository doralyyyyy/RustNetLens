use std::sync::Arc;
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
