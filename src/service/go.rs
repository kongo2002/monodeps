use std::collections::HashSet;
use std::path::Path;

use anyhow::Result;

use crate::cli::Opts;
use crate::config::{DepPattern, GoDepsConfig};

use super::{non_hidden_files, read_lines};

const SCAN_MAX_LINES: usize = 300;

pub(super) struct GoAnalyzer {}

impl GoAnalyzer {
    pub(super) fn dependencies<P>(&self, dir: P, config: &Opts) -> Result<Vec<DepPattern>>
    where
        P: AsRef<Path>,
    {
        let mut collected_imports = HashSet::new();

        for entry in non_hidden_files(&dir) {
            let filename = entry.file_name().to_str().unwrap_or("").to_lowercase();
            if !filename.ends_with(".go") {
                continue;
            }

            let lines = read_lines(entry.path())?.map_while(Result::ok);

            collected_imports.extend(find_imports(lines, &config.config.auto_discovery.go)?);
        }

        Ok(collected_imports
            .into_iter()
            .flat_map(|import| DepPattern::new(&import, &config.target.canonicalized))
            .collect())
    }
}

fn find_imports<I>(lines: I, config: &GoDepsConfig) -> Result<Vec<String>>
where
    I: IntoIterator<Item = String>,
{
    let mut imports = Vec::new();
    let mut in_imports = false;
    let mut scanned_lines = 0usize;

    for line in lines {
        scanned_lines += 1;
        if scanned_lines > SCAN_MAX_LINES {
            break;
        }

        if in_imports {
            if line.contains(")") {
                in_imports = false;
                continue;
            }

            if let Some(import) = extract_from_line(&line, config) {
                imports.push(import);
            }
        } else if line.starts_with("import (") {
            in_imports = true;
        } else if line.starts_with("import") {
            if let Some(import) = extract_from_line(&line, config) {
                imports.push(import);
            }
        }
    }

    Ok(imports)
}

fn extract_from_line(line: &str, config: &GoDepsConfig) -> Option<String> {
    let parts: Vec<_> = line.splitn(3, "\"").collect();
    if parts.len() != 3 {
        return None;
    }

    let import = parts[1].to_string();
    config
        .package_prefixes
        .iter()
        .flat_map(|module_prefix| {
            if import.starts_with(module_prefix) {
                let stripped = import.trim_start_matches(module_prefix).trim_matches('/');
                Some(stripped)
            } else {
                None
            }
        })
        .next()
        .map(String::from)
}

#[cfg(test)]
mod tests {
    use super::find_imports;

    const GO_IMPORT01: &str = include_str!("../../tests/go_import01.go");
    const GO_IMPORT02: &str = include_str!("../../tests/go_import02.go");

    #[test]
    fn grouped_imports_with_matching_prefix() {
        let imports = find_imports(
            GO_IMPORT01.lines().map(String::from),
            &crate::config::GoDepsConfig {
                package_prefixes: vec![String::from("dev.azure.com/foo/bar")],
            },
        )
        .unwrap();

        assert_eq!(imports, vec!["pkg/some", "pkg/retry"]);
    }

    #[test]
    fn grouped_imports_without_matching_prefix() {
        let imports = find_imports(
            GO_IMPORT01.lines().map(String::from),
            &crate::config::GoDepsConfig {
                package_prefixes: vec![String::from("dev.azure.com/bar/foo")],
            },
        )
        .unwrap();

        assert_eq!(imports.len(), 0);
    }

    #[test]
    fn single_imports_with_matching_prefix() {
        let imports = find_imports(
            GO_IMPORT02.lines().map(String::from),
            &crate::config::GoDepsConfig {
                package_prefixes: vec![String::from("dev.azure.com/foo/bar")],
            },
        )
        .unwrap();

        assert_eq!(imports, vec!["pkg/some", "pkg/retry"]);
    }
}
