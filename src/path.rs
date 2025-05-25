use std::path::Path;

use anyhow::{Result, anyhow};

#[derive(Debug, Clone)]
pub struct PathInfo {
    pub path: String,
    pub canonicalized: String,
}

impl PathInfo {
    pub fn new(path: &str, root_dir: &str) -> Result<Self> {
        let base = Path::new(root_dir).join(path);
        let canonicalized = canonicalize(&base).or_else(|_| {
            base.to_str()
                .map(|x| x.to_string())
                .ok_or(anyhow!("cannot convert dependency pattern: '{base:?}'"))
        })?;

        Ok(Self {
            path: path.to_string(),
            canonicalized,
        })
    }
}

fn canonicalize(path: &Path) -> Result<String> {
    let canonicalized = std::fs::canonicalize(path)?;
    let canonical_str = canonicalized
        .to_str()
        .ok_or(anyhow!("cannot convert file path to string"))?;

    Ok(canonical_str.to_owned())
}

#[cfg(test)]
mod tests {
    use super::PathInfo;

    #[test]
    fn new_path_info_non_existing_file() {
        let info = PathInfo::new("dir/does/not/exist", ".");

        assert_eq!(info.is_ok(), true);
        assert_eq!(info.unwrap().canonicalized, "./dir/does/not/exist");
    }

    #[test]
    fn new_path_info_existing_file() {
        let info = PathInfo::new("src/cli.rs", ".");

        assert_eq!(info.is_ok(), true);
        assert_eq!(info.unwrap().path, "src/cli.rs");
    }

    #[test]
    fn new_path_info_existing_file_unknown_root_dir() {
        let info = PathInfo::new("src/cli.rs", "/tmp/some/where");

        assert_eq!(info.is_ok(), true);
        assert_eq!(info.unwrap().canonicalized, "/tmp/some/where/src/cli.rs");
    }

    #[test]
    fn new_path_info_wildcard_path() {
        let info = PathInfo::new("src/*", ".");

        assert_eq!(info.is_ok(), true);
        assert_eq!(info.unwrap().canonicalized, "./src/*");
    }
}
