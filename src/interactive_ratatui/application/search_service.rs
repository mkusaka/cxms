use crate::interactive_ratatui::domain::models::{SearchRequest, SearchResponse};
use crate::query::condition::{QueryCondition, SearchResult};
use crate::query::fast_lowercase::FastLowercase;
use crate::schemas::{SessionContext, parse_searchable_message};
use crate::search::SmolEngine;
use crate::search::engine::SearchEngineTrait;
use crate::search::file_discovery::discover_codex_files;
use crate::utils::path_encoding::cwd_belongs_to_project;
use crate::{SearchOptions, parse_query};
use anyhow::Result;
use rayon::prelude::*;
use std::collections::HashMap;
use std::io::{BufRead, Read};
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

// Type alias for session data: (file_path, session_id, timestamp, message_count, first_message, preview_messages, summary)
pub type SessionData = (
    String,
    String,
    String,
    usize,
    String,
    Vec<(String, String, String)>, // (role, content, timestamp)
    Option<String>,
);

pub struct SearchService {
    base_options: SearchOptions,
    session_list_cache: OnceLock<Vec<SessionData>>,
    raw_query_cache: RwLock<HashMap<(String, String), bool>>,
}

impl SearchService {
    pub fn new(options: SearchOptions) -> Self {
        Self {
            base_options: options,
            session_list_cache: OnceLock::new(),
            raw_query_cache: RwLock::new(HashMap::new()),
        }
    }

    pub fn search(&self, request: SearchRequest) -> Result<SearchResponse> {
        let results = self.execute_search(
            &request.query,
            &request.pattern,
            request.role_filter,
            request.order,
            None, // No session_id filter for general search
            request.limit,
            request.offset,
        )?;

        Ok(SearchResponse {
            id: request.id,
            results,
            error: None,
        })
    }

