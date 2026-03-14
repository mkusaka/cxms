# AGENTS.md

This repository uses Codex-oriented agent instructions.

## Core Workflow

```bash
# Build
cargo build
cargo build --release

# Test
cargo test

# Lint and format
cargo fmt
cargo clippy -- -D warnings
```

Run `cargo fmt`, `cargo test`, and `cargo clippy -- -D warnings` before you finish a change.

## Project Focus

- `cxms` is a CLI and TUI for searching Codex rollout JSONL files.
- Default search input is `~/.codex/sessions/**/*.jsonl`.
- The parser still accepts older session JSONL shapes where needed, but new documentation and UX should be Codex-first.

## Architecture Notes

1. `query`
   Parses search syntax and evaluates conditions.
2. `schemas`
   Defines searchable message structures for Codex rollouts and legacy session messages.
3. `search`
   Handles file discovery, parsing, and parallel search execution.
4. `interactive_ratatui`
   Contains the interactive TUI with domain, application, and UI layers.
5. `profiling`
   Holds optional profiling helpers behind the `profiling` feature.
