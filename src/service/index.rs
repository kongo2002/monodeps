use std::path::{MAIN_SEPARATOR_STR, Path, PathBuf};
use std::sync::mpsc;

use ignore::{WalkBuilder, WalkState};

/// All files of a directory tree that passed a name filter, collected in a single walk.
pub(super) struct RepoIndex {
    // sorted by path bytes, so all files sharing a directory prefix form a contiguous range
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
        let mut files: Vec<_> = non_ignored_files(root)
            .into_iter()
            .filter_map(|path| {
                let name = path.file_name()?.to_str()?;
                if !keep(name) {
                    return None;
                }
                let name = name.to_lowercase();
                Some(IndexedFile { path, name })
            })
            .collect();

        // byte-wise comparison is about 10x faster than the component-wise `Path` ordering
        files.sort_unstable_by(|a, b| bytes(&a.path).cmp(bytes(&b.path)));

        Self { files }
    }

    pub fn files(&self) -> &[IndexedFile] {
        &self.files
    }

    /// All indexed files whose file name equals `file_name` (case-sensitive).
    pub fn named<'a>(&'a self, file_name: &'a str) -> impl Iterator<Item = &'a Path> {
        self.files
            .iter()
            .map(|f| f.path.as_path())
            .filter(move |path| path.file_name().is_some_and(|name| name == file_name))
    }

    /// All indexed files below `dir`, including those of nested directories.
    pub fn within<P>(&self, dir: P) -> &[IndexedFile]
    where
        P: AsRef<Path>,
    {
        // the trailing separator prevents `a/b` from matching files of its sibling `a/b-c`
        let mut prefix = bytes(dir.as_ref()).to_vec();
        if !prefix.ends_with(MAIN_SEPARATOR_STR.as_bytes()) {
            prefix.extend_from_slice(MAIN_SEPARATOR_STR.as_bytes());
        }

        let start = self
            .files
            .partition_point(|f| bytes(&f.path) < prefix.as_slice());
        let len = self.files[start..].partition_point(|f| bytes(&f.path).starts_with(&prefix));

        &self.files[start..start + len]
    }
}

fn bytes(path: &Path) -> &[u8] {
    path.as_os_str().as_encoded_bytes()
}

/// Collect all non-directory paths below `dir` that are neither hidden nor excluded by
/// `.gitignore` or `.ignore` files.
fn non_ignored_files<P>(dir: P) -> Vec<PathBuf>
where
    P: AsRef<Path>,
{
    let (tx, rx) = mpsc::channel();

    WalkBuilder::new(dir)
        // results must not depend on the local machine's git configuration
        .git_global(false)
        .git_exclude(false)
        .build_parallel()
        .run(|| {
            let tx = tx.clone();
            Box::new(move |entry| {
                // skip errors (e.g. non permission directories)
                if let Ok(entry) = entry
                    && entry.file_type().is_some_and(|t| !t.is_dir())
                {
                    _ = tx.send(entry.into_path());
                }
                WalkState::Continue
            })
        });

    drop(tx);
    rx.into_iter().collect()
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
        files.sort_unstable_by(|a, b| bytes(&a.path).cmp(bytes(&b.path)));
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

    #[test]
    fn build_skips_hidden_and_gitignored_files() -> std::io::Result<()> {
        let dir = tempfile::Builder::default().prefix("mdtest").tempdir()?;
        let root = dir.path();
        for sub in [".git", ".hidden", "ignored", "kept"] {
            std::fs::create_dir(root.join(sub))?;
        }
        std::fs::write(root.join(".gitignore"), "ignored/\n")?;
        for file in [".hidden/Depsfile", "ignored/Depsfile", "kept/Depsfile"] {
            std::fs::write(root.join(file), "")?;
        }

        let idx = RepoIndex::build(root, |_| true);
        let files: Vec<_> = idx.files().iter().map(|f| f.path.clone()).collect();

        assert_eq!(vec![root.join("kept/Depsfile")], files);
        Ok(())
    }
}