    // New method for session-specific search
    pub fn search_session(
        &self,
        request: SearchRequest,
        session_id: String,
    ) -> Result<SearchResponse> {
        let results = self.execute_search(
            &request.query,
            &request.pattern,
            request.role_filter,
            request.order,
            Some(session_id),
            request.limit,
            request.offset,
        )?;

        Ok(SearchResponse {
            id: request.id,
            results,
            error: None,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_search(
        &self,
        query: &str,
        pattern: &str,
        role_filter: Option<String>,
        order: crate::interactive_ratatui::domain::models::SearchOrder,
        session_id: Option<String>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<SearchResult>> {
        let query_condition = if query.trim().is_empty() {
            // Empty query means "match all" - use empty AND condition
            QueryCondition::And { conditions: vec![] }
        } else {
            parse_query(query)?
        };

        // Create a new options with session_id if provided
        let mut options = self.base_options.clone();

        if let Some(sid) = session_id {
            options.session_id = Some(sid);
            // For session viewer, show all messages without limit
            options.max_results = None;
        } else if limit.is_none() {
            // For regular search without explicit limit, use default
            // If limit is specified, we'll apply it after getting all results
            options.max_results = None;
        }

        // Create a new engine with the updated options
        let engine = SmolEngine::new(options);

        let (mut results, _, _) = engine.search_with_role_filter_and_order(
            pattern,
            query_condition,
            role_filter,
            order,
        )?;

        // Apply pagination if specified
        if let Some(offset_val) = offset {
            results = results.into_iter().skip(offset_val).collect();
        }

        if let Some(limit_val) = limit {
            results = results.into_iter().take(limit_val).collect();
        }

        // Results are already sorted by the engine based on the order
        Ok(results)
    }

    pub fn get_all_sessions(&self) -> Result<Vec<SessionData>> {
        if let Some(cached) = self.session_list_cache.get() {
            return Ok(cached.clone());
        }

        let files = discover_codex_files(None)?;
        let sessions =
            collect_sessions_from_files(files, self.base_options.project_path.as_deref())?;
        let _ = self.session_list_cache.set(sessions.clone());
        Ok(sessions)
    }

    pub fn session_matches_literal_query(&self, file_path: &str, query: &str) -> Result<bool> {
        if query.trim().is_empty() {
            return Ok(true);
        }

        let cache_key = (file_path.to_string(), query.to_string());
        if let Ok(cache) = self.raw_query_cache.read()
            && let Some(cached) = cache.get(&cache_key)
        {
            return Ok(*cached);
        }

        let matches = file_contains_query(file_path, query)?;
        if let Ok(mut cache) = self.raw_query_cache.write() {
            cache.insert(cache_key, matches);
        }

        Ok(matches)
    }
}

pub(crate) fn collect_sessions_from_files(
    files: Vec<PathBuf>,
    project_path: Option<&str>,
) -> Result<Vec<SessionData>> {
    let mut sessions: Vec<SessionData> = files
        .into_par_iter()
        .map(|path| collect_session_from_file(path, project_path))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();

    sessions.sort_by(|a, b| b.2.cmp(&a.2));
    Ok(sessions)
}

fn collect_session_from_file(
    path: PathBuf,
    project_path: Option<&str>,
) -> Result<Option<SessionData>> {
    const MAX_PREVIEW_MESSAGES: usize = 5;
    const FULL_SCAN_LIMIT_BYTES: u64 = 256 * 1024;

    let Ok(file) = std::fs::File::open(&path) else {
        return Ok(None);
    };
    let metadata_only = file
        .metadata()
        .map(|metadata| metadata.len() > FULL_SCAN_LIMIT_BYTES)
        .unwrap_or(false);

    let mut reader = std::io::BufReader::with_capacity(64 * 1024, file);
    let mut line_buffer = Vec::with_capacity(16 * 1024);
    let mut session_context = SessionContext::default();
    let mut session_id = String::new();
    let mut timestamp = String::new();
    let mut message_count = 0usize;
    let mut first_message = String::new();
    let mut preview_messages: Vec<(String, String, String)> = Vec::new();
    let mut summary_message: Option<String> = None;
    let mut skip_session = false;
    let mut scanned_bytes = 0u64;

    loop {
        line_buffer.clear();
        let bytes_read = reader.read_until(b'\n', &mut line_buffer)?;
        if bytes_read == 0 {
            break;
        }
        scanned_bytes += bytes_read as u64;
        if line_buffer.trim_ascii().is_empty() {
            continue;
        }
        trim_line_ending(&mut line_buffer);

        let needs_full_parse = session_context.session_id.is_none()
            || session_context.cwd.is_none()
            || timestamp.is_empty()
            || first_message.is_empty()
            || preview_messages.len() < MAX_PREVIEW_MESSAGES
            || looks_like_summary_line(&line_buffer);

        if needs_full_parse {
            let message = parse_searchable_message(&line_buffer, &mut session_context);

            if let Some(project_path) = project_path
                && let Some(cwd) = session_context.cwd.as_deref()
                && !cwd_belongs_to_project(cwd, project_path)
            {
                skip_session = true;
                break;
            }

            let Some(message) = message else {
                continue;
            };

            message_count += 1;

            if session_id.is_empty() {
                session_id = message.get_session_id().unwrap_or_default().to_string();
            }

            if timestamp.is_empty() {
                timestamp = message.get_timestamp().unwrap_or_default().to_string();
            }

            let content = message
                .get_content_text()
                .chars()
                .take(200)
                .collect::<String>()
                .replace('\n', " ");

            match message.get_type() {
                "summary" if !content.is_empty() => {
                    summary_message = Some(content.clone());
                    if first_message.is_empty() {
                        first_message = content;
                    }
                }
                "summary" => {}
                "user" | "assistant" => {
                    if first_message.is_empty()
                        && message.get_type() == "user"
                        && !content.is_empty()
                    {
                        first_message = content.clone();
                    }

                    if preview_messages.len() < MAX_PREVIEW_MESSAGES && !content.is_empty() {
                        preview_messages.push((
                            message.get_type().to_string(),
                            content,
                            message.get_timestamp().unwrap_or_default().to_string(),
                        ));
                    }
                }
                _ => {}
            }
        } else if looks_like_message_line(&line_buffer) {
            message_count += 1;
        }

        if metadata_only
            && (session_has_display_metadata(
                &session_context,
                &timestamp,
                &first_message,
                &preview_messages,
            ) || (scanned_bytes >= FULL_SCAN_LIMIT_BYTES
                && session_has_minimum_metadata(
                    &session_context,
                    &timestamp,
                    &first_message,
                    &preview_messages,
                )))
        {
            break;
        }
    }

    if !skip_session && session_id.is_empty() {
        session_id = session_context.session_id.unwrap_or_default();
    }

    if !skip_session && !session_id.is_empty() {
        Ok(Some((
            path.to_string_lossy().to_string(),
            session_id,
            timestamp,
            message_count,
            first_message,
            preview_messages,
            summary_message,
        )))
    } else {
        Ok(None)
    }
}

fn session_has_display_metadata(
    context: &SessionContext,
    timestamp: &str,
    first_message: &str,
    preview_messages: &[(String, String, String)],
) -> bool {
    context.session_id.is_some()
        && context.cwd.is_some()
        && !timestamp.is_empty()
        && !first_message.is_empty()
        && preview_messages.len() >= 5
}

fn session_has_minimum_metadata(
    context: &SessionContext,
    timestamp: &str,
    first_message: &str,
    preview_messages: &[(String, String, String)],
) -> bool {
    context.session_id.is_some()
        && context.cwd.is_some()
        && !timestamp.is_empty()
        && (!first_message.is_empty() || !preview_messages.is_empty())
}

fn trim_line_ending(line: &mut Vec<u8>) {
    if line.ends_with(b"\n") {
        line.pop();
        if line.ends_with(b"\r") {
            line.pop();
        }
    }
}

fn looks_like_message_line(line: &[u8]) -> bool {
    contains_bytes(line, br#""response_item""#)
        || contains_bytes(line, br#""type":"user""#)
        || contains_bytes(line, br#""type":"assistant""#)
        || contains_bytes(line, br#""type":"summary""#)
        || contains_bytes(line, br#""type": "user""#)
        || contains_bytes(line, br#""type": "assistant""#)
        || contains_bytes(line, br#""type": "summary""#)
}

fn looks_like_summary_line(line: &[u8]) -> bool {
    contains_bytes(line, br#""type":"summary""#) || contains_bytes(line, br#""type": "summary""#)
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn file_contains_query(file_path: &str, query: &str) -> Result<bool> {
    let file = std::fs::File::open(file_path)?;
    let mut reader = std::io::BufReader::with_capacity(64 * 1024, file);
    let query_lower = query.fast_to_lowercase();

    if query_lower.is_ascii() {
        return file_contains_ascii_query(&mut reader, query_lower.as_bytes());
    }

    let mut line = String::new();
    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line)?;
        if bytes_read == 0 {
            return Ok(false);
        }
        let line_bytes = line.as_bytes();
        let searchable_region = searchable_line_region(line_bytes);
        if looks_like_message_line(line_bytes)
            && std::str::from_utf8(searchable_region)
                .map(|region| region.fast_to_lowercase().contains(&query_lower))
                .unwrap_or(false)
        {
            return Ok(true);
        }
    }
}

fn file_contains_ascii_query<R: Read>(
    reader: &mut std::io::BufReader<R>,
    query: &[u8],
) -> Result<bool> {
    let mut line = Vec::with_capacity(16 * 1024);

    loop {
        line.clear();
        let bytes_read = reader.read_until(b'\n', &mut line)?;
        if bytes_read == 0 {
            return Ok(false);
        }
        if looks_like_message_line(&line)
            && contains_ascii_case_insensitive(searchable_line_region(&line), query)
        {
            return Ok(true);
        }
    }
}

fn searchable_line_region(line: &[u8]) -> &[u8] {
    find_bytes(line, br#""content""#)
        .or_else(|| find_bytes(line, br#""text""#))
        .or_else(|| find_bytes(line, br#""summary""#))
        .map(|index| &line[index..])
        .unwrap_or(line)
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn contains_ascii_case_insensitive(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }

    haystack.windows(needle.len()).any(|window| {
        window
            .iter()
            .zip(needle)
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
    })
}
