use std::path::Path;

use anyhow::{Result, anyhow};
use path_clean::PathClean;

#[derive(Debug, Clone)]
pub struct PathInfo {
    pub path: String,
    pub canonicalized: String,
}

impl PathInfo {
    pub fn new<P>(path: &str, root_dir: P) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        let base = Path::new(root_dir.as_ref()).join(path);
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
    let canonicalized = std::path::absolute(path)?;
    let cleaned = canonicalized.clean();
    let canonical_str = cleaned
        .to_str()
        .ok_or(anyhow!("cannot convert file path to string"))?;

    Ok(canonical_str.to_owned())
}

#[cfg(test)]
mod tests {
    use super::PathInfo;

    fn absolute(path: &str) -> String {
        std::path::absolute(path)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn new_path_info_non_existing_file() {
        let info = PathInfo::new("dir/does/not/exist", ".");

        assert_eq!(info.is_ok(), true);
        assert_eq!(
            info.unwrap().canonicalized,
            absolute("./dir/does/not/exist")
        );
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
        assert_eq!(info.unwrap().canonicalized, absolute("./src/*"));
    }
}
