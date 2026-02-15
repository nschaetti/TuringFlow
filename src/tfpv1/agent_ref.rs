use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentRef {
    local: String,
    hostname: String,
}

impl AgentRef {
    pub fn parse(input: &str) -> Result<Self, AgentRefError> {
        let (local, hostname) = input
            .split_once('@')
            .ok_or(AgentRefError::MissingSeparator)?;

        if local.is_empty() {
            return Err(AgentRefError::InvalidLocalPart);
        }
        if hostname.is_empty() {
            return Err(AgentRefError::InvalidHostname);
        }

        if !is_valid_local_part(local) {
            return Err(AgentRefError::InvalidLocalPart);
        }

        let normalized_hostname = normalize_hostname(hostname);
        if !is_valid_hostname(&normalized_hostname) {
            return Err(AgentRefError::InvalidHostname);
        }

        Ok(Self {
            local: local.to_string(),
            hostname: normalized_hostname,
        })
    }

    pub fn local(&self) -> &str {
        &self.local
    }

    pub fn hostname(&self) -> &str {
        &self.hostname
    }

    pub fn normalized(&self) -> String {
        format!("{}@{}", self.local, self.hostname)
    }

    pub fn validate_hostname(hostname: &str) -> bool {
        is_valid_hostname(&normalize_hostname(hostname))
    }
}

impl Display for AgentRef {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}@{}", self.local, self.hostname)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentRefError {
    MissingSeparator,
    InvalidLocalPart,
    InvalidHostname,
}

impl Display for AgentRefError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentRefError::MissingSeparator => {
                write!(
                    f,
                    "agent reference must be formatted as name-or-id@hostname"
                )
            }
            AgentRefError::InvalidLocalPart => {
                write!(f, "invalid local part in agent reference")
            }
            AgentRefError::InvalidHostname => {
                write!(f, "invalid hostname in agent reference")
            }
        }
    }
}

impl Error for AgentRefError {}

fn normalize_hostname(hostname: &str) -> String {
    hostname.to_ascii_lowercase()
}

fn is_valid_local_part(local: &str) -> bool {
    is_valid_name(local) || is_valid_id(local)
}

fn is_valid_name(name: &str) -> bool {
    if !(3..=64).contains(&name.len()) {
        return false;
    }

    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    if !first.is_ascii_lowercase() {
        return false;
    }

    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

fn is_valid_id(id: &str) -> bool {
    if !(3..=128).contains(&id.len()) {
        return false;
    }

    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    if !first.is_ascii_alphanumeric() {
        return false;
    }

    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn is_valid_hostname(hostname: &str) -> bool {
    if hostname.is_empty() || hostname.len() > 253 {
        return false;
    }
    if hostname.starts_with('.') || hostname.ends_with('.') {
        return false;
    }

    let labels = hostname.split('.');
    for label in labels {
        if label.is_empty() || label.len() > 63 {
            return false;
        }

        let mut chars = label.chars();
        let Some(first) = chars.next() else {
            return false;
        };
        if !first.is_ascii_alphanumeric() {
            return false;
        }

        let mut previous = first;
        for ch in chars {
            if !(ch.is_ascii_alphanumeric() || ch == '-') {
                return false;
            }
            previous = ch;
        }

        if previous == '-' {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::AgentRef;

    #[test]
    fn parses_name_ref_and_normalizes_hostname() {
        let parsed = AgentRef::parse("planner@Node-A.Local").expect("must parse");
        assert_eq!(parsed.local(), "planner");
        assert_eq!(parsed.hostname(), "node-a.local");
        assert_eq!(parsed.normalized(), "planner@node-a.local");
    }

    #[test]
    fn parses_id_ref() {
        let parsed = AgentRef::parse("ag_01JCM9ABCD@gpu-03.internal").expect("must parse");
        assert_eq!(parsed.local(), "ag_01JCM9ABCD");
    }

    #[test]
    fn rejects_invalid_refs() {
        assert!(AgentRef::parse("planner").is_err());
        assert!(AgentRef::parse("plan!er@node-a.local").is_err());
        assert!(AgentRef::parse("planner@-node-a.local").is_err());
        assert!(AgentRef::parse("planner@node_a.local").is_err());
    }
}
