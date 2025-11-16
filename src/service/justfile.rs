use std::collections::HashSet;
use std::path::Path;

use anyhow::{Result, anyhow};
use walkdir::DirEntry;

use crate::cli::Opts;
use crate::config::DepPattern;

use super::{LanguageAnalyzer, read_lines};

const SCAN_MAX_LINES: usize = 200;

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
        let mut found = HashSet::new();

        for entry in entries {
            dependencies.extend(extract_imports(entry.path(), &mut found)?);
        }

        Ok(dependencies)
    }
}

fn extract_imports<P>(path: P, found: &mut HashSet<String>) -> Result<Vec<DepPattern>>
where
    P: AsRef<Path>,
{
    let mut scanned_lines = 0usize;
    let mut imports = Vec::new();

    let self_path = path
        .as_ref()
        .to_str()
        .ok_or_else(|| anyhow!("cannot determine justfile path"))?
        .to_string();

    // check for cyclic dependencies
    if !found.insert(self_path) {
        return Ok(imports);
    }

    let parent = path
        .as_ref()
        .parent()
        .ok_or_else(|| anyhow!("cannot determine parent directory"))?;

    // ignore non-existing imports
    if !path.as_ref().is_file() {
        return Ok(imports);
    }

    let lines = read_lines(&path)?.map_while(Result::ok);

    for line in lines {
        scanned_lines += 1;
        if scanned_lines > SCAN_MAX_LINES {
            break;
        }

        if let Some(import) = extract_from_line(&line, parent) {
            imports.extend(extract_imports(&import, found)?);
            imports.push(import);
        }
    }

    Ok(imports)
}

fn extract_from_line(line: &str, dir: &Path) -> Option<DepPattern> {
    if !line.starts_with("import") {
        return None;
    }

    let parts: Vec<_> = line.splitn(3, "'").collect();
    if parts.len() != 3 {
        return None;
    }

    DepPattern::new(parts[1], dir).ok()
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
