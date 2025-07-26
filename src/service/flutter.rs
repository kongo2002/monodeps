use anyhow::Result;
use std::path::Path;
use yaml_rust::Yaml;

use crate::config::DepPattern;
use crate::service::parent_dir;
use crate::utils::load_yaml;

use super::non_hidden_files;

pub(super) struct FlutterAnalyzer {}

impl FlutterAnalyzer {
    pub(super) fn dependencies<P>(&self, dir: P) -> Result<Vec<DepPattern>>
    where
        P: AsRef<Path>,
    {
        let mut dependencies = Vec::new();

        for entry in non_hidden_files(&dir) {
            if !entry.file_name().eq_ignore_ascii_case("pubspec.yaml") {
                continue;
            }

            let pubspec_dir = match parent_dir(entry.path()) {
                Some(d) => d,
                None => continue,
            };

            log::debug!(
                "flutter: analyzing dart pubspec file '{}'",
                entry.path().display()
            );

            let yaml = load_yaml(entry.path())?;

            // regular lib dependencies
            dependencies.extend(
                find_local_dependencies(&yaml["dependencies"], &pubspec_dir).unwrap_or_default(),
            );

            // development dependencies
            dependencies.extend(
                find_local_dependencies(&yaml["dev_dependencies"], &pubspec_dir)
                    .unwrap_or_default(),
            );
        }

        Ok(dependencies)
    }
}

fn find_local_dependencies(dependencies: &Yaml, pubspec_dir: &str) -> Option<Vec<DepPattern>> {
    let mut collected = Vec::new();
    let vs = dependencies.as_hash()?;

    for (_, value) in vs.iter() {
        if let Some(dep) = value["path"]
            .as_str()
            .and_then(|path| DepPattern::new(path, pubspec_dir).ok())
        {
            collected.push(dep);
        }
    }

    Some(collected)
}
