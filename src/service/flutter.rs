use anyhow::Result;
use std::path::PathBuf;
use walkdir::DirEntry;
use yaml_rust::Yaml;

use crate::cli::Opts;
use crate::config::DepPattern;
use crate::path::PathInfo;
use crate::service::parent_dir;
use crate::utils::{load_yaml, yaml_str_list};

use super::LanguageAnalyzer;

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
}

impl LanguageAnalyzer for FlutterAnalyzer {
    fn file_relevant(&self, file_name: &str) -> bool {
        file_name == "pubspec.yaml"
    }

    fn dependencies(
        &self,
        entries: Vec<DirEntry>,
        _dir: &str,
        _opts: &Opts,
    ) -> Result<Vec<DepPattern>> {
        let mut dependencies = Vec::new();

        for entry in entries {
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

            // fonts
            dependencies.extend(find_fonts(&yaml["fonts"], &pubspec_dir).unwrap_or_default());

            // assets
            dependencies
                .extend(find_assets(&yaml["flutter"]["assets"], &pubspec_dir).unwrap_or_default());
        }

        Ok(dependencies)
    }
}

fn find_assets(assets: &Yaml, pubspec_dir: &PathBuf) -> Option<Vec<DepPattern>> {
    Some(
        assets
            .as_vec()?
            .iter()
            .flat_map(|asset| DepPattern::plain(asset.as_str()?, pubspec_dir).ok())
            .collect(),
    )
}

fn find_fonts(fonts: &Yaml, pubspec_dir: &PathBuf) -> Option<Vec<DepPattern>> {
    let mut assets = Vec::new();
    let font_families = fonts.as_vec()?;

    for family in font_families {
        let family_fonts = family["fonts"].as_vec()?;

        for font_path in family_fonts {
            let asset = font_path["asset"].as_str()?;
            assets.push(asset);
        }
    }

    Some(
        assets
            .into_iter()
            .flat_map(|asset| DepPattern::plain(asset, pubspec_dir).ok())
            .collect(),
    )
}

fn find_local_dependencies(dependencies: &Yaml, pubspec_dir: &PathBuf) -> Option<Vec<DepPattern>> {
    let mut collected = Vec::new();
    let vs = dependencies.as_hash()?;

    for (_, value) in vs.iter() {
        if let Some(dep) = value["path"]
            .as_str()
            .and_then(|path| DepPattern::plain(path, pubspec_dir).ok())
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
        .flat_map(|reference| DepPattern::plain(&reference, &root.canonicalized))
        .collect();

    if workspaces.is_empty() {
        None
    } else {
        let yaml = DepPattern::plain("pubspec.yaml", &root.canonicalized).ok()?;
        let lockfile = DepPattern::plain("pubspec.lock", &root.canonicalized).ok()?;
        let dependencies = vec![yaml, lockfile];

        Some(Workspace { dependencies })
    }
}
