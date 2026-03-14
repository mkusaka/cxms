# CXMS (Codex Message Searcher)

[![CI](https://github.com/mkusaka/cxms/actions/workflows/ci.yml/badge.svg)](https://github.com/mkusaka/cxms/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

High-performance CLI for searching Codex session rollout JSONL files with an interactive TUI mode.

## Features

- 🚀 **Blazing Fast**: SIMD-accelerated JSON parsing with parallel file processing
- 🔍 **Powerful Query Syntax**: Boolean operators (AND/OR/NOT), regex, and quoted literals
- 🎯 **Smart Filtering**: Filter by role, session ID, timestamp ranges, and project paths
- 💻 **Interactive Mode**: fzf-like TUI with Search and Session List tabs
- 📊 **Statistics Mode**: Comprehensive search statistics with `--stats` flag
- 📋 **Session Browser**: Browse and search all sessions with full-text search
- 🎨 **Beautiful Output**: Colored terminal output with match highlighting
- 📄 **Multiple Output Formats**: Text, JSON, or JSONL with customizable formatting
- 🔧 **Robust Testing**: Comprehensive test suite with cargo-nextest support
- 🚀 **Shell Completion**: Auto-completion support for bash, zsh, and fish shells

## Installation

### From GitHub (Recommended)

```bash
# Install directly from GitHub
cargo install --git https://github.com/mkusaka/cxms

# Or install a specific version/tag
cargo install --git https://github.com/mkusaka/cxms --tag v0.0.1
```

### From Source

```bash
# Clone the repository
git clone https://github.com/mkusaka/cxms.git
cd cxms

# Build and install
cargo install --path .
```

### Manual Build

```bash
# Build release version
cargo build --release

# Copy to your PATH
cp target/release/cxms ~/.local/bin/
# or
sudo cp target/release/cxms /usr/local/bin/
```

## Shell Completion

CXMS supports shell completion for bash, zsh, and fish. To enable it:

### Bash

```bash
# Generate completion script to file
cxms --completion bash > ~/.bash_completion.d/cxms

# Or add to .bashrc for persistent completion
echo 'source <(cxms --completion bash)' >> ~/.bashrc

# Or enable completion immediately in current shell
source <(cxms --completion bash)
```

### Zsh

```bash
# Generate completion script to file
cxms --completion zsh > ~/.zsh/completions/_cxms

# Or add to .zshrc for persistent completion
echo 'source <(cxms --completion zsh)' >> ~/.zshrc

# Or enable completion immediately in current shell
source <(cxms --completion zsh)
```

### Fish

```bash
# Generate completion script
cxms --completion fish > ~/.config/fish/completions/cxms.fish
```

After installation, restart your shell or source your configuration file to enable completions.

## Usage

### Basic Search

```bash
# Launch interactive mode (no arguments needed)
cxms

# Search for "error" in all Codex sessions
cxms "error"

# Search in specific files
cxms -p "~/.codex/sessions/**/*.jsonl" "bug"

# Filter by role
cxms -r user "how to"
cxms -r assistant "I can help"

# Filter by current project directory (default behavior)
cxms "TODO"                          # Searches in current directory by default

# Explicitly specify project directory
cxms --project "/path/to/project" "TODO"

# Search all projects (bypass default filter)
cxms --project "/" "TODO"

# Show statistics only (no message content)
cxms --stats ""                      # Stats for all messages
cxms --stats "error"                 # Stats for messages containing "error"
cxms --stats --role user "question"  # Stats with filters
```

### Interactive Mode (TUI)

Launch an interactive search interface similar to fzf. Interactive mode starts automatically when no query is provided:

```bash
# Interactive search (default when no query provided)
cxms

# Interactive search in specific directory
cxms -p "~/my-project/*.jsonl"

# Interactive search with filters
cxms --project $(pwd)                    # Current project only
cxms --since "1 day ago"                  # Recent messages only
cxms -r user                              # Pre-filter by role
cxms --project $(pwd) --since "2 hours ago"  # Combine filters

# All standard filters are supported
cxms -s "session-id"                      # Filter by session
cxms --after "2024-01-01T00:00:00Z"       # Time range filters
cxms -n 100                               # Adjust result limit
```

**Interactive Mode Controls:**
- Type to search in real-time
- `↑/↓` - Navigate results
- `Ctrl+u/d` - Half-page scrolling (up/down) 
- `Enter` - View full message
- `Ctrl+S` - Jump directly to session viewer
- `Tab` - Cycle role filters (all → user → assistant → system → summary)
- `Shift+Tab` - Switch between Search and Session List tabs
- `Ctrl+R` - Clear cache and reload files
- `Ctrl+T` - Toggle message truncation (Truncated/Full Text)
- `Alt+←` - Navigate back through history
- `Alt+→` - Navigate forward through history
- `Ctrl+C (2x)` - Exit (press twice within 1 second)
- `Esc` - Go back to previous screen (does not exit from search screen)

**Session List Tab:**
- View all available sessions in a browsable list
- Real-time search through all messages in all sessions
- Shows session ID, message count, timestamp, and first message preview
- `p` - Toggle first message preview on/off
- `Enter` - Open session in Session Viewer
- Search highlights matching text in yellow

**Note on Filters in Interactive Mode:**
- All command-line filters (`--project`, `--since`, `--after`, `--before`, `-s`, etc.) are applied as base filters
- The `-r` flag sets the initial role filter, but you can still cycle through roles with Tab
- Filters persist throughout the interactive session
- Results are loaded incrementally - initially 100 items, then automatically loads more as you scroll
- Interactive mode ignores the `-n` flag to allow viewing all available results

**Result Actions:**
- `Enter` - View message details
- `Ctrl+S` - Jump directly to session viewer
- `Tab` - Toggle role filter (all → user → assistant → system)
- `Ctrl+O` - Toggle sort order (newest/oldest first)
- `Ctrl+T` - Toggle message truncation

**Message Detail & Session Viewer Copy Operations (Unified):**
- `c` - Copy content/text
- `C` - Copy as JSON
- `i` - Copy session ID
- `f` - Copy file path  
- `p` - Copy project path

**Session Viewer Controls:**
- `↑/↓` or `Ctrl+P/N` - Navigate messages
- `Ctrl+U/D` - Half-page scrolling (up/down)
- `Tab` - Cycle role filters (all → user → assistant → system)
- `/` - Search within session (Tab works in search mode too)
- `Ctrl+O` - Toggle sort order
- `Enter` - View message detail
- `Esc` - Return to previous screen

### Advanced Queries

```bash
# AND operator
cxms "error AND connection"

# OR operator
cxms "warning OR error"

# NOT operator
cxms "response NOT error"

# Complex queries with parentheses
cxms "(error OR warning) AND NOT /test/i"

# Regular expressions
cxms "/failed.*connection/i"
cxms "/^Error:.*\d+/m"
```

### Filtering Options

```bash
# Limit results
cxms -n 100 "search term"

# Filter by session ID
cxms -s "session-123" "query"

# Filter by timestamp
cxms --after "2024-01-01T00:00:00Z" "recent"
cxms --before "2024-12-31T23:59:59Z" "old"

# Filter using relative time or Unix timestamp
cxms --since "1 day ago" "recent activity"
cxms --since "2 hours ago" "latest changes"
cxms --since "yesterday" "yesterday's work"
cxms --since "last week" "weekly review"
cxms --since "3 days ago" "recent work"
cxms --since 1720000000 "since Unix timestamp"

# Filter by project path (defaults to current directory if not specified)
cxms --project "/Users/me/project" "bug"

# Search all projects (bypass default current directory filter)
cxms --project "/" "bug"

# Combine filters
cxms -r user -n 20 --after "2024-06-01T00:00:00Z" "question"
```

### Output Formats

```bash
# Default text output with colors
cxms "query"

# Disable colors
cxms --no-color "query"

# Show full message text
cxms --full-text "query"

# Show raw JSON of matched messages
cxms --raw "query"

# JSON output with detailed statistics
cxms -f json "query" > results.json

# JSONL output (one JSON per line)
cxms -f jsonl "query" > results.jsonl

# Verbose output with debug info
cxms -v "query"
```

#### JSON Output Format

The JSON output format provides rich metadata about search results:

```bash
# Get detailed JSON output with session and file statistics
cxms -f json "error" --project "/" -n 100

# Extract summary information
cxms -f json "query" | jq '.summary'

# List all unique sessions with message counts
cxms -f json "query" | jq -r '.sessions[] | "\(.session_id): \(.message_count) messages"'

# List all unique files with message counts
cxms -f json "query" | jq -r '.files[] | "\(.message_count) messages: \(.path)"'
```

JSON output structure includes:
- `results`: Array of search results with full message details
- `summary`: Search statistics including duration, total/returned counts, unique sessions/files
- `sessions`: List of unique sessions with message counts
- `files`: List of unique files with message counts and associated session IDs

## CLI Options

### General Options
- `-p, --pattern <PATTERN>` - File pattern to search (default: `~/.codex/sessions/**/*.jsonl`)
- `-n, --max-results <N>` - Maximum number of results to return (default: 200)
- `-f, --format <FORMAT>` - Output format: `text`, `json`, or `jsonl` (default: text)
- `-v, --verbose` - Enable verbose output
- `--no-color` - Disable colored output
- `--full-text` - Show full message text without truncation
- `--raw` - Show raw JSON of matched messages
- `--stats` - Show only statistics without message content

### Filtering Options
- `-r, --role <ROLE>` - Filter by message role: `user`, `assistant`, `system`, or `summary`
- `-s, --session-id <ID>` - Filter by session ID
- `--project <PATH>` - Filter by project path (default: current directory; use `/` to search all projects)
- `--before <TIMESTAMP>` - Filter messages before this timestamp (RFC3339 format)
- `--after <TIMESTAMP>` - Filter messages after this timestamp (RFC3339 format)
- `--since <TIME>` - Filter messages since this time (relative time like "1 day ago" or Unix timestamp)

### Interactive Mode
- `-i, --interactive` - Launch interactive search mode (fzf-like TUI)
- **Note**: Interactive mode starts automatically when no query is provided

### Other Options
- `--help-query` - Show query syntax help
- `--completion <SHELL>` - Generate shell completion script for bash, zsh, or fish
- `--profile <NAME>` - Generate profiling report (requires --features profiling)
- `-h, --help` - Print help information
- `-V, --version` - Print version information

## Query Syntax Reference

### Basic Queries
- `hello` - Case-insensitive literal search
- `"hello world"` - Quoted literal (preserves spaces)
- `'hello world'` - Single-quoted literal
- `/pattern/flags` - Regular expression with optional flags

### Operators
- `AND` - Both terms must be present
- `OR` - Either term must be present  
- `NOT` - Term must not be present
- `()` - Grouping for complex expressions

### Regex Flags
- `i` - Case insensitive
- `m` - Multi-line mode
- `s` - Dot matches newline

### Query Examples
```bash
# Find errors in connection handling
error AND /failed.*connection/i

# Find user messages excluding tests
"user message" AND NOT test

# Find warnings or errors with timestamps
(warning OR error) AND timestamp

# Find specific error patterns
/^Error:.*\d+/m

# Complex nested query
(("connection failed" OR "timeout") AND error) NOT debug
```

### Statistics Mode

The `--stats` flag displays comprehensive statistics about search results:

```bash
# Statistics for all messages
cxms --stats ""

# Statistics for error messages
cxms --stats "error"

# Statistics with filters
cxms --stats --role assistant --since "1 week ago" "code"
```

Output includes:
- Total message count
- Messages by role with percentages
- Unique sessions, files, and projects
- Message type breakdown
- Time range (earliest to latest)
- Search execution time

## Development

### Prerequisites

- Rust 1.75 or later
- cargo-nextest (for enhanced testing)
- clippy (for linting)

### Setup

```bash
# Clone the repository
git clone https://github.com/mkusaka/ccmeta.git
cd ccmeta/schema/cxms

# Install development tools
cargo install cargo-nextest --locked
rustup component add clippy

# Build the project
cargo build

# Run tests
cargo nextest run

# Run clippy (checks code quality and style)
cargo clippy -- -D warnings
```

### Running Tests

**Note on Clipboard Tests:** Some tests that verify clipboard functionality are marked with `#[ignore]` as they require clipboard utilities (`pbcopy` on macOS, `xclip`/`xsel` on Linux) that may not be available in CI environments. To run these tests locally:

```bash
# Run all tests including ignored ones
cargo test -- --ignored

# Run only the clipboard tests
cargo test clipboard -- --ignored
```

```bash
# Run all tests with cargo-nextest
cargo nextest run

# Run specific test
cargo nextest run test_name

# Run tests with standard cargo
cargo test

# Run tests with output
cargo test -- --nocapture
```

### Benchmarking

```bash
# Run benchmarks
cargo bench

# Run specific benchmark
cargo bench search_benchmark

# Profile with flamegraph (requires profiling feature)
cargo run --release --features profiling -- --profile baseline "query"
```

### Project Structure

```
cxms/
├── src/
│   ├── main.rs                    # CLI entry point
│   ├── lib.rs                     # Library exports
│   ├── interactive_ratatui/       # Interactive TUI mode (Clean Architecture)
│   │   ├── mod.rs                 # Main event loop
│   │   ├── domain/                # Domain layer (models, business rules)
│   │   ├── application/           # Application layer (services)
│   │   └── ui/                    # UI layer (MVU pattern, components)
│   ├── query/                     # Query parsing and evaluation
│   │   ├── parser.rs              # Nom-based query parser
│   │   └── condition.rs           # Query condition types
│   ├── schemas/                   # Claude message schemas
│   │   ├── session_message.rs
│   │   └── tool_result.rs
│   ├── search/                    # Search engine implementation
│   │   ├── engine.rs              # Core search logic
│   │   ├── file_discovery.rs
│   │   └── async_engine.rs
│   ├── stats.rs                   # Statistics collection and formatting
│   └── profiling.rs               # Performance profiling
├── benches/                       # Benchmarks
├── tests/                         # Integration tests
├── CLAUDE.md                      # Guidance for Claude Code
├── spec.md                        # Detailed interactive mode specification
└── PERFORMANCE.md                 # Performance characteristics and benchmarks
```

## Performance

This tool is optimized for maximum performance:

- **SIMD JSON Parsing**: Uses sonic-rs for hardware-accelerated parsing
- **Parallel Processing**: Leverages all CPU cores with Rayon
- **Zero-Copy Design**: Minimizes allocations and string copies
- **Smart Filtering**: Early termination and efficient predicate evaluation
- **Memory-Mapped I/O**: Efficient handling of large files

## Configuration

### Default Search Location

By default, searches in `~/.codex/sessions/**/*.jsonl`

### Custom Patterns

```bash
# Search in specific project
cxms -p "~/.codex/sessions/**/*.jsonl" "query"

# Search in current directory
cxms -p "$(pwd)/**/*.jsonl" "query"

# Search single file
cxms -p "/path/to/specific/session.jsonl" "query"
```

## Contributing

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Run tests and ensure they pass (`cargo nextest run`)
4. Run clippy and fix any warnings (`cargo clippy -- -D warnings`)
5. Commit your changes (`git commit -m 'Add amazing feature'`)
6. Push to the branch (`git push origin feature/amazing-feature`)
7. Open a Pull Request

### Code Style

- Follow Rust standard style guidelines
- Use `cargo fmt` before committing
- Ensure `cargo clippy` passes with no warnings
- Add tests for new functionality
- Update documentation as needed

## Troubleshooting

### No results found
- Check file permissions on Claude session files
- Verify the search pattern matches existing files
- Use `-v` flag for verbose output to debug file discovery

### Performance issues
- Use `-n` to limit results for large datasets
- Consider using more specific search patterns
- Enable profiling with `--features profiling` to identify bottlenecks

### Interactive mode issues
- Ensure terminal supports ANSI escape codes
- Check that required clipboard utilities are installed (pbcopy/xclip/xsel)
- Try running with `--no-color` if display issues occur

## License

MIT License - see [LICENSE](LICENSE) file for details

## Acknowledgments

- Built with [nom](https://github.com/rust-bakery/nom) for parsing
- Uses [sonic-rs](https://github.com/cloudwego/sonic-rs) for fast JSON parsing
- Parallel processing powered by [rayon](https://github.com/rayon-rs/rayon)
- Interactive UI built with [ratatui](https://github.com/ratatui-org/ratatui) and [crossterm](https://github.com/crossterm-rs/crossterm)
