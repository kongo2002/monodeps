use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader, Lines};
use std::path::Path;

use anyhow::Result;
use walkdir::WalkDir;

use crate::cli::Opts;
use crate::config::{DepPattern, GoDepsConfig};

pub(super) fn dependencies<P>(dir: P, config: &Opts) -> Result<Vec<DepPattern>>
where
    P: AsRef<Path>,
{
    let walker = WalkDir::new(dir)
        .into_iter()
        // filter hidden files/directories
        .filter_entry(|e| {
            !e.file_name()
                .to_str()
                .map(|s| s.starts_with("."))
                .unwrap_or(false)
        })
        // skip errors (e.g. non permission directories)
        .filter_map(|e| e.ok());

    let mut collected_imports = HashSet::new();

    for entry in walker {
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

fn find_imports<I>(lines: I, config: &GoDepsConfig) -> Result<Vec<String>>
where
    I: IntoIterator<Item = String>,
{
    let mut imports = Vec::new();
    let mut in_imports = false;

    for line in lines {
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

fn read_lines<P>(filename: P) -> Result<Lines<BufReader<File>>>
where
    P: AsRef<Path>,
{
    let file = File::open(filename)?;
    Ok(BufReader::new(file).lines())
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
