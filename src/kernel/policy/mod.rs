//! Policy configuration and evaluation engine.
//!
//! Evaluation is deny-by-default and resolves principals in this order:
//! `agent_tool:<agent_ref>:<tool_id>` then `agent:<agent_ref>`.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde_json::Map;
use serde_json::Value;

use crate::kernel::context::ExecutionContext;

/// Root policy configuration loaded from YAML.
#[derive(Debug, Clone, Deserialize)]
pub struct PolicyConfig {
    /// Schema version (must be `1`).
    pub version: u32,
    /// Default decision policy.
    pub defaults: PolicyDefaults,
    /// Principal-specific rules.
    #[serde(default)]
    pub principals: Vec<PrincipalPolicy>,
}

impl PolicyConfig {
    /// Loads and validates a policy file.
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, Box<dyn Error>> {
        let raw = fs::read_to_string(path)?;
        let config: Self = serde_yaml::from_str(&raw)?;
        config.validate()?;
        Ok(config)
    }

    /// Validates schema and rule invariants.
    pub fn validate(&self) -> Result<(), Box<dyn Error>> {
        if self.version != 1 {
            return Err("policy version must be 1".into());
        }
        if self.defaults.decision != Decision::Deny {
            return Err("defaults.decision must be deny".into());
        }

        let mut principal_ids = HashSet::new();
        for principal in &self.principals {
            if principal.id.trim().is_empty() {
                return Err("principal id must not be empty".into());
            }
            if !principal_ids.insert(principal.id.clone()) {
                return Err(
                    format!("duplicate principal id '{}': not allowed", principal.id).into(),
                );
            }

            let mut rule_ids = HashSet::new();
            for rule in &principal.rules {
                if rule.id.trim().is_empty() {
                    return Err(format!("principal '{}' has empty rule id", principal.id).into());
                }
                if !rule_ids.insert(rule.id.clone()) {
                    return Err(format!(
                        "principal '{}' has duplicate rule id '{}'",
                        principal.id, rule.id
                    )
                    .into());
                }
                if rule.syscall.trim().is_empty() {
                    return Err(format!(
                        "principal '{}' rule '{}' has empty syscall",
                        principal.id, rule.id
                    )
                    .into());
                }
            }
        }

        Ok(())
    }
}

/// Default policy behavior.
#[derive(Debug, Clone, Deserialize)]
pub struct PolicyDefaults {
    /// Default decision for unmatched requests.
    pub decision: Decision,
}

/// Allow/deny effect.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Allow,
    Deny,
}

/// Rules attached to a principal.
#[derive(Debug, Clone, Deserialize)]
pub struct PrincipalPolicy {
    /// Principal id (`agent:*` or `agent_tool:*`).
    pub id: String,
    /// Ordered rules (sorted by priority during engine construction).
    #[serde(default)]
    pub rules: Vec<PolicyRule>,
}

/// Individual policy rule.
#[derive(Debug, Clone, Deserialize)]
pub struct PolicyRule {
    /// Rule identifier, unique within principal.
    pub id: String,
    /// Rule effect.
    pub effect: Decision,
    /// Syscall name matched by this rule.
    pub syscall: String,
    /// Optional resource matcher.
    pub resource: Option<Value>,
    /// Optional future constraints payload.
    pub constraints: Option<Value>,
    /// Rule priority (higher first).
    pub priority: Option<i64>,
}

/// Result of one policy evaluation.
#[derive(Debug, Clone)]
pub struct DecisionResult {
    /// Whether access is granted.
    pub allowed: bool,
    /// Matching rule id, when any.
    pub rule_id: Option<String>,
    /// Principal that matched, when any.
    pub principal_id: Option<String>,
}

/// In-memory policy evaluator.
#[derive(Debug, Clone)]
pub struct PolicyEngine {
    by_principal: HashMap<String, Vec<PolicyRule>>,
}

impl PolicyEngine {
    /// Builds an evaluator and pre-sorts rules by descending priority.
    pub fn new(config: PolicyConfig) -> Self {
        let mut by_principal = HashMap::new();

        for principal in config.principals {
            let mut rules = principal.rules;
            rules.sort_by_key(|rule| std::cmp::Reverse(rule.priority.unwrap_or(0)));
            by_principal.insert(principal.id, rules);
        }

        Self { by_principal }
    }

