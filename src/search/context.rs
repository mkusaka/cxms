use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};

use crate::query::{QueryCondition, SearchResult};
use crate::schemas::{SessionContext, parse_searchable_message};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextMessage {
    pub offset: isize,
    pub is_hit: bool,
    pub message: SearchResult,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AroundSearchResult {
    pub hit: SearchResult,
    pub context: Vec<ContextMessage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionOutline {
    pub session_id: String,
    pub file: String,
    pub cwd: String,
    pub first_timestamp: String,
    pub last_timestamp: String,
    pub matched_message_count: usize,
    pub total_message_count: usize,
    pub first_user_request_preview: Option<String>,
    pub latest_assistant_or_summary_preview: Option<String>,
}

pub fn codex_home_pattern(codex_home: &str) -> String {
    format!("{}/sessions/**/*.jsonl", codex_home.trim_end_matches('/'))
}

pub fn build_around_results(
    results: &[SearchResult],
    around: usize,
) -> Result<Vec<AroundSearchResult>> {
    let query = results.first().map(|r| r.query.clone());
    let mut cache = MessageFileCache::default();
    let mut used_indexes: HashMap<String, HashSet<usize>> = HashMap::new();
    let mut output = Vec::with_capacity(results.len());

    for hit in results {
        let Some(messages) = cache.messages_for(&hit.file, query.as_ref())? else {
            output.push(AroundSearchResult {
                hit: hit.clone(),
                context: vec![ContextMessage {
                    offset: 0,
                    is_hit: true,
                    message: hit.clone(),
                }],
            });
            continue;
        };

        let index = find_hit_index(
            hit,
            messages,
            used_indexes.entry(hit.file.clone()).or_default(),
        )
        .unwrap_or_else(|| {
            messages
                .iter()
                .position(|message| same_session(message, hit))
                .unwrap_or(0)
        });

        used_indexes
            .entry(hit.file.clone())
            .or_default()
            .insert(index);

        let start = index.saturating_sub(around);
        let end = (index + around + 1).min(messages.len());
        let context = messages[start..end]
            .iter()
            .enumerate()
            .filter_map(|(relative_index, message)| {
                if !same_session(message, hit) {
                    return None;
                }
                let absolute_index = start + relative_index;
                Some(ContextMessage {
                    offset: absolute_index as isize - index as isize,
                    is_hit: absolute_index == index,
                    message: message.clone(),
                })
            })
            .collect();

        output.push(AroundSearchResult {
            hit: hit.clone(),
            context,
        });
    }

    Ok(output)
}

pub fn build_session_outlines(
    results: &[SearchResult],
    max_sessions: usize,
) -> Result<Vec<SessionOutline>> {
    let query = results.first().map(|r| r.query.clone());
    let mut cache = MessageFileCache::default();
    let mut matched_counts: HashMap<(String, String), usize> = HashMap::new();

    for result in results {
        *matched_counts
            .entry((result.file.clone(), result.session_id.clone()))
            .or_insert(0) += 1;
    }

    let mut outlines = Vec::with_capacity(matched_counts.len());
    for ((file, session_id), matched_message_count) in matched_counts {
        let Some(messages) = cache.messages_for(&file, query.as_ref())? else {
            continue;
        };
        let session_messages: Vec<_> = messages
            .iter()
            .filter(|message| message.session_id == session_id)
            .collect();
        if session_messages.is_empty() {
            continue;
        }

        let first_timestamp = session_messages
            .iter()
            .filter_map(|message| non_empty(&message.timestamp))
            .min()
            .unwrap_or_default();
        let last_timestamp = session_messages
            .iter()
            .filter_map(|message| non_empty(&message.timestamp))
            .max()
            .unwrap_or_default();
        let cwd = session_messages
            .iter()
            .find_map(|message| non_empty(&message.cwd))
            .unwrap_or_default();
        let first_user_request_preview = session_messages
            .iter()
            .find(|message| message.role == "user")
            .map(|message| preview(&message.text, 160));
        let latest_assistant_or_summary_preview = session_messages
            .iter()
            .rev()
            .find(|message| message.role == "assistant" || message.role == "summary")
            .map(|message| preview(&message.text, 160));

        outlines.push(SessionOutline {
            session_id,
            file,
            cwd,
            first_timestamp,
            last_timestamp,
            matched_message_count,
            total_message_count: session_messages.len(),
            first_user_request_preview,
            latest_assistant_or_summary_preview,
        });
    }

    outlines.sort_by(|a, b| b.last_timestamp.cmp(&a.last_timestamp));
    outlines.truncate(max_sessions);
    Ok(outlines)
}

#[derive(Default)]
struct MessageFileCache {
    files: HashMap<String, Option<Vec<SearchResult>>>,
}

impl MessageFileCache {
    fn messages_for(
        &mut self,
        file: &str,
        query: Option<&QueryCondition>,
    ) -> Result<Option<&Vec<SearchResult>>> {
        if !self.files.contains_key(file) {
            let parsed = parse_file_messages(file, query)
                .with_context(|| format!("Failed to parse context messages from {file}"))?;
            self.files.insert(file.to_string(), Some(parsed));
        }

        Ok(self.files.get(file).and_then(Option::as_ref))
    }
}

fn parse_file_messages(
    file_path: &str,
    query: Option<&QueryCondition>,
) -> Result<Vec<SearchResult>> {
    let file = File::open(file_path)?;
    let mut reader = BufReader::with_capacity(64 * 1024, file);
    let mut line_buffer = Vec::with_capacity(16 * 1024);
    let mut session_context = SessionContext::default();
    let query = query.cloned().unwrap_or(QueryCondition::Literal {
        pattern: String::new(),
        case_sensitive: false,
    });

    let mut messages = Vec::new();
    loop {
        line_buffer.clear();
        let bytes_read = reader.read_until(b'\n', &mut line_buffer)?;
        if bytes_read == 0 {
            break;
        }
        if line_buffer.trim_ascii().is_empty() {
            continue;
        }
        if line_buffer.ends_with(b"\n") {
            line_buffer.pop();
            if line_buffer.ends_with(b"\r") {
                line_buffer.pop();
            }
        }

        if let Some(message) = parse_searchable_message(&line_buffer, &mut session_context) {
            let role = message.get_type().to_string();
            messages.push(SearchResult {
                file: file_path.to_string(),
                uuid: message.get_uuid().unwrap_or("").to_string(),
                timestamp: message.get_timestamp().unwrap_or("").to_string(),
                session_id: message.get_session_id().unwrap_or("").to_string(),
                role: role.clone(),
                text: message.get_content_text(),
                message_type: role,
                query: query.clone(),
                cwd: message.get_cwd().unwrap_or("").to_string(),
                raw_json: None,
            });
        }
    }

    Ok(messages)
}

fn find_hit_index(
    hit: &SearchResult,
    messages: &[SearchResult],
    used_indexes: &HashSet<usize>,
) -> Option<usize> {
    messages
        .iter()
        .enumerate()
        .find(|(index, message)| !used_indexes.contains(index) && same_message(message, hit))
        .map(|(index, _)| index)
}

fn same_message(candidate: &SearchResult, hit: &SearchResult) -> bool {
    if !same_session(candidate, hit) {
        return false;
    }
    if !hit.uuid.is_empty() {
        return candidate.uuid == hit.uuid;
    }

    candidate.timestamp == hit.timestamp
        && candidate.role == hit.role
        && candidate.text == hit.text
        && candidate.cwd == hit.cwd
}

fn same_session(candidate: &SearchResult, hit: &SearchResult) -> bool {
    candidate.file == hit.file && candidate.session_id == hit.session_id
}

fn non_empty(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn preview(text: &str, max_chars: usize) -> String {
    let cleaned = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.chars().count() <= max_chars {
        return cleaned;
    }

    let mut truncated = cleaned.chars().take(max_chars).collect::<String>();
    truncated.push_str("...");
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs::{File, create_dir_all};
    use std::io::Write;
    use tempfile::tempdir;

    fn write_codex_message(file: &mut File, ts: &str, role: &str, text: &str) -> Result<()> {
        let content_type = if role == "assistant" {
            "output_text"
        } else {
            "input_text"
        };
        writeln!(
            file,
            "{}",
            json!({
                "timestamp": ts,
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": role,
                    "content": [{"type": content_type, "text": text}]
                }
            })
        )?;
        Ok(())
    }

    fn write_session(file: &mut File, session_id: &str, cwd: &str) -> Result<()> {
        writeln!(
            file,
            "{}",
            json!({
                "timestamp": "2026-03-15T00:00:00Z",
                "type": "session_meta",
                "payload": {
                    "id": session_id,
                    "cwd": cwd,
                }
            })
        )?;
        Ok(())
    }

    #[test]
    fn codex_home_pattern_points_to_sessions_jsonl() {
        assert_eq!(
            codex_home_pattern("~/.codex-work2"),
            "~/.codex-work2/sessions/**/*.jsonl"
        );
    }

    #[test]
    fn around_includes_previous_and_next_message() -> Result<()> {
        let temp_dir = tempdir()?;
        let test_file = temp_dir.path().join("session.jsonl");
        let mut file = File::create(&test_file)?;
        write_session(&mut file, "s1", "/repo")?;
        write_codex_message(&mut file, "2026-03-15T00:00:01Z", "user", "before")?;
        write_codex_message(&mut file, "2026-03-15T00:00:02Z", "assistant", "needle")?;
        write_codex_message(&mut file, "2026-03-15T00:00:03Z", "user", "after")?;

        let hit = SearchResult {
            file: test_file.display().to_string(),
            uuid: String::new(),
            timestamp: "2026-03-15T00:00:02Z".to_string(),
            session_id: "s1".to_string(),
            role: "assistant".to_string(),
            text: "needle".to_string(),
            message_type: "assistant".to_string(),
            query: QueryCondition::Literal {
                pattern: "needle".to_string(),
                case_sensitive: false,
            },
            cwd: "/repo".to_string(),
            raw_json: None,
        };

        let results = build_around_results(&[hit], 1)?;

        assert_eq!(results[0].context.len(), 3);
        assert_eq!(results[0].context[0].offset, -1);
        assert_eq!(results[0].context[0].message.text, "before");
        assert!(results[0].context[1].is_hit);
        assert_eq!(results[0].context[2].message.text, "after");
        Ok(())
    }

    #[test]
    fn around_does_not_cross_session_boundary() -> Result<()> {
        let temp_dir = tempdir()?;
        let test_file = temp_dir.path().join("session.jsonl");
        let mut file = File::create(&test_file)?;
        write_session(&mut file, "s1", "/repo")?;
        write_codex_message(&mut file, "2026-03-15T00:00:01Z", "user", "target")?;
        write_session(&mut file, "s2", "/repo")?;
        write_codex_message(
            &mut file,
            "2026-03-15T00:00:02Z",
            "assistant",
            "other session",
        )?;

        let hit = SearchResult {
            file: test_file.display().to_string(),
            uuid: String::new(),
            timestamp: "2026-03-15T00:00:01Z".to_string(),
            session_id: "s1".to_string(),
            role: "user".to_string(),
            text: "target".to_string(),
            message_type: "user".to_string(),
            query: QueryCondition::Literal {
                pattern: "target".to_string(),
                case_sensitive: false,
            },
            cwd: "/repo".to_string(),
            raw_json: None,
        };

        let results = build_around_results(&[hit], 1)?;

        assert_eq!(results[0].context.len(), 1);
        assert_eq!(results[0].context[0].message.session_id, "s1");
        assert_eq!(results[0].context[0].message.text, "target");
        Ok(())
    }

    #[test]
    fn session_outline_groups_by_session() -> Result<()> {
        let temp_dir = tempdir()?;
        let test_file = temp_dir.path().join("session.jsonl");
        let mut file = File::create(&test_file)?;
        write_session(&mut file, "s1", "/repo")?;
        write_codex_message(&mut file, "2026-03-15T00:00:01Z", "user", "first request")?;
        write_codex_message(
            &mut file,
            "2026-03-15T00:00:02Z",
            "assistant",
            "needle answer",
        )?;
        write_codex_message(
            &mut file,
            "2026-03-15T00:00:03Z",
            "assistant",
            "latest answer",
        )?;

        let hit = SearchResult {
            file: test_file.display().to_string(),
            uuid: String::new(),
            timestamp: "2026-03-15T00:00:02Z".to_string(),
            session_id: "s1".to_string(),
            role: "assistant".to_string(),
            text: "needle answer".to_string(),
            message_type: "assistant".to_string(),
            query: QueryCondition::Literal {
                pattern: "needle".to_string(),
                case_sensitive: false,
            },
            cwd: "/repo".to_string(),
            raw_json: None,
        };

        let outlines = build_session_outlines(&[hit], 10)?;

        assert_eq!(outlines.len(), 1);
        assert_eq!(outlines[0].session_id, "s1");
        assert_eq!(outlines[0].matched_message_count, 1);
        assert_eq!(outlines[0].total_message_count, 3);
        assert_eq!(
            outlines[0].first_user_request_preview.as_deref(),
            Some("first request")
        );
        assert_eq!(
            outlines[0].latest_assistant_or_summary_preview.as_deref(),
            Some("latest answer")
        );
        Ok(())
    }

    #[test]
    fn pattern_priority_prefers_explicit_pattern_over_codex_home() {
        let explicit = Some("/tmp/custom/*.jsonl".to_string());
        let codex_home = Some("~/.codex-work2".to_string());
        let pattern = explicit
            .as_deref()
            .map(ToString::to_string)
            .or_else(|| codex_home.as_deref().map(codex_home_pattern))
            .unwrap();

        assert_eq!(pattern, "/tmp/custom/*.jsonl");
    }

    #[test]
    fn codex_home_search_pattern_matches_nested_sessions() -> Result<()> {
        let temp_dir = tempdir()?;
        let session_dir = temp_dir.path().join("sessions/2026/03/15");
        create_dir_all(&session_dir)?;
        let test_file = session_dir.join("session.jsonl");
        File::create(&test_file)?;

        let pattern = codex_home_pattern(&temp_dir.path().display().to_string());
        let files = crate::search::file_discovery::discover_codex_files(Some(&pattern))?;

        assert_eq!(files, vec![test_file]);
        Ok(())
    }
}
