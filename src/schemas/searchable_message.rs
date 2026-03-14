use crate::schemas::session_message::SessionMessage as ClaudeSessionMessage;
use serde::Deserialize;

const USER_MESSAGE_BEGIN: &str = "## My request for Codex:";
const HIDDEN_CONTEXT_TAGS: [&str; 7] = [
    "<user_instructions>",
    "<environment_context>",
    "<apps_instructions>",
    "<skills_instructions>",
    "<plugins_instructions>",
    "<collaboration_mode>",
    "<realtime_conversation>",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionContext {
    pub session_id: Option<String>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchableMessage {
    role: String,
    text: String,
    uuid: Option<String>,
    timestamp: Option<String>,
    session_id: Option<String>,
    cwd: Option<String>,
}

impl SearchableMessage {
    fn from_claude(message: ClaudeSessionMessage) -> Self {
        Self {
            role: message.get_type().to_string(),
            text: message.get_content_text(),
            uuid: message.get_uuid().map(str::to_string),
            timestamp: message.get_timestamp().map(str::to_string),
            session_id: message.get_session_id().map(str::to_string),
            cwd: message.get_cwd().map(str::to_string),
        }
    }

    fn from_codex_response(
        role: &str,
        text: String,
        timestamp: String,
        context: &SessionContext,
    ) -> Option<Self> {
        let normalized_text = normalize_codex_message_text(role, &text)?;

        if normalized_text.is_empty() {
            return None;
        }

        Some(Self {
            role: role.to_string(),
            text: normalized_text,
            uuid: None,
            timestamp: Some(timestamp),
            session_id: context.session_id.clone(),
            cwd: context.cwd.clone(),
        })
    }

    pub fn get_type(&self) -> &str {
        &self.role
    }

    pub fn get_content_text(&self) -> String {
        self.text.clone()
    }

    pub fn get_uuid(&self) -> Option<&str> {
        self.uuid.as_deref()
    }

    pub fn get_timestamp(&self) -> Option<&str> {
        self.timestamp.as_deref()
    }

    pub fn get_session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn get_cwd(&self) -> Option<&str> {
        self.cwd.as_deref()
    }

    pub fn get_searchable_text(&self) -> String {
        let mut parts = vec![self.text.clone()];

        if let Some(session_id) = &self.session_id {
            parts.push(session_id.clone());
        }

        if let Some(uuid) = &self.uuid {
            parts.push(uuid.clone());
        }

        parts.join(" ")
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum CodexRolloutLine {
    SessionMeta {
        payload: CodexSessionMetaPayload,
    },
    TurnContext {
        payload: CodexTurnContextPayload,
    },
    ResponseItem {
        timestamp: String,
        payload: CodexResponseItem,
    },
    EventMsg {
        #[serde(rename = "payload")]
        _payload: serde_json::Value,
    },
    Compacted {
        #[serde(rename = "payload")]
        _payload: serde_json::Value,
    },
}

#[derive(Debug, Deserialize)]
struct CodexSessionMetaPayload {
    id: String,
    cwd: String,
}

#[derive(Debug, Deserialize)]
struct CodexTurnContextPayload {
    #[serde(default)]
    cwd: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum CodexResponseItem {
    Message {
        role: String,
        #[serde(default)]
        content: Vec<CodexContentItem>,
    },
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum CodexContentItem {
    InputText {
        text: String,
    },
    OutputText {
        text: String,
    },
    InputImage {
        image_url: String,
    },
    #[serde(other)]
    Other,
}

pub fn parse_searchable_message(
    line: &[u8],
    context: &mut SessionContext,
) -> Option<SearchableMessage> {
    if let Ok(message) = sonic_rs::from_slice::<ClaudeSessionMessage>(line) {
        return Some(SearchableMessage::from_claude(message));
    }

    let rollout_line = sonic_rs::from_slice::<CodexRolloutLine>(line).ok()?;

    match rollout_line {
        CodexRolloutLine::SessionMeta { payload } => {
            context.session_id = Some(payload.id);
            context.cwd = Some(payload.cwd);
            None
        }
        CodexRolloutLine::TurnContext { payload } => {
            if context.cwd.is_none() {
                context.cwd = payload.cwd;
            }
            None
        }
        CodexRolloutLine::ResponseItem { timestamp, payload } => match payload {
            CodexResponseItem::Message { role, content } => {
                let mut parts = Vec::new();

                for item in content {
                    match item {
                        CodexContentItem::InputText { text }
                        | CodexContentItem::OutputText { text } => {
                            if let Some(text) = normalize_codex_message_text(&role, &text) {
                                parts.push(text);
                            }
                        }
                        CodexContentItem::InputImage { image_url } => {
                            if !image_url.trim().is_empty() {
                                parts.push("[Image]".to_string());
                            }
                        }
                        CodexContentItem::Other => {}
                    }
                }

                SearchableMessage::from_codex_response(&role, parts.join("\n"), timestamp, context)
            }
            CodexResponseItem::Other => None,
        },
        CodexRolloutLine::EventMsg { .. } | CodexRolloutLine::Compacted { .. } => None,
    }
}

fn strip_user_message_prefix(text: &str) -> &str {
    match text.find(USER_MESSAGE_BEGIN) {
        Some(idx) => text[idx + USER_MESSAGE_BEGIN.len()..].trim(),
        None => text.trim(),
    }
}

fn normalize_codex_message_text(role: &str, text: &str) -> Option<String> {
    let trimmed = text.trim();

    if trimmed.is_empty() || is_hidden_context_block(trimmed) {
        return None;
    }

    let normalized = if role == "user" {
        strip_user_message_prefix(trimmed).trim().to_string()
    } else {
        trimmed.to_string()
    };

    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn is_hidden_context_block(text: &str) -> bool {
    HIDDEN_CONTEXT_TAGS.iter().any(|tag| text.starts_with(tag))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_claude_message() {
        let mut context = SessionContext::default();
        let json = br#"{
            "type": "user",
            "message": {
                "role": "user",
                "content": "Hello from Claude"
            },
            "uuid": "claude-uuid",
            "timestamp": "2024-01-01T00:00:00Z",
            "sessionId": "claude-session",
            "parentUuid": null,
            "isSidechain": false,
            "userType": "external",
            "cwd": "/repo",
            "version": "1.0"
        }"#;

        let message = parse_searchable_message(json, &mut context).unwrap();

        assert_eq!(message.get_type(), "user");
        assert_eq!(message.get_content_text(), "Hello from Claude");
        assert_eq!(message.get_uuid(), Some("claude-uuid"));
        assert_eq!(message.get_session_id(), Some("claude-session"));
        assert_eq!(message.get_cwd(), Some("/repo"));
    }

    #[test]
    fn updates_context_from_codex_session_meta() {
        let mut context = SessionContext::default();
        let json = br#"{
            "timestamp": "2026-03-15T00:00:00Z",
            "type": "session_meta",
            "payload": {
                "id": "codex-session",
                "cwd": "/Users/test/project"
            }
        }"#;

        let message = parse_searchable_message(json, &mut context);

        assert!(message.is_none());
        assert_eq!(context.session_id.as_deref(), Some("codex-session"));
        assert_eq!(context.cwd.as_deref(), Some("/Users/test/project"));
    }

    #[test]
    fn parses_codex_response_messages_with_context() {
        let mut context = SessionContext {
            session_id: Some("codex-session".to_string()),
            cwd: Some("/Users/test/project".to_string()),
        };
        let user_json = br###"{
            "timestamp": "2026-03-15T00:00:01Z",
            "type": "response_item",
            "payload": {
                "type": "message",
                "role": "user",
                "content": [
                    {
                        "type": "input_text",
                        "text": "<user_instructions>\nignored\n</user_instructions>"
                    },
                    {
                        "type": "input_text",
                        "text": "## My request for Codex:\nFix the parser"
                    }
                ]
            }
        }"###;
        let assistant_json = br###"{
            "timestamp": "2026-03-15T00:00:02Z",
            "type": "response_item",
            "payload": {
                "type": "message",
                "role": "assistant",
                "content": [
                    {
                        "type": "output_text",
                        "text": "Updated the parser"
                    }
                ]
            }
        }"###;

        let user_message = parse_searchable_message(user_json, &mut context).unwrap();
        let assistant_message = parse_searchable_message(assistant_json, &mut context).unwrap();

        assert_eq!(user_message.get_type(), "user");
        assert_eq!(user_message.get_content_text(), "Fix the parser");
        assert_eq!(user_message.get_session_id(), Some("codex-session"));
        assert_eq!(user_message.get_cwd(), Some("/Users/test/project"));

        assert_eq!(assistant_message.get_type(), "assistant");
        assert_eq!(assistant_message.get_content_text(), "Updated the parser");
        assert_eq!(
            assistant_message.get_timestamp(),
            Some("2026-03-15T00:00:02Z")
        );
    }

    #[test]
    fn ignores_non_message_codex_lines() {
        let mut context = SessionContext::default();
        let json = br#"{
            "timestamp": "2026-03-15T00:00:03Z",
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": null
            }
        }"#;

        let message = parse_searchable_message(json, &mut context);

        assert!(message.is_none());
    }
}
