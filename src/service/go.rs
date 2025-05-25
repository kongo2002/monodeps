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

            let parts: Vec<_> = line.splitn(3, "\"").collect();
            if parts.len() != 3 {
                continue;
            }

            let import = parts[1].to_string();
            if let Some(found) = config
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
            {
                imports.push(found.to_string());
            }
        } else {
            in_imports = line.starts_with("import (");
        }
    }

    Ok(imports)
}

fn read_lines<P>(filename: P) -> Result<Lines<BufReader<File>>>
where
    P: AsRef<Path>,
{
    let file = File::open(filename)?;
    Ok(BufReader::new(file).lines())
}
