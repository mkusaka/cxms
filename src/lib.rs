pub mod interactive_ratatui;
pub mod profiling;
#[cfg(feature = "profiling")]
pub mod profiling_enhanced;
pub mod query;
pub mod schemas;
pub mod search;
pub mod stats;
pub mod utils;

pub use query::{QueryCondition, SearchOptions, SearchResult, parse_query};
pub use schemas::{SearchableMessage, SessionMessage, ToolResult};
pub use search::{
    RayonEngine, SearchEngineTrait, SmolEngine, default_claude_pattern, default_codex_pattern,
    discover_claude_files, discover_codex_files, expand_tilde, format_search_result,
};
pub use stats::{Statistics, format_statistics};
