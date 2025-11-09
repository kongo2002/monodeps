use std::path::Path;

use anyhow::{Result, anyhow};
use path_clean::PathClean;

#[derive(Debug, Clone)]
pub struct PathInfo {
    pub display_path: String,
    pub canonicalized: String,
}

impl PathInfo {
    pub fn new<P, R>(path: P, root_dir: R) -> Result<Self>
    where
        P: AsRef<Path>,
        R: AsRef<Path>,
    {
        let base = Path::new(root_dir.as_ref()).join(&path);
        let canonicalized = canonicalize(&base).or_else(|_| {
            base.to_str().map(|x| x.to_string()).ok_or(anyhow!(
                "cannot convert dependency pattern: '{}'",
                base.display()
            ))
        })?;

        let display_path = path.as_ref().to_string_lossy().into_owned();

        Ok(Self {
            display_path,
            canonicalized,
        })
    }

    pub fn relative_to(&self, origin: &PathInfo) -> String {
        let canonicalized = Path::new(&self.canonicalized);

        canonicalized
            .strip_prefix(&origin.canonicalized)
            .map(|stripped| format!("./{}", stripped.display()))
            .unwrap_or_else(|_| self.canonicalized.clone())
    }
}

pub fn canonicalize(path: &Path) -> Result<String> {
    let canonicalized = std::path::absolute(path)?;
    let cleaned = canonicalized.clean();
    let canonical_str = cleaned
        .to_str()
        .ok_or_else(|| anyhow!("cannot convert file path to string '{}'", path.display()))?;

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
        assert_eq!(info.unwrap().display_path, "src/cli.rs");
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
