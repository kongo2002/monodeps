use anyhow::Result;
use std::path::{Path, PathBuf};
use yaml_rust::Yaml;

use crate::config::DepPattern;
use crate::path::PathInfo;
use crate::service::parent_dir;
use crate::utils::{load_yaml, yaml_str_list};

use super::non_hidden_files;

struct Workspace {
    dependencies: Vec<DepPattern>,
}

pub(super) struct FlutterAnalyzer {
    workspace: Option<Workspace>,
}

impl FlutterAnalyzer {
    pub(super) fn new(root: &PathInfo) -> Self {
        let workspace = try_parse_workspace_pubspec(root);

        Self { workspace }
    }

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

            if log::log_enabled!(log::Level::Debug) {
                log::debug!(
                    "flutter: analyzing dart pubspec file '{}'",
                    entry.path().display()
                );
            }

            let yaml = load_yaml(entry.path())?;

            if let Some(workspace) = &self.workspace {
                let is_part_of_workspace = yaml["resolution"]
                    .as_str()
                    .map(|resolution| resolution.eq_ignore_ascii_case("workspace"))
                    .unwrap_or(false);

                // we could also check if the package is _really_ listed in the workspaces
                // but usually that would fail the dependency resolution anyways
                if is_part_of_workspace {
                    dependencies.extend(workspace.dependencies.clone());
                }
            }

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

fn find_local_dependencies(dependencies: &Yaml, pubspec_dir: &PathBuf) -> Option<Vec<DepPattern>> {
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

fn try_parse_workspace_pubspec(root: &PathInfo) -> Option<Workspace> {
    let path = PathInfo::new("pubspec.yaml", &root.canonicalized).ok()?;
    let yaml = load_yaml(&path.canonicalized).ok()?;
    let references = yaml_str_list(&yaml["workspace"]);
    let workspaces: Vec<_> = references
        .into_iter()
        .flat_map(|reference| DepPattern::new(&reference, &root.canonicalized))
        .collect();

    if workspaces.is_empty() {
        None
    } else {
        let yaml = DepPattern::new("pubspec.yaml", &root.canonicalized).ok()?;
        let lockfile = DepPattern::new("pubspec.lock", &root.canonicalized).ok()?;
        let dependencies = vec![yaml, lockfile];

        Some(Workspace { dependencies })
    }
}