    /// Evaluates a syscall without resource attributes.
    pub fn evaluate(&self, ctx: &ExecutionContext, syscall: &str) -> DecisionResult {
        self.evaluate_with_resource(ctx, syscall, None)
    }

    /// Evaluates a syscall with optional resource attributes.
    pub fn evaluate_with_resource(
        &self,
        ctx: &ExecutionContext,
        syscall: &str,
        resource: Option<&Value>,
    ) -> DecisionResult {
        for principal_id in ctx.principal_candidates() {
            if let Some(rules) = self.by_principal.get(&principal_id) {
                for rule in rules {
                    if rule.syscall == syscall && resource_matches(rule.resource.as_ref(), resource)
                    {
                        return DecisionResult {
                            allowed: rule.effect == Decision::Allow,
                            rule_id: Some(rule.id.clone()),
                            principal_id: Some(principal_id),
                        };
                    }
                }
            }
        }

        DecisionResult {
            allowed: false,
            rule_id: None,
            principal_id: None,
        }
    }
}

fn resource_matches(rule_resource: Option<&Value>, request_resource: Option<&Value>) -> bool {
    match (rule_resource, request_resource) {
        (None, _) => true,
        (Some(_), None) => false,
        (Some(rule), Some(request)) => value_matches(rule, request),
    }
}

fn value_matches(rule: &Value, request: &Value) -> bool {
    match (rule, request) {
        (Value::Object(rule_obj), Value::Object(request_obj)) => {
            object_matches(rule_obj, request_obj)
        }
        (Value::Array(rule_arr), Value::Array(request_arr)) => rule_arr.iter().all(|required| {
            request_arr
                .iter()
                .any(|candidate| value_matches(required, candidate))
        }),
        (Value::Array(rule_arr), _) => rule_arr
            .iter()
            .any(|candidate| value_matches(candidate, request)),
        _ => rule == request,
    }
}

fn object_matches(rule: &Map<String, Value>, request: &Map<String, Value>) -> bool {
    for (key, required) in rule {
        let matched = match key.as_str() {
            "path_prefix" => request
                .get("path")
                .and_then(Value::as_str)
                .map(|path| any_string_prefix_match(required, path))
                .unwrap_or(false),
            "host_allowlist" => request
                .get("host")
                .and_then(Value::as_str)
                .map(|host| any_string_exact_match(required, host))
                .unwrap_or(false),
            "command_allowlist" => request
                .get("command")
                .and_then(Value::as_str)
                .map(|command| any_string_exact_match(required, command))
                .unwrap_or(false),
            "methods" => request
                .get("method")
                .and_then(Value::as_str)
                .map(|method| any_string_equal_case_insensitive(required, method))
                .unwrap_or(false),
            _ => request
                .get(key)
                .map(|actual| value_matches(required, actual))
                .unwrap_or(false),
        };

        if !matched {
            return false;
        }
    }
    true
}

fn any_string_prefix_match(rule_value: &Value, candidate: &str) -> bool {
    collect_strings(rule_value)
        .iter()
        .any(|prefix| candidate.starts_with(prefix))
}

fn any_string_exact_match(rule_value: &Value, candidate: &str) -> bool {
    collect_strings(rule_value)
        .iter()
        .any(|allowed| allowed == candidate)
}

fn any_string_equal_case_insensitive(rule_value: &Value, candidate: &str) -> bool {
    collect_strings(rule_value)
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(candidate))
}

