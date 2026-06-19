#[cfg(feature = "mimalloc")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::{Command, CommandFactory, Parser, ValueEnum};
use clap_complete::{Generator, Shell, generate};
#[cfg(all(feature = "profiling", unix))]
use cxms::profiling_enhanced;
use cxms::{
    AroundSearchResult, QueryCondition, RayonEngine, SearchEngineTrait, SearchOptions,
    SearchResult, SessionOutline, SmolEngine, Statistics, build_around_results,
    build_session_outlines, codex_home_pattern, default_codex_pattern, format_search_result,
    interactive_ratatui::InteractiveSearch, parse_query, profiling,
};
use parse_datetime::parse_datetime;
use std::collections::HashMap;
use std::io::{self, Write};

#[derive(Parser)]
#[command(
    name = "cxms",
    version,
    about = "High-performance CLI for searching Codex session rollout JSONL files",
    long_about = None
)]
struct Cli {
    /// Search query (supports literal, regex, AND/OR/NOT operators). If not provided, enters interactive mode.
    query: Option<String>,

    /// File pattern to search (default: ~/.codex/sessions/**/*.jsonl)
    #[arg(short, long)]
    pattern: Option<String>,

    /// Codex home to search (uses <PATH>/sessions/**/*.jsonl; overridden by --pattern)
    #[arg(long)]
    codex_home: Option<String>,

    /// Filter by message role (user, assistant, system, summary)
    #[arg(short, long)]
    role: Option<String>,

    /// Filter by session ID
    #[arg(short, long)]
    session_id: Option<String>,

    /// Jump directly to the latest message detail in the most recent session
    #[arg(long, conflicts_with_all = ["session_id", "latest_session"])]
    latest: bool,

    /// Jump directly to the most recent session's detail view
    #[arg(long, conflicts_with = "session_id")]
    latest_session: bool,

    /// Search for a specific message by UUID
    #[arg(long)]
    message_id: Option<String>,

    /// Maximum number of results to return
    #[arg(short = 'n', long, default_value = "200")]
    max_results: usize,

    /// Filter messages before this timestamp (RFC3339 format)
    #[arg(long)]
    before: Option<String>,

    /// Filter messages after this timestamp (RFC3339 format)
    #[arg(long)]
    after: Option<String>,

    /// Filter messages since this time (Unix timestamp or relative time like "1 day ago")
    #[arg(long)]
    since: Option<String>,

    /// Output format
    #[arg(short = 'f', long, value_enum, default_value = "text")]
    format: OutputFormat,

    /// Disable colored output
    #[arg(long)]
    no_color: bool,

    /// Enable verbose output
    #[arg(short, long)]
    verbose: bool,

    /// Show query syntax help
    #[arg(long)]
    help_query: bool,

    /// Show full message text without truncation
    #[arg(long)]
    full_text: bool,

    /// Show raw JSON of matched messages
    #[arg(long)]
    raw: bool,

    /// Include N messages before and after each matched message in CLI output
    #[arg(long, default_value = "0")]
    around: usize,

    /// Output matching sessions as session-level outlines instead of message results
    #[arg(long)]
    session_outline: bool,

    /// Filter by working directory (cwd) path
    #[arg(long = "project")]
    project_path: Option<String>,

    /// Generate profiling report (requires --features profiling)
    #[cfg(all(feature = "profiling", unix))]
    #[arg(long)]
    profile: Option<String>,

    #[cfg(not(all(feature = "profiling", unix)))]
    #[arg(long, hide = true)]
    profile: Option<String>,

    /// Generate shell completion script
    #[arg(long = "completion", value_enum)]
    generator: Option<Shell>,

    /// Search engine to use
    #[arg(long, value_enum, default_value = "smol")]
    engine: EngineType,

