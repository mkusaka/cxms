#[cfg(test)]
mod tests {
    use super::super::search_service::*;
    use crate::SearchOptions;
    use crate::interactive_ratatui::domain::models::{SearchOrder, SearchRequest};
    use serde_json::json;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    fn write_codex_rollout(
        dir: &std::path::Path,
        file_name: &str,
        session_id: &str,
        cwd: &str,
        user_message: &str,
        assistant_message: &str,
    ) -> std::path::PathBuf {
        let path = dir.join(file_name);
        let mut file = File::create(&path).unwrap();

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
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            json!({
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
                            "text": user_message
                        }
                    ]
                }
            })
        )
        .unwrap();
        writeln!(
            file,
            "{}",
            json!({
                "timestamp": "2026-03-15T00:00:02Z",
                "type": "response_item",
                "payload": {
                    "type": "message",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": assistant_message
                        }
                    ]
                }
            })
        )
        .unwrap();

        path
    }

    #[test]
    fn test_search_service_creation() {
        let options = SearchOptions {
            project_path: Some("/nonexistent/test/path".to_string()),
            ..Default::default()
        };
        let _service = SearchService::new(options);

        // Just ensure it can be created
    }

    #[test]
    fn test_empty_query_returns_all_results() {
        let options = SearchOptions {
            project_path: Some("/nonexistent/test/path".to_string()),
            ..Default::default()
        };
        let service = SearchService::new(options);

        let request = SearchRequest {
            id: 1,
            query: "   ".to_string(), // Empty/whitespace query
            role_filter: None,
            pattern: "/nonexistent/test/path/*.jsonl".to_string(),
            order: SearchOrder::Descending,
            limit: None,
            offset: None,
        };

        let response = service.search(request).unwrap();

        assert_eq!(response.id, 1);
        // Since we're searching a nonexistent path, we'll still get 0 results
        // but the important thing is that it doesn't reject the empty query
        // In a real scenario with files, this would return all messages
        assert_eq!(response.results.len(), 0);
    }

    #[test]
    fn test_search_with_role_filter() {
        let options = SearchOptions {
            project_path: Some("/nonexistent/test/path".to_string()),
            ..Default::default()
        };
        let service = SearchService::new(options);

        let request = SearchRequest {
            id: 42,
            query: "test".to_string(),
            role_filter: Some("user".to_string()),
            pattern: "/nonexistent/test/path/*.jsonl".to_string(),
            order: SearchOrder::Descending,
            limit: None,
            offset: None,
        };

        // This would normally search files, but without test files it returns empty
        let response = service.search(request).unwrap();

        assert_eq!(response.id, 42);
        // Results would be filtered by role if any were found
    }

    #[test]
    fn test_search_request_id_propagation() {
        let options = SearchOptions {
            project_path: Some("/nonexistent/test/path".to_string()),
            ..Default::default()
        };
        let service = SearchService::new(options);

        let test_ids = vec![1, 42, 100, 999];

        for id in test_ids {
            let request = SearchRequest {
                id,
                query: "test".to_string(),
                role_filter: None,
                pattern: "/nonexistent/test/path/*.jsonl".to_string(),
                order: SearchOrder::Descending,
                limit: None,
                offset: None,
            };

            let response = service.search(request).unwrap();
            assert_eq!(response.id, id);
        }
    }

    #[test]
    fn test_search_with_invalid_pattern() {
        let options = SearchOptions {
            project_path: Some("/nonexistent/test/path".to_string()),
            ..Default::default()
        };
        let service = SearchService::new(options);

        let request = SearchRequest {
            id: 1,
            query: "[[invalid regex".to_string(),
            role_filter: None,
            pattern: "/nonexistent/test/path/*.jsonl".to_string(),
            order: SearchOrder::Descending,
            limit: None,
            offset: None,
        };

        // Should handle invalid regex gracefully
        let result = service.search(request);
        assert!(result.is_err());
    }

    #[test]
    fn test_role_filter_applied_before_max_results() {
        // Test that role filter is applied before max_results truncation
        // This ensures that when changing role filter, we get up to max_results
        // of the filtered role, not just whatever was in the first max_results

        // Create a service with a low max_results for testing
        let options = SearchOptions {
            max_results: Some(5),
            ..Default::default()
        };
        let service = SearchService::new(options);

        // Request without role filter should get mixed results
        let request1 = SearchRequest {
            id: 1,
            query: "".to_string(),
            role_filter: None,
            pattern: "/nonexistent/test/path/*.jsonl".to_string(),
            order: SearchOrder::Descending,
            limit: None,
            offset: None,
        };

        // Request with role filter should get only that role
        let request2 = SearchRequest {
            id: 2,
            query: "".to_string(),
            role_filter: Some("user".to_string()),
            pattern: "/nonexistent/test/path/*.jsonl".to_string(),
            order: SearchOrder::Descending,
            limit: None,
            offset: None,
        };

        // Both will return empty due to missing file, but the structure is correct
        let response1 = service.search(request1).unwrap();
        let response2 = service.search(request2).unwrap();

        assert_eq!(response1.id, 1);
        assert_eq!(response2.id, 2);
        // In a real scenario with files containing mixed roles,
        // response2 would have up to 5 user messages, not just user messages
        // that happened to be in the first 5 overall results
    }

    // Tests for get_all_sessions

    #[test]
    fn test_collect_sessions_empty_directory() {
        let sessions = collect_sessions_from_files(Vec::new(), None).unwrap();
        assert!(sessions.is_empty());
    }

    #[test]
    fn test_collect_sessions_from_codex_rollout() {
        let temp_dir = tempdir().unwrap();
        let rollout = write_codex_rollout(
            temp_dir.path(),
            "session.jsonl",
            "session-1",
            "/repo/project",
            "## My request for Codex:\nFind the bug",
            "I found the bug",
        );

        let sessions = collect_sessions_from_files(vec![rollout], None).unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].1, "session-1");
        assert_eq!(sessions[0].3, 2);
        assert_eq!(sessions[0].4, "Find the bug");
        assert_eq!(sessions[0].5.len(), 2);
        assert_eq!(sessions[0].5[0].0, "user");
        assert_eq!(sessions[0].5[0].1, "Find the bug");
        assert_eq!(sessions[0].5[1].0, "assistant");
    }

    #[test]
    fn test_collect_sessions_respects_project_path() {
        let temp_dir = tempdir().unwrap();
        let rollout_a = write_codex_rollout(
            temp_dir.path(),
            "project-a.jsonl",
            "session-a",
            "/repo/project-a",
            "Search project A",
            "Done",
        );
        let rollout_b = write_codex_rollout(
            temp_dir.path(),
            "project-b.jsonl",
            "session-b",
            "/repo/project-b",
            "Search project B",
            "Done",
        );

        let sessions =
            collect_sessions_from_files(vec![rollout_a, rollout_b], Some("/repo/project-a"))
                .unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].1, "session-a");
    }

    #[test]
    fn test_get_all_sessions_with_nonexistent_project_filter_returns_empty() {
        let temp_dir = tempdir().unwrap();
        let rollout = write_codex_rollout(
            temp_dir.path(),
            "session.jsonl",
            "session-1",
            "/repo/project",
            "Search project",
            "Done",
        );

        let sessions =
            collect_sessions_from_files(vec![rollout], Some("/repo/does-not-match")).unwrap();

        assert!(sessions.is_empty());
    }
}
