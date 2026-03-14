use codspeed_criterion_compat::{Criterion, black_box, criterion_group, criterion_main};
use cxms::discover_codex_files;
use serde_json::json;
use std::fs::{self, File};
use std::io::Write;
use tempfile::tempdir;

fn setup_test_session_structure(num_days: usize, sessions_per_day: usize) -> tempfile::TempDir {
    let temp_dir = tempdir().unwrap();
    let sessions_dir = temp_dir.path().join(".codex").join("sessions");
    fs::create_dir_all(&sessions_dir).unwrap();

    for day_idx in 0..num_days {
        let year = 2026;
        let month = 3;
        let day = (day_idx % 28) + 1;
        let day_dir = sessions_dir
            .join(year.to_string())
            .join(format!("{month:02}"))
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
                    "timestamp": "2026-03-01T00:00:00Z",
                    "type": "session_meta",
                    "payload": {
                        "id": format!("session-{day_idx}-{session_idx}"),
                        "cwd": format!("/workspace/project-{session_idx}"),
                    }
                })
            )
            .unwrap();
            writeln!(
                file,
                "{}",
                json!({
                    "timestamp": "2026-03-01T00:00:01Z",
                    "type": "response_item",
                    "payload": {
                        "type": "message",
                        "role": "user",
                        "content": [
                            {
                                "type": "input_text",
                                "text": format!("## My request for Codex:\nTest message {session_idx} on day {day_idx}"),
                            }
                        ]
                    }
                })
            )
            .unwrap();
        }

        for extra_idx in 0..3 {
            let extra_path = day_dir.join(format!("other-{extra_idx}.txt"));
            File::create(&extra_path)
                .unwrap()
                .write_all(b"other file")
                .unwrap();
        }
    }

    temp_dir
}

fn benchmark_file_discovery(c: &mut Criterion) {
    let mut group = c.benchmark_group("file_discovery");

    let small_temp = setup_test_session_structure(10, 5);
    let small_pattern = format!("{}/**/*.jsonl", small_temp.path().display());
    group.bench_function("small_10x5", |b| {
        b.iter(|| {
            let files = discover_codex_files(Some(&small_pattern)).unwrap();
            black_box(files.len())
        });
    });

    let medium_temp = setup_test_session_structure(50, 10);
    let medium_pattern = format!("{}/**/*.jsonl", medium_temp.path().display());
    group.bench_function("medium_50x10", |b| {
        b.iter(|| {
            let files = discover_codex_files(Some(&medium_pattern)).unwrap();
            black_box(files.len())
        });
    });

    let large_temp = setup_test_session_structure(100, 20);
    let large_pattern = format!("{}/**/*.jsonl", large_temp.path().display());
    group.bench_function("large_100x20", |b| {
        b.iter(|| {
            let files = discover_codex_files(Some(&large_pattern)).unwrap();
            black_box(files.len())
        });
    });

    group.finish();
}

criterion_group!(benches, benchmark_file_discovery);
criterion_main!(benches);
