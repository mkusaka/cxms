pub mod context;
pub mod engine;
pub mod file_discovery;
pub mod rayon_engine;
pub mod smol_engine;

pub use context::{
    AroundSearchResult, ContextMessage, SessionOutline, build_around_results,
    build_session_outlines, codex_home_pattern,
};
pub use engine::{SearchEngineTrait, format_search_result};
pub use file_discovery::{default_codex_pattern, discover_codex_files, expand_tilde};
pub use rayon_engine::RayonEngine;
pub use smol_engine::SmolEngine;