fn collect_strings(value: &Value) -> Vec<String> {
    match value {
        Value::String(single) => vec![single.clone()],
        Value::Array(values) => values
            .iter()
            .filter_map(Value::as_str)
            .map(ToString::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{PolicyConfig, PolicyEngine};
    use crate::kernel::context::ExecutionContext;
    use serde_json::json;

    #[test]
    fn denies_by_default_when_no_rule_matches() {
        let yaml = r#"
version: 1
defaults:
  decision: deny
principals: []
"#;

        let config: PolicyConfig = serde_yaml::from_str(yaml).expect("valid yaml");
        config.validate().expect("config valid");
        let engine = PolicyEngine::new(config);
        let ctx = ExecutionContext {
            trace_id: "trc_1".to_string(),
            kingdom_id: "kingdom-main".to_string(),
            agent_ref: "planner@node-a.local".to_string(),
            tool_id: None,
        };

        let decision = engine.evaluate(&ctx, "fs.read");
        assert!(!decision.allowed);
        assert!(decision.rule_id.is_none());
    }

    #[test]
    fn prefers_agent_tool_over_agent() {
        let yaml = r#"
version: 1
defaults:
  decision: deny
principals:
  - id: "agent:planner@node-a.local"
    rules:
      - id: "agent-deny-fs-read"
        effect: deny
        syscall: "fs.read"
  - id: "agent_tool:planner@node-a.local:reader"
    rules:
      - id: "tool-allow-fs-read"
        effect: allow
        syscall: "fs.read"
"#;

        let config: PolicyConfig = serde_yaml::from_str(yaml).expect("valid yaml");
        config.validate().expect("config valid");
        let engine = PolicyEngine::new(config);

        let ctx = ExecutionContext {
            trace_id: "trc_2".to_string(),
            kingdom_id: "kingdom-main".to_string(),
            agent_ref: "planner@node-a.local".to_string(),
            tool_id: Some("reader".to_string()),
        };

        let decision = engine.evaluate(&ctx, "fs.read");
        assert!(decision.allowed);
        assert_eq!(decision.rule_id.as_deref(), Some("tool-allow-fs-read"));
    }

    #[test]
    fn matches_resource_path_prefix() {
        let yaml = r#"
version: 1
defaults:
  decision: deny
principals:
  - id: "agent:planner@node-a.local"
    rules:
      - id: "allow-fs-read-workspace"
        effect: allow
        syscall: "fs.read"
        resource:
          path_prefix:
            - "/workspace/project"
"#;

        let config: PolicyConfig = serde_yaml::from_str(yaml).expect("valid yaml");
        config.validate().expect("config valid");
        let engine = PolicyEngine::new(config);
        let ctx = ExecutionContext {
            trace_id: "trc_3".to_string(),
            kingdom_id: "kingdom-main".to_string(),
            agent_ref: "planner@node-a.local".to_string(),
            tool_id: None,
        };

        let allowed = engine.evaluate_with_resource(
            &ctx,
            "fs.read",
            Some(&json!({"path": "/workspace/project/src/main.rs"})),
        );
        assert!(allowed.allowed);

        let denied =
            engine.evaluate_with_resource(&ctx, "fs.read", Some(&json!({"path": "/etc/passwd"})));
        assert!(!denied.allowed);
    }

    #[test]
    fn applies_priority_order_within_principal() {
        let yaml = r#"
version: 1
defaults:
  decision: deny
principals:
  - id: "agent:planner@node-a.local"
    rules:
      - id: "deny-default"
        effect: deny
        syscall: "proc.exec"
        priority: 1
      - id: "allow-rg"
        effect: allow
        syscall: "proc.exec"
        priority: 10
        resource:
          command_allowlist:
            - "rg"
"#;

        let config: PolicyConfig = serde_yaml::from_str(yaml).expect("valid yaml");
        config.validate().expect("config valid");
        let engine = PolicyEngine::new(config);
        let ctx = ExecutionContext {
            trace_id: "trc_4".to_string(),
            kingdom_id: "kingdom-main".to_string(),
            agent_ref: "planner@node-a.local".to_string(),
            tool_id: None,
        };

        let allow_rg =
            engine.evaluate_with_resource(&ctx, "proc.exec", Some(&json!({"command": "rg"})));
        assert!(allow_rg.allowed);
        assert_eq!(allow_rg.rule_id.as_deref(), Some("allow-rg"));

        let deny_cargo =
            engine.evaluate_with_resource(&ctx, "proc.exec", Some(&json!({"command": "cargo"})));
        assert!(!deny_cargo.allowed);
        assert_eq!(deny_cargo.rule_id.as_deref(), Some("deny-default"));
    }

    #[test]
    fn rejects_non_deny_default() {
        let yaml = r#"
version: 1
defaults:
  decision: allow
principals: []
"#;

        let config: PolicyConfig = serde_yaml::from_str(yaml).expect("yaml parsed");
        let err = config.validate().expect_err("must reject allow default");
        assert!(err.to_string().contains("defaults.decision must be deny"));
    }
}