    /// Show only statistics
    #[arg(long)]
    stats: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OutputFormat {
    Text,
    Json,
    JsonL,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum EngineType {
    Smol,
    Rayon,
}

fn print_completions<G: Generator>(generator: G, cmd: &mut Command) {
    generate(
        generator,
        cmd,
        cmd.get_name().to_string(),
        &mut io::stdout(),
    );
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Handle completion generation
    if let Some(generator) = cli.generator {
        let mut cmd = Cli::command();
        eprintln!("Generating completion file for {generator:?}...");
        print_completions(generator, &mut cmd);
        return Ok(());
    }

    // Initialize tracing
    profiling::init_tracing();

    if cli.help_query {
        print_query_help();
        return Ok(());
    }

    if cli.stats && cli.session_outline {
        eprintln!("Error: --stats cannot be used with --session-outline");
        std::process::exit(1);
    }
    if cli.stats && cli.around > 0 {
        eprintln!("Error: --stats cannot be used with --around");
        std::process::exit(1);
    }
    if cli.raw && cli.session_outline {
        eprintln!("Error: --raw cannot be used with --session-outline");
        std::process::exit(1);
    }
    if cli.raw && cli.around > 0 {
        eprintln!("Error: --raw cannot be used with --around");
        std::process::exit(1);
    }

    // Initialize profiler if requested
    #[cfg(all(feature = "profiling", unix))]
    let mut profiler = if cli.profile.is_some() {
        Some(profiling_enhanced::EnhancedProfiler::new("main")?)
    } else {
        None
    };

    #[cfg(not(all(feature = "profiling", unix)))]
    if cli.profile.is_some() {
        eprintln!(
            "Warning: Profiling is not enabled. Build with --features profiling to enable profiling."
        );
    }

    // Parse --since flag if provided
    let parsed_after = if let Some(since) = &cli.since {
        match parse_since_time(since) {
            Ok(dt) => Some(dt),
            Err(e) => {
                eprintln!("Error parsing --since: {e}");
                std::process::exit(1);
            }
        }
    } else {
        cli.after.clone()
    };

    // Set default project_path to current directory if not specified
    let project_path = cli.project_path.clone().or_else(|| {
        std::env::current_dir()
            .ok()
            .and_then(|path| path.to_str().map(|s| s.to_string()))
    });

    // Get pattern
    let pattern = resolve_pattern(cli.pattern.as_deref(), cli.codex_home.as_deref());

    // Handle --message-id search
    if let Some(message_id) = &cli.message_id {
        // Create a special query to search for the UUID
        let query = parse_query(message_id)?;

        // Create search options
        let options = SearchOptions {
            max_results: Some(1), // We only need one result
            role: None,
            session_id: None,
            message_id: Some(message_id.clone()),
            before: None,
            after: None,
            verbose: cli.verbose,
            project_path: None,
        };

        if cli.verbose {
            eprintln!("Searching for message ID: {message_id}");
        }

        // Execute search
        let engine = SmolEngine::new(options);
        let (results, duration, _) = engine.search(&pattern, query)?;

        if results.is_empty() {
            eprintln!("Message with ID '{message_id}' not found.");
            std::process::exit(1);
        }

        // Pretty print the message
        let result = &results[0];
        print_message_details(result, !cli.no_color);

        if cli.verbose {
            eprintln!("\n⏱️  Search completed in {}ms", duration.as_millis());
        }

        return Ok(());
    }

    // Handle --latest mode
    if cli.latest {
        if cli.query.as_ref().map(|q| !q.is_empty()).unwrap_or(false) {
            eprintln!("Error: --latest cannot be used with a search query");
            std::process::exit(1);
        }

        let options = SearchOptions {
            max_results: None, // Interactive mode should not be limited by max_results
            role: cli.role,
            session_id: None,
            message_id: None,
            before: cli.before,
            after: parsed_after.clone(),
            verbose: cli.verbose,
            project_path: project_path.clone(),
        };

        let mut interactive = InteractiveSearch::new(options);
        interactive.set_start_latest_message_detail(true);
        return interactive.run(&pattern);
    }

    // Handle --latest-session mode
    if cli.latest_session {
        if cli.query.as_ref().map(|q| !q.is_empty()).unwrap_or(false) {
            eprintln!("Error: --latest-session cannot be used with a search query");
            std::process::exit(1);
        }

        let options = SearchOptions {
            max_results: None, // Interactive mode should not be limited by max_results
            role: cli.role,
            session_id: None,
            message_id: None,
            before: cli.before,
            after: parsed_after.clone(),
            verbose: cli.verbose,
            project_path: project_path.clone(),
        };

        let mut interactive = InteractiveSearch::new(options);
        interactive.set_start_latest(true);
        return interactive.run(&pattern);
    }

    // Interactive mode when no query provided or query is empty (but not when --stats is used)
    if !cli.stats
        && (cli.query.is_none() || cli.query.as_ref().map(|s| s.is_empty()).unwrap_or(false))
    {
        let options = SearchOptions {
            max_results: None, // Interactive mode should not be limited by max_results
            role: cli.role,
            session_id: cli.session_id,
            message_id: None,
            before: cli.before,
            after: parsed_after.clone(),
            verbose: cli.verbose,
            project_path: project_path.clone(),
        };

        let mut interactive = InteractiveSearch::new(options);
        return interactive.run(&pattern);
    }

    // Regular search mode - query is provided (or empty string for --stats)
    let query_str = cli.query.unwrap_or_else(String::new);

    // Parse the query (empty query for --stats means match all)
    let query = if cli.stats && query_str.is_empty() {
        // Empty query for stats: match everything
        QueryCondition::Literal {
            pattern: String::new(),
            case_sensitive: false,
        }
    } else {
        match parse_query(&query_str) {
            Ok(q) => q,
            Err(e) => {
                eprintln!("Error parsing query: {e}");
                eprintln!("Use --help-query for query syntax help");
                std::process::exit(1);
            }
        }
    };

    // Create search options
    let options = SearchOptions {
        max_results: if cli.stats || cli.session_outline {
            None // Don't limit results when calculating statistics or session outlines
        } else {
            Some(cli.max_results)
        },
        role: cli.role,
        session_id: cli.session_id,
        message_id: None,
        before: cli.before,
        after: parsed_after,
        verbose: cli.verbose,
        project_path,
    };

    if cli.verbose {
        eprintln!("Searching in: {pattern}");
        eprintln!("Query: {query:?}");
    }

    // Debug: only search specific file
    let debug_file = "/Users/masatomokusaka/.codex/sessions/debug.jsonl";
    let pattern_to_use = if std::env::var("DEBUG_SINGLE_FILE").is_ok() {
        eprintln!("DEBUG: Searching only {debug_file}");
        debug_file.to_string()
    } else {
        pattern.clone()
    };

    // Execute search
    if cli.verbose {
        eprintln!(
            "Using {} engine",
            match cli.engine {
                EngineType::Smol => "Smol",
                EngineType::Rayon => "Rayon",
            }
        );
    }

    // Create appropriate engine based on CLI flag
    let (results, duration, total_count) = match cli.engine {
        EngineType::Smol => {
            let engine = SmolEngine::new(options);
            engine.search(&pattern_to_use, query)?
        }
        EngineType::Rayon => {
            let engine = RayonEngine::new(options);
            engine.search(&pattern_to_use, query)?
        }
    };

    // If stats flag is set, collect and display statistics
    if cli.stats {
        let stats = collect_statistics(&results);
        println!("{}", cxms::stats::format_statistics(&stats, !cli.no_color));

        eprintln!("\n⏱️  Search completed in {}ms", duration.as_millis());
        if total_count > results.len() {
            eprintln!(
                "(Showing stats for {} of {} total results)",
                results.len(),
                total_count
            );
        }
        return Ok(());
    }

    if cli.session_outline {
        let outlines = build_session_outlines(&results, cli.max_results)?;
        output_session_outlines(
            &outlines,
            cli.format,
            !cli.no_color,
            duration,
            total_count,
            results.len(),
        )?;
        return Ok(());
    }

    let around_results = if cli.around > 0 {
        Some(build_around_results(&results, cli.around)?)
    } else {
        None
    };

    // Output results
    let stdout = io::stdout();
    let mut handle = stdout.lock();

    match cli.format {
        OutputFormat::Text => {
            if results.is_empty() {
                println!("No results found.");
            } else if cli.raw {
                // Raw mode: output raw JSON lines
                for result in &results {
                    if let Some(raw_json) = &result.raw_json {
                        println!("{raw_json}");
                    }
                }
            } else if let Some(around_results) = &around_results {
                println!("Found {} results:\n", around_results.len());
                for result in around_results {
                    println!(
                        "{}",
                        format_around_search_result(result, !cli.no_color, cli.full_text)
                    );
                }

                // Print search statistics
                eprintln!("\n⏱️  Search completed in {}ms", duration.as_millis());
                if total_count > results.len() {
                    eprintln!(
                        "(Showing {} of {} total results)",
                        results.len(),
                        total_count
                    );
                } else {
                    eprintln!("(Found {total_count} results)");
                }
            } else {
                println!("Found {} results:\n", results.len());
                for result in &results {
                    println!(
                        "{}",
                        format_search_result(result, !cli.no_color, cli.full_text)
                    );
                }

                // Print search statistics
                eprintln!("\n⏱️  Search completed in {}ms", duration.as_millis());
                if total_count > results.len() {
                    eprintln!(
                        "(Showing {} of {} total results)",
                        results.len(),
                        total_count
                    );
                } else {
                    eprintln!("(Found {total_count} results)");
                }
            }
        }
        OutputFormat::Json => {
            // Collect statistics
            let mut session_counts: HashMap<String, usize> = HashMap::new();
            let mut file_counts: HashMap<String, usize> = HashMap::new();

            for result in &results {
                *session_counts.entry(result.session_id.clone()).or_insert(0) += 1;
                *file_counts.entry(result.file.clone()).or_insert(0) += 1;
            }

            // Create detailed file information
            let files_detail: Vec<_> = file_counts
                .iter()
                .map(|(file, count)| {
                    serde_json::json!({
                        "path": file,
                        "message_count": count,
                        "session_id": results.iter()
                            .find(|r| &r.file == file)
                            .map(|r| &r.session_id)
                            .unwrap_or(&String::new())
                    })
                })
                .collect();

            // Create detailed session information
            let sessions_detail: Vec<_> = session_counts
                .iter()
                .map(|(session_id, count)| {
                    serde_json::json!({
                        "session_id": session_id,
                        "message_count": count
                    })
                })
                .collect();

            let output = serde_json::json!({
                "results": around_results
                    .as_ref()
                    .map(serde_json::to_value)
                    .transpose()?
                    .unwrap_or_else(|| serde_json::to_value(&results).unwrap_or(serde_json::Value::Null)),
                "summary": {
                    "duration_ms": duration.as_millis(),
                    "total_count": total_count,
                    "returned_count": results.len(),
                    "unique_sessions": session_counts.len(),
                    "unique_files": file_counts.len()
                },
                "files": files_detail,
                "sessions": sessions_detail
            });
            serde_json::to_writer_pretty(&mut handle, &output)?;
            writeln!(&mut handle)?;
        }
        OutputFormat::JsonL => {
            if let Some(around_results) = &around_results {
                for result in around_results {
                    serde_json::to_writer(&mut handle, result)?;
                    writeln!(&mut handle)?;
                }
            } else {
                for result in &results {
                    serde_json::to_writer(&mut handle, result)?;
                    writeln!(&mut handle)?;
                }
            }
            // Write metadata as last line
            let metadata = serde_json::json!({
                "_metadata": {
                    "duration_ms": duration.as_millis(),
                    "total_count": total_count,
                    "returned_count": results.len()
                }
            });
            serde_json::to_writer(&mut handle, &metadata)?;
            writeln!(&mut handle)?;
        }
    }

    // Generate profiling report if requested
    #[cfg(all(feature = "profiling", unix))]
    if let Some(ref mut profiler) = profiler
        && let Some(profile_path) = &cli.profile
    {
        let report = profiler.generate_comprehensive_report(profile_path)?;
        eprintln!("\n{report}");
        eprintln!("\nDetailed profiling reports saved to {profile_path}_{{comprehensive.txt,svg}}");
    }

    Ok(())
}

fn resolve_pattern(pattern: Option<&str>, codex_home: Option<&str>) -> String {
    pattern
        .map(ToString::to_string)
        .or_else(|| codex_home.map(codex_home_pattern))
        .unwrap_or_else(default_codex_pattern)
}

fn output_session_outlines(
    outlines: &[SessionOutline],
    format: OutputFormat,
    use_color: bool,
    duration: std::time::Duration,
    total_match_count: usize,
    returned_match_count: usize,
) -> Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();

    match format {
        OutputFormat::Text => {
            if outlines.is_empty() {
                println!("No matching sessions found.");
            } else {
                println!("Found {} sessions:\n", outlines.len());
                for outline in outlines {
                    println!("{}", format_session_outline(outline, use_color));
                }
            }
            eprintln!("\n⏱️  Search completed in {}ms", duration.as_millis());
            eprintln!(
                "(Grouped {returned_match_count} matching messages into {} sessions)",
                outlines.len()
            );
        }
        OutputFormat::Json => {
            let output = serde_json::json!({
                "sessions": outlines,
                "summary": {
                    "duration_ms": duration.as_millis(),
                    "total_match_count": total_match_count,
                    "returned_match_count": returned_match_count,
                    "returned_session_count": outlines.len()
                }
            });
            serde_json::to_writer_pretty(&mut handle, &output)?;
            writeln!(&mut handle)?;
        }
        OutputFormat::JsonL => {
            for outline in outlines {
                serde_json::to_writer(&mut handle, outline)?;
                writeln!(&mut handle)?;
            }
            let metadata = serde_json::json!({
                "_metadata": {
                    "duration_ms": duration.as_millis(),
                    "total_match_count": total_match_count,
                    "returned_match_count": returned_match_count,
                    "returned_session_count": outlines.len()
                }
            });
            serde_json::to_writer(&mut handle, &metadata)?;
            writeln!(&mut handle)?;
        }
    }

    Ok(())
}

fn format_session_outline(outline: &SessionOutline, use_color: bool) -> String {
    use colored::Colorize;

    let title = if use_color {
        format!(
            "{} [{} matches / {} messages]",
            outline.session_id.bright_yellow(),
            outline.matched_message_count,
            outline.total_message_count
        )
    } else {
        format!(
            "{} [{} matches / {} messages]",
            outline.session_id, outline.matched_message_count, outline.total_message_count
        )
    };

    let mut lines = vec![
        title,
        format!("  cwd: {}", outline.cwd),
        format!("  file: {}", outline.file),
        format!(
            "  time: {} -> {}",
            outline.first_timestamp, outline.last_timestamp
        ),
    ];

    if let Some(preview) = &outline.first_user_request_preview {
        lines.push(format!("  first user: {preview}"));
    }
    if let Some(preview) = &outline.latest_assistant_or_summary_preview {
        lines.push(format!("  latest assistant/summary: {preview}"));
    }

    lines.join("\n")
}

fn format_around_search_result(
    result: &AroundSearchResult,
    use_color: bool,
    full_text: bool,
) -> String {
    use colored::Colorize;

    let mut lines = Vec::new();
    for context in &result.context {
        let marker = if context.is_hit { "=>" } else { "  " };
        let label = if context.is_hit { "hit" } else { "context" };
        let prefix = if use_color && context.is_hit {
            format!(
                "{} {}",
                marker.bright_yellow().bold(),
                label.bright_yellow()
            )
        } else {
            format!("{marker} {label}")
        };
        let formatted = format_search_result(&context.message, use_color, full_text);
        lines.push(format!("{prefix} offset={}\n{formatted}", context.offset));
    }

    lines.join("\n")
}

fn parse_since_time(input: &str) -> Result<String> {
    use anyhow::Context;

    // First, try to parse as Unix timestamp
    if let Ok(timestamp) = input.parse::<i64>() {
        let dt = DateTime::<Utc>::from_timestamp(timestamp, 0).context("Invalid Unix timestamp")?;
        return Ok(dt.to_rfc3339());
    }

    // Try to parse as relative time using parse_datetime
    match parse_datetime(input) {
        Ok(dt) => Ok(dt.timestamp().to_string()),
        Err(e) => Err(anyhow::anyhow!(
            "Failed to parse time '{input}': {e}. Expected Unix timestamp or relative time like '1 day ago'"
        )),
    }
}

fn print_message_details(result: &SearchResult, use_color: bool) {
    use chrono::{DateTime, Local, TimeZone};
    use colored::Colorize;

    // Parse timestamp to local time
    let timestamp = if let Ok(dt) = DateTime::parse_from_rfc3339(&result.timestamp) {
        let local_dt = Local.from_utc_datetime(&dt.naive_utc());
        local_dt.format("%Y-%m-%d %H:%M:%S").to_string()
    } else {
        result.timestamp.clone()
    };

    // Pretty print the message details
    if use_color {
        println!("{}", "Message Details".bright_blue().bold());
        println!("{}", "═".repeat(80).bright_blue());
        println!("{}: {}", "Message ID".bright_yellow(), result.uuid);
        println!("{}: {}", "Type".bright_yellow(), result.message_type);
        println!("{}: {}", "Role".bright_yellow(), result.role.bright_green());
        println!(
            "{}: {}",
            "Timestamp".bright_yellow(),
            timestamp.bright_cyan()
        );
        println!(
            "{}: {}",
            "Session ID".bright_yellow(),
            result.session_id.dimmed()
        );
        println!(
            "{}: {}",
            "File".bright_yellow(),
            result.file.bright_magenta()
        );
        println!("{}: {}", "Working Dir".bright_yellow(), result.cwd.dimmed());
        println!("\n{}", "Content".bright_yellow().bold());
        println!("{}", "─".repeat(80).bright_blue());

        // Print the full content with nice formatting
        let content_lines: Vec<&str> = result.text.lines().collect();
        for line in content_lines {
            println!("{line}");
        }

        if let Some(raw_json) = &result.raw_json {
            println!("\n{}", "Raw JSON".bright_yellow().bold());
            println!("{}", "─".repeat(80).bright_blue());
            // Pretty print JSON if possible
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(raw_json) {
                if let Ok(pretty) = serde_json::to_string_pretty(&parsed) {
                    println!("{}", pretty.dimmed());
                } else {
                    println!("{}", raw_json.dimmed());
                }
            } else {
                println!("{}", raw_json.dimmed());
            }
        }
    } else {
        println!("Message Details");
        println!("{}", "=".repeat(80));
        println!("Message ID: {}", result.uuid);
        println!("Type: {}", result.message_type);
        println!("Role: {}", result.role);
        println!("Timestamp: {timestamp}");
        println!("Session ID: {}", result.session_id);
        println!("File: {}", result.file);
        println!("Working Dir: {}", result.cwd);
        println!("\nContent");
        println!("{}", "-".repeat(80));

        // Print the full content
        println!("{}", result.text);

        if let Some(raw_json) = &result.raw_json {
            println!("\nRaw JSON");
            println!("{}", "-".repeat(80));
            // Pretty print JSON if possible
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(raw_json) {
                if let Ok(pretty) = serde_json::to_string_pretty(&parsed) {
                    println!("{pretty}");
                } else {
                    println!("{raw_json}");
                }
            } else {
                println!("{raw_json}");
            }
        }
    }
}

fn collect_statistics(results: &[SearchResult]) -> Statistics {
    let mut stats = Statistics::new();

    for result in results {
        stats.add_message(
            &result.role,
            &result.session_id,
            &result.file,
            &result.timestamp,
            &result.cwd,
            &result.message_type,
        );
    }

    stats
}

fn print_query_help() {
    println!(
        r#"Codex Search Query Syntax Help

BASIC QUERIES:
  hello                   Literal search (case-insensitive)
  "hello world"          Quoted literal (preserves spaces)
  'hello world'          Single-quoted literal
  /hello.*world/i        Regular expression with flags

OPERATORS:
  hello AND world        Both terms must be present
  hello OR world         Either term must be present
  NOT hello              Term must not be present
  (hello OR hi) AND bye  Parentheses for grouping

REGEX FLAGS:
  i - Case insensitive
  m - Multi-line mode
  s - Dot matches newline

EXAMPLES:
  error AND /failed.*connection/i
  "user message" AND NOT system
  (warning OR error) AND timestamp
  /^Error:.*\d+/m

ROLE FILTERS (via --role):
  user, assistant, system, summary

TIPS:
  - Unquoted literals cannot contain spaces or special characters
  - Use quotes for exact phrases with spaces
  - Regular expressions must be enclosed in forward slashes
  - AND has higher precedence than OR
  - Use --stats flag to see only statistics without message content"#
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_unix_timestamp() {
        // Test Unix timestamp parsing
        let result = parse_since_time("1704067200"); // 2024-01-01 00:00:00 UTC
        assert!(result.is_ok());
        let dt = result.unwrap();
        assert!(dt.starts_with("2024-01-01"));
    }

    #[test]
    fn test_parse_relative_time() {
        // Test relative time parsing
        let result = parse_since_time("1 hour ago");
        assert!(result.is_ok());
        // Just check it parses correctly - exact time depends on when test runs
    }

    #[test]
    fn test_parse_invalid_time() {
        // Test invalid input
        let result = parse_since_time("invalid time");
        assert!(result.is_err());
    }

    #[test]
    fn test_collect_statistics() {
        use cxms::query::QueryCondition;

        let results = vec![
            SearchResult {
                file: "file1.jsonl".to_string(),
                uuid: "uuid1".to_string(),
                timestamp: "2024-01-01T00:00:00Z".to_string(),
                session_id: "session1".to_string(),
                role: "user".to_string(),
                text: "test message 1".to_string(),
                message_type: "message".to_string(),
                query: QueryCondition::Literal {
                    pattern: "test".to_string(),
                    case_sensitive: false,
                },
                cwd: "/project1".to_string(),
                raw_json: None,
            },
            SearchResult {
                file: "file1.jsonl".to_string(),
                uuid: "uuid2".to_string(),
                timestamp: "2024-01-01T01:00:00Z".to_string(),
                session_id: "session1".to_string(),
                role: "assistant".to_string(),
                text: "test response 1".to_string(),
                message_type: "message".to_string(),
                query: QueryCondition::Literal {
                    pattern: "test".to_string(),
                    case_sensitive: false,
                },
                cwd: "/project1".to_string(),
                raw_json: None,
            },
            SearchResult {
                file: "file2.jsonl".to_string(),
                uuid: "uuid3".to_string(),
                timestamp: "2024-01-02T00:00:00Z".to_string(),
                session_id: "session2".to_string(),
                role: "user".to_string(),
                text: "test message 2".to_string(),
                message_type: "message".to_string(),
                query: QueryCondition::Literal {
                    pattern: "test".to_string(),
                    case_sensitive: false,
                },
                cwd: "/project2".to_string(),
                raw_json: None,
            },
        ];

        let stats = collect_statistics(&results);

        assert_eq!(stats.total_messages, 3);
        assert_eq!(stats.session_count, 2);
        assert_eq!(stats.file_count, 2);
        assert_eq!(stats.project_count, 2);
        assert_eq!(stats.role_counts.get("user"), Some(&2));
        assert_eq!(stats.role_counts.get("assistant"), Some(&1));
        assert_eq!(stats.message_type_counts.get("message"), Some(&3));

        if let Some((earliest, latest)) = &stats.timestamp_range {
            assert_eq!(earliest, "2024-01-01T00:00:00Z");
            assert_eq!(latest, "2024-01-02T00:00:00Z");
        } else {
            panic!("Expected timestamp range to be set");
        }
    }

    #[test]
    fn test_empty_query_detection() {
        // Test that empty string queries are detected correctly
        let empty_string = Some(String::from(""));
        let non_empty_string = Some(String::from("test"));
        let none_query: Option<String> = None;

        // Empty string should be detected as empty
        assert!(empty_string.as_ref().map(|s| s.is_empty()).unwrap_or(false));

        // Non-empty string should not be detected as empty
        assert!(
            !non_empty_string
                .as_ref()
                .map(|s| s.is_empty())
                .unwrap_or(false)
        );

        // None should trigger the first condition (is_none())
        assert!(none_query.is_none());

        // The combined condition used in main should be true for both None and empty string
        assert!(none_query.is_none() || none_query.as_ref().map(|s| s.is_empty()).unwrap_or(false));
        assert!(
            empty_string.is_none() || empty_string.as_ref().map(|s| s.is_empty()).unwrap_or(false)
        );
        assert!(
            !(non_empty_string.is_none()
                || non_empty_string
                    .as_ref()
                    .map(|s| s.is_empty())
                    .unwrap_or(false))
        );
    }

    #[test]
    fn test_cli_latest_conflicts_with_session_id() {
        let parsed = Cli::try_parse_from(["cxms", "--latest", "--session-id", "sid"]);
        assert!(parsed.is_err());
    }

    #[test]
    fn test_cli_latest_session_conflicts_with_session_id() {
        let parsed = Cli::try_parse_from(["cxms", "--latest-session", "--session-id", "sid"]);
        assert!(parsed.is_err());
    }

    #[test]
    fn test_cli_latest_conflicts_with_latest_session() {
        let parsed = Cli::try_parse_from(["cxms", "--latest", "--latest-session"]);
        assert!(parsed.is_err());
    }

    #[test]
    fn test_cli_parses_context_options() {
        let parsed = Cli::try_parse_from([
            "cxms",
            "--codex-home",
            "~/.codex-work2",
            "--around",
            "2",
            "--session-outline",
            "KARTE",
        ])
        .unwrap();

        assert_eq!(parsed.codex_home.as_deref(), Some("~/.codex-work2"));
        assert_eq!(parsed.around, 2);
        assert!(parsed.session_outline);
        assert_eq!(parsed.query.as_deref(), Some("KARTE"));
    }

    #[test]
    fn test_resolve_pattern_prefers_pattern_over_codex_home() {
        let pattern = resolve_pattern(Some("/tmp/custom/*.jsonl"), Some("~/.codex-work2"));

        assert_eq!(pattern, "/tmp/custom/*.jsonl");
    }

    #[test]
    fn test_resolve_pattern_uses_codex_home_sessions() {
        let pattern = resolve_pattern(None, Some("~/.codex-work2"));

        assert_eq!(pattern, "~/.codex-work2/sessions/**/*.jsonl");
    }
}
