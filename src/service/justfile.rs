use std::path::{Path, PathBuf};

use anyhow::Result;
use walkdir::DirEntry;

use crate::cli::Opts;
use crate::config::DepPattern;
use crate::service::ReferenceFinder;

use super::LanguageAnalyzer;

pub(super) struct JustfileAnalyzer {}

impl LanguageAnalyzer for JustfileAnalyzer {
    fn file_relevant(&self, file_name: &str) -> bool {
        file_name == "justfile" || file_name.ends_with(".just")
    }

    fn dependencies(
        &self,
        entries: Vec<DirEntry>,
        _dir: &str,
        _opts: &Opts,
    ) -> Result<Vec<DepPattern>> {
        let mut dependencies = Vec::new();

        for entry in entries {
            dependencies.extend(extract_imports(entry.path())?);
        }

        Ok(dependencies)
    }
}

fn extract_imports<P>(path: P) -> Result<Vec<DepPattern>>
where
    P: AsRef<Path>,
{
    let mut finder = ReferenceFinder::new();

    finder.extract_from(path, &|line, parent_dir| {
        extract_from_line(&line, parent_dir)
    })
}

fn extract_from_line(line: &str, dir: &Path) -> Option<DepPattern> {
    if line.starts_with("import") {
        extract_from_import(line, dir)
    } else if line.starts_with("mod") {
        extract_from_submodule(line, dir)
    } else {
        None
    }
}

fn extract_from_submodule(line: &str, dir: &Path) -> Option<DepPattern> {
    // first we are looking for a named submodule like `mod something '../some.just'`
    let parts: Vec<_> = line.splitn(3, "'").collect();
    if parts.len() == 3 {
        DepPattern::plain(parts[1], dir).ok()
    } else {
        // afterwards we are looking for the submodule shorthand `mod foobar`
        let mut words: Vec<_> = line.split(" ").collect();
        if words.len() == 2 {
            let module_name = words.remove(1);

            // these are all the justfile variants the submodule could refer to
            // we pick the first one that exists
            let candidates = [
                format!("./{module_name}.just"),
                format!("./{module_name}/mod.just"),
                format!("./{module_name}/justfile"),
                format!("./{module_name}/.justfile"),
            ];
            candidates
                .iter()
                .flat_map(|justfile| {
                    let path = PathBuf::from(dir).join(justfile);
                    if path.is_file() {
                        DepPattern::plain(path, dir).ok()
                    } else {
                        None
                    }
                })
                .next()
        } else {
            None
        }
    }
}

fn extract_from_import(line: &str, dir: &Path) -> Option<DepPattern> {
    let parts: Vec<_> = line.splitn(3, "'").collect();
    if parts.len() != 3 {
        return None;
    }

    DepPattern::plain(parts[1], dir).ok()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::service::justfile::extract_from_line;

    fn extract(line: &str, dir: &Path) -> Option<String> {
        let pattern = extract_from_line(line, dir)?;
        let hash = pattern.hash()?;

        Some(hash.to_string())
    }

    #[test]
    fn no_match_invalid() {
        let dir = Path::new("/tmp");
        assert_eq!(None, extract("whatever this is", &dir));
    }

    #[test]
    fn no_match_import_not_beginning() {
        let dir = Path::new("/tmp");
        assert_eq!(None, extract("whatever this import is", &dir));
    }

    #[test]
    fn no_match_import_no_content() {
        let dir = Path::new("/tmp");
        assert_eq!(None, extract("import '", &dir));
    }

    #[test]
    fn match_some_import() {
        let dir = Path::new("/tmp/some/where");
        assert_eq!(
            Some("/tmp/some/usr/share/justfile".to_string()),
            extract("import '../usr/share/justfile'", &dir)
        );
    }
}
