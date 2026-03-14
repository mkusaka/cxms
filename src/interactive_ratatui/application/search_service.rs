use crate::interactive_ratatui::domain::models::{SearchRequest, SearchResponse};
use crate::query::condition::{QueryCondition, SearchResult};
use crate::schemas::{SessionContext, parse_searchable_message};
use crate::search::SmolEngine;
use crate::search::engine::SearchEngineTrait;
use crate::search::file_discovery::discover_codex_files;
use crate::utils::path_encoding::cwd_belongs_to_project;
use crate::{SearchOptions, parse_query};
use anyhow::Result;
use std::io::BufRead;
use std::path::PathBuf;

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
}

impl SearchService {
    pub fn new(options: SearchOptions) -> Self {
        Self {
            base_options: options,
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
        let files = discover_codex_files(None)?;
        collect_sessions_from_files(files, self.base_options.project_path.as_deref())
    }
}

pub(crate) fn collect_sessions_from_files(
    files: Vec<PathBuf>,
    project_path: Option<&str>,
) -> Result<Vec<SessionData>> {
    let mut sessions: Vec<SessionData> = Vec::new();
    const MAX_PREVIEW_MESSAGES: usize = 5;

    for path in files {
        let Ok(file) = std::fs::File::open(&path) else {
            continue;
        };

        let reader = std::io::BufReader::new(file);
        let mut session_context = SessionContext::default();
        let mut session_id = String::new();
        let mut timestamp = String::new();
        let mut message_count = 0usize;
        let mut first_message = String::new();
        let mut preview_messages: Vec<(String, String, String)> = Vec::new();
        let mut summary_message: Option<String> = None;
        let mut skip_session = false;

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }

            let message = parse_searchable_message(line.as_bytes(), &mut session_context);

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
                "summary" => {
                    if !content.is_empty() {
                        summary_message = Some(content.clone());
                        if first_message.is_empty() {
                            first_message = content;
                        }
                    }
                }
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
        }

        if !skip_session && !session_id.is_empty() {
            sessions.push((
                path.to_string_lossy().to_string(),
                session_id,
                timestamp,
                message_count,
                first_message,
                preview_messages,
                summary_message,
            ));
        }
    }

    sessions.sort_by(|a, b| b.2.cmp(&a.2));
    Ok(sessions)
}
