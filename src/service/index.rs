use std::path::{Path, PathBuf};

use super::non_hidden_files;

/// All files of a directory tree that passed a name filter, collected in a single walk.
pub(super) struct RepoIndex {
    // sorted by path, so all files below a directory form a contiguous range
    files: Vec<IndexedFile>,
}

pub(super) struct IndexedFile {
    pub path: PathBuf,
    /// lowercased file name
    pub name: String,
}

impl RepoIndex {
    /// Walk `root` and keep every non-directory entry whose file name satisfies `keep`.
    pub fn build<P, F>(root: P, keep: F) -> Self
    where
        P: AsRef<Path>,
        F: Fn(&str) -> bool,
    {
        let mut files: Vec<_> = non_hidden_files(root)
            .into_iter()
            .filter(|entry| !entry.file_type().is_dir())
            .filter_map(|entry| {
                let name = entry.file_name().to_str()?;
                if !keep(name) {
                    return None;
                }
                let name = name.to_lowercase();
                Some(IndexedFile {
                    path: entry.into_path(),
                    name,
                })
            })
            .collect();

        files.sort_unstable_by(|a, b| a.path.cmp(&b.path));

        Self { files }
    }

    pub fn files(&self) -> &[IndexedFile] {
        &self.files
    }

    /// All indexed files below `dir`, including those of nested directories.
    pub fn within<P>(&self, dir: P) -> &[IndexedFile]
    where
        P: AsRef<Path>,
    {
        let dir = dir.as_ref();
        // `Path` compares by components, so `a/b/c` sorts between `a/b` and `a/b-c`
        let start = self.files.partition_point(|f| f.path.as_path() < dir);
        let len = self.files[start..].partition_point(|f| f.path.starts_with(dir));

        &self.files[start..start + len]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(paths: &[&str]) -> RepoIndex {
        let mut files: Vec<_> = paths
            .iter()
            .map(|p| IndexedFile {
                path: PathBuf::from(p),
                name: String::new(),
            })
            .collect();
        files.sort_unstable_by(|a, b| a.path.cmp(&b.path));
        RepoIndex { files }
    }

    fn within(idx: &RepoIndex, dir: &str) -> Vec<String> {
        idx.within(dir)
            .iter()
            .map(|f| f.path.display().to_string())
            .collect()
    }

    #[test]
    fn within_excludes_sibling_with_common_prefix() {
        let idx = index(&["/r/a/x", "/r/a-b/y", "/r/a/sub/z", "/r/ab", "/r/b/w"]);

        assert_eq!(vec!["/r/a/sub/z", "/r/a/x"], within(&idx, "/r/a"));
        assert_eq!(vec!["/r/a-b/y"], within(&idx, "/r/a-b"));
        assert!(within(&idx, "/r/c").is_empty());
    }
}
