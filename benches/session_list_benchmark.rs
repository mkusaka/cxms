use codspeed_criterion_compat::{Criterion, black_box, criterion_group, criterion_main};
use cxms::SearchOptions;
use cxms::interactive_ratatui::SearchService;
use serde_json::json;
use std::fs::{self, File};
use std::io::Write;
use tempfile::tempdir;

fn setup_codex_home(
    num_days: usize,
    sessions_per_day: usize,
    project_cwd: &str,
) -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    let sessions_dir = temp_dir.path().join(".codex").join("sessions");
    fs::create_dir_all(&sessions_dir).unwrap();

    for day_idx in 0..num_days {
        let day = (day_idx % 28) + 1;
        let day_dir = sessions_dir
            .join("2026")
            .join("03")
            .join(format!("{day:02}"));
        fs::create_dir_all(&day_dir).unwrap();

        for session_idx in 0..sessions_per_day {
            let file_path = day_dir.join(format!(
                "rollout-2026-03-{day:02}T00-00-{session_idx:02}-session-{day_idx}-{session_idx}.jsonl"
            ));
            let mut file = File::create(&file_path).unwrap();
            writeln!(
                file,
                "{}",
                json!({
                    "timestamp": "2026-03-15T00:00:00Z",
                    "type": "session_meta",
                    "payload": {
                        "id": format!("session-{day_idx}-{session_idx}"),
                        "cwd": project_cwd,
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
                                "text": format!("## My request for Codex:\nInspect project session {session_idx}"),
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
                                "text": format!("Session {session_idx} indexed"),
                            }
                        ]
                    }
                })
            )
            .unwrap();
        }
    }

    temp_dir
}

fn benchmark_get_all_sessions(c: &mut Criterion) {
    let temp_home = setup_codex_home(30, 20, "/Users/masatomokusaka/src/github.com/mkusaka/cxms");
    let codex_home = temp_home.path().to_string_lossy().to_string();

    unsafe {
        std::env::set_var("HOME", &codex_home);
    }

    let mut group = c.benchmark_group("session_list");

    group.bench_function("get_all_sessions_filtered_project", |b| {
        let service = SearchService::new(SearchOptions {
            project_path: Some("/Users/masatomokusaka/src/github.com/mkusaka/cxms".to_string()),
            ..Default::default()
        });

        b.iter(|| {
            let sessions = service.get_all_sessions().unwrap();
            black_box(sessions.len())
        });
    });

    group.bench_function("get_all_sessions_all_projects", |b| {
        let service = SearchService::new(SearchOptions {
            project_path: Some("/".to_string()),
            ..Default::default()
        });

        b.iter(|| {
            let sessions = service.get_all_sessions().unwrap();
            black_box(sessions.len())
        });
    });

    group.finish();
}

criterion_group!(benches, benchmark_get_all_sessions);
criterion_main!(benches);
