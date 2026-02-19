use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyEntry {
    pub rule: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Quotas {
    pub tokens: Option<u32>,
    pub time_ms: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    LocalOnly,
    Preferred,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub route: Route,
    pub budgets: Quotas,
    pub trace: Vec<PolicyEntry>,
    pub blocked: bool,
    pub require_dual_control: bool,
}

pub trait Policy {
    fn evaluate(&self, work: &Value) -> Decision;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultPolicy;

impl Policy for DefaultPolicy {
    fn evaluate(&self, work: &Value) -> Decision {
        let topic = work
            .get("topic")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let payload = work.get("payload").unwrap_or(work);

        let mut trace = Vec::new();
        let mut budgets = Quotas::default();
        let mut route = Route::Preferred;
        let mut blocked = false;
        let mut require_dual_control = false;

        if topic.starts_with("chat:") {
            budgets.tokens = Some(4000);
            trace.push(PolicyEntry {
                rule: "quotas_chat".to_string(),
                ok: true,
                detail: Some("tokens<=4000".to_string()),
            });
        }

        if pii_detected(payload) {
            route = Route::LocalOnly;
            trace.push(PolicyEntry {
                rule: "pii_local".to_string(),
                ok: true,
                detail: Some("local-only + no-network".to_string()),
            });
        }

        if topic.ends_with(":commit") {
            require_dual_control = true;
            trace.push(PolicyEntry {
                rule: "commitment".to_string(),
                ok: true,
                detail: Some("dual control required".to_string()),
            });
        }

        if let Some(tokens) = budgets.tokens
            && topic.starts_with("chat:")
            && token_estimate(payload) > tokens
        {
            blocked = true;
            trace.push(PolicyEntry {
                rule: "quotas_chat_enforced".to_string(),
                ok: false,
                detail: Some("payload estimated over token budget".to_string()),
            });
        }

        Decision {
            route,
            budgets,
            trace,
            blocked,
            require_dual_control,
        }
    }
}

fn token_estimate(value: &Value) -> u32 {
    value.to_string().chars().count().div_ceil(4) as u32
}

fn pii_detected(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(k, v)| {
            let key = k.to_ascii_lowercase();
            key.contains("email")
                || key.contains("phone")
                || key.contains("cpf")
                || key.contains("ssn")
                || pii_detected(v)
        }),
        Value::Array(items) => items.iter().any(pii_detected),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn chat_quota_is_applied() {
        let policy = DefaultPolicy;
        let decision = policy.evaluate(&json!({"topic":"chat:answer","payload":{"msg": "hello"}}));
        assert_eq!(decision.budgets.tokens, Some(4000));
        assert!(!decision.blocked);
    }

    #[test]
    fn pii_forces_local_only() {
        let policy = DefaultPolicy;
        let decision = policy.evaluate(&json!({"topic":"science:run","payload":{"email":"x@y.z"}}));
        assert!(matches!(decision.route, Route::LocalOnly));
    }

    #[test]
    fn commit_requires_dual_control() {
        let policy = DefaultPolicy;
        let decision = policy.evaluate(&json!({"topic":"oc:commit","payload":{}}));
        assert!(decision.require_dual_control);
    }

    #[test]
    fn chat_budget_exceeded_is_blocked() {
        let policy = DefaultPolicy;
        // Construct a payload that token_estimate will count as > 4000 tokens
        let large_text = "word ".repeat(4500);
        let decision = policy.evaluate(&json!({
            "topic": "chat:answer",
            "payload": {"prompt": large_text}
        }));
        assert!(
            decision.blocked,
            "expected chat budget to be exceeded and blocked"
        );
        let blocked_rule = decision
            .trace
            .iter()
            .any(|e| e.rule == "quotas_chat_enforced" && !e.ok);
        assert!(blocked_rule, "expected quotas_chat_enforced rule in trace");
    }

    #[test]
    fn phone_field_forces_local_only() {
        let policy = DefaultPolicy;
        let decision = policy.evaluate(&json!({
            "topic": "science:run",
            "payload": {"phone": "+55-11-99999-0000", "data": "other"}
        }));
        assert!(matches!(decision.route, Route::LocalOnly));
        let pii_rule = decision.trace.iter().any(|e| e.rule == "pii_local");
        assert!(pii_rule, "expected pii_local rule in policy trace");
    }

    #[test]
    fn ssn_nested_in_array_forces_local_only() {
        let policy = DefaultPolicy;
        let decision = policy.evaluate(&json!({
            "topic": "science:run",
            "payload": {
                "records": [{"ssn": "123-45-6789"}, {"name": "Alice"}]
            }
        }));
        assert!(matches!(decision.route, Route::LocalOnly));
    }

    #[test]
    fn non_pii_non_commit_has_preferred_route() {
        let policy = DefaultPolicy;
        let decision = policy.evaluate(&json!({
            "topic": "vcx:transcode",
            "payload": {"codec": "av1", "width": 1920}
        }));
        assert!(matches!(decision.route, Route::Preferred));
        assert!(!decision.blocked);
        assert!(!decision.require_dual_control);
    }

    #[test]
    fn policy_trace_is_populated_for_chat_topic() {
        let policy = DefaultPolicy;
        let decision = policy.evaluate(&json!({
            "topic": "chat:ask",
            "payload": {"q": "hello"}
        }));
        let has_quota_rule = decision.trace.iter().any(|e| e.rule == "quotas_chat");
        assert!(
            has_quota_rule,
            "chat topic should have quotas_chat rule in trace"
        );
        assert_eq!(decision.budgets.tokens, Some(4000));
    }
}
