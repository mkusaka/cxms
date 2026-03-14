pub mod engine;
pub mod file_discovery;
pub mod rayon_engine;
pub mod smol_engine;

pub use engine::{SearchEngineTrait, format_search_result};
pub use file_discovery::{
    default_claude_pattern, default_codex_pattern, discover_claude_files, discover_codex_files,
    expand_tilde,
};
pub use rayon_engine::RayonEngine;
pub use smol_engine::SmolEngine;
