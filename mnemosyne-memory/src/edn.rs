use crate::episode::{Episode, EpisodeKind};

/// Serialize a value as an EDN string.
pub trait ToEdn {
    fn to_edn(&self) -> String;
}

fn edn_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Convert a JSON string key to an EDN keyword.
/// Non-identifier characters are kept but the leading `:` is always added.
fn edn_key(s: &str) -> String {
    format!(":{s}")
}

fn json_to_edn(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => "nil".into(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => edn_str(s),
        serde_json::Value::Array(a) => {
            let items: Vec<String> = a.iter().map(json_to_edn).collect();
            format!("[{}]", items.join(" "))
        }
        serde_json::Value::Object(m) => {
            let pairs: Vec<String> = m
                .iter()
                .map(|(k, v)| format!("{} {}", edn_key(k), json_to_edn(v)))
                .collect();
            format!("{{{}}}", pairs.join(", "))
        }
    }
}

impl ToEdn for Episode {
    fn to_edn(&self) -> String {
        let mut pairs = vec![
            format!(":seq {}", self.seq),
            format!(":session {}", edn_str(&self.session_id.to_string())),
            format!(":ts {}", edn_str(&self.timestamp.to_rfc3339())),
        ];

        match &self.kind {
            EpisodeKind::UserMessage { content } => {
                pairs.push(":kind :user-message".into());
                pairs.push(format!(":content {}", edn_str(content)));
            }
            EpisodeKind::AssistantReply { content } => {
                pairs.push(":kind :assistant-reply".into());
                pairs.push(format!(":content {}", edn_str(content)));
            }
            EpisodeKind::ToolCall { tool, input } => {
                pairs.push(":kind :tool-call".into());
                pairs.push(format!(":tool {}", edn_str(tool)));
                pairs.push(format!(":input {}", json_to_edn(input)));
            }
            EpisodeKind::ToolResult {
                tool,
                success,
                output,
            } => {
                pairs.push(":kind :tool-result".into());
                pairs.push(format!(":tool {}", edn_str(tool)));
                pairs.push(format!(":success {success}"));
                pairs.push(format!(":output {}", edn_str(output)));
            }
            EpisodeKind::WorkingMemory { bindings } => {
                pairs.push(":kind :working-memory".into());
                let bp: Vec<String> = bindings
                    .iter()
                    .map(|(k, v)| format!("{} {}", edn_str(k), edn_str(v)))
                    .collect();
                pairs.push(format!(":bindings {{{}}}", bp.join(", ")));
            }
            EpisodeKind::Note { content } => {
                pairs.push(":kind :note".into());
                pairs.push(format!(":content {}", edn_str(content)));
            }
        }

        format!("{{{}}}", pairs.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::episode::{EpisodeKind, SessionId};
    use chrono::Utc;

    fn make_episode(seq: u64, kind: EpisodeKind) -> Episode {
        Episode {
            seq,
            session_id: SessionId::new(),
            timestamp: Utc::now(),
            kind,
        }
    }

    #[test]
    fn edn_user_message() {
        let ep = make_episode(
            0,
            EpisodeKind::UserMessage {
                content: "hello".into(),
            },
        );
        let s = ep.to_edn();
        assert!(s.starts_with('{'));
        assert!(s.ends_with('}'));
        assert!(s.contains(":kind :user-message"));
        assert!(s.contains(":seq 0"));
        assert!(s.contains(r#":content "hello""#));
    }

    #[test]
    fn edn_tool_call() {
        let ep = make_episode(
            1,
            EpisodeKind::ToolCall {
                tool: "search_code".into(),
                input: serde_json::json!({"query": "retry", "limit": 5}),
            },
        );
        let s = ep.to_edn();
        assert!(s.contains(":kind :tool-call"));
        assert!(s.contains(r#":tool "search_code""#));
        assert!(s.contains(":query"));
    }

    #[test]
    fn edn_string_escaping() {
        let ep = make_episode(
            2,
            EpisodeKind::Note {
                content: "line1\nline2\ttab\"quote\"".into(),
            },
        );
        let s = ep.to_edn();
        assert!(s.contains("\\n"));
        assert!(s.contains("\\t"));
        assert!(s.contains("\\\""));
    }
}
