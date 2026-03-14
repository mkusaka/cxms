/// Check if an actual cwd/path belongs to a specific project path.
pub fn cwd_belongs_to_project(candidate_path: &str, project_path: &str) -> bool {
    let candidate = std::path::Path::new(candidate_path);
    let project = std::path::Path::new(project_path);

    if candidate.as_os_str().is_empty() || project.as_os_str().is_empty() {
        return false;
    }

    if let (Ok(candidate), Ok(project)) = (candidate.canonicalize(), project.canonicalize()) {
        return candidate.starts_with(&project);
    }

    candidate.starts_with(project)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cwd_belongs_to_project() {
        assert!(cwd_belongs_to_project(
            "/Users/me/src/project/subdir",
            "/Users/me/src/project"
        ));
        assert!(!cwd_belongs_to_project(
            "/Users/me/src/other-project",
            "/Users/me/src/project"
        ));
    }
}
