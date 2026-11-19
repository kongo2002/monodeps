use anyhow::Result;
use std::path::{Path, PathBuf};
use yaml_rust2::Yaml;

use crate::cli::Opts;
use crate::config::DepPattern;
use crate::path::PathInfo;
use crate::service::{ReferenceFinder, parent_dir};
use crate::utils::{load_yaml, yaml_str_list};

use super::{LanguageAnalyzer, nearest_ancestor};

struct Workspace {
    dependencies: Vec<DepPattern>,
}

pub(super) struct FlutterAnalyzer {
    workspace: Option<Workspace>,
    root_dir: PathBuf,
}

impl FlutterAnalyzer {
    pub(super) fn new(root: &PathInfo) -> Self {
        let workspace = try_parse_workspace_pubspec(root);
        let root_dir = PathBuf::from(&root.canonicalized);

        Self {
            workspace,
            root_dir,
        }
    }

    fn analyze_pubspec(
        &self,
        yaml: &Yaml,
        pubspec_dir: &PathBuf,
        git_repositories: &[String],
    ) -> Vec<DepPattern> {
        let mut dependencies = Vec::new();

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
            find_local_dependencies(
                &yaml["dependencies"],
                pubspec_dir,
                &self.root_dir,
                git_repositories,
            )
            .unwrap_or_default(),
        );

        // development dependencies
        dependencies.extend(
            find_local_dependencies(
                &yaml["dev_dependencies"],
                pubspec_dir,
                &self.root_dir,
                git_repositories,
            )
            .unwrap_or_default(),
        );

        // dependency overrides
        dependencies.extend(
            find_local_dependencies(
                &yaml["dependency_overrides"],
                pubspec_dir,
                &self.root_dir,
                git_repositories,
            )
            .unwrap_or_default(),
        );

        // fonts
        dependencies.extend(find_fonts(&yaml["fonts"], pubspec_dir).unwrap_or_default());

        // assets
        dependencies
            .extend(find_assets(&yaml["flutter"]["assets"], pubspec_dir).unwrap_or_default());

        dependencies
    }
}

impl LanguageAnalyzer for FlutterAnalyzer {
    fn file_relevant(&self, file_name: &str) -> bool {
        file_name == "pubspec.yaml"
    }

    fn dependencies(&self, files: &[&Path], dir: &str, opts: &Opts) -> Result<Vec<DepPattern>> {
        let mut dependencies = Vec::new();

        for file in files {
            let pubspec_dir = match parent_dir(file) {
                Some(d) => d,
                None => continue,
            };

            if log::log_enabled!(log::Level::Debug) {
                log::debug!("flutter: analyzing dart pubspec file '{}'", file.display());
            }

            let yaml = load_yaml(*file)?;
            dependencies.extend(self.analyze_pubspec(
                &yaml,
                &pubspec_dir,
                &opts.config.auto_discovery.flutter.git_repositories,
            ));
        }

        // aside from analysis
        if let Some(analysis_options) = nearest_ancestor("analysis_options.yaml", dir, &opts.target)
        {
            if log::log_enabled!(log::Level::Debug) {
                log::debug!(
                    "flutter: analyzing dart analysis options file '{}'",
                    analysis_options.display()
                );
            }

            let mut finder = ReferenceFinder::new();
            dependencies.extend(finder.extract_from(analysis_options, &analyze_analysis_options)?);
        }

        Ok(dependencies)
    }
}

fn analyze_analysis_options(path: &Path, dir: &Path) -> Result<Vec<DepPattern>> {
    let mut found = Vec::new();
    let yaml = load_yaml(path)?;
    let include = &yaml["include"];

    let includes = if let Some(str) = include.as_str() {
        vec![str.to_string()]
    } else {
        yaml_str_list(include)
    };

    for include in includes {
        if include.starts_with("package:") {
            continue;
        }

        found.push(DepPattern::plain(&include, dir)?);
    }

    Ok(found)
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

fn find_local_dependencies(
    dependencies: &Yaml,
    pubspec_dir: &PathBuf,
    root_dir: &PathBuf,
    git_repositories: &[String],
) -> Option<Vec<DepPattern>> {
    let mut collected = Vec::new();
    let vs = dependencies.as_hash()?;
    let has_git_repos = !git_repositories.is_empty();
    let empty = "";

    for (_, value) in vs.iter() {
        if let Some(dep) = value["path"]
            .as_str()
            .and_then(|path| DepPattern::plain(path, pubspec_dir).ok())
        {
            collected.push(dep);
            continue;
        }

        // afterwards we try git-path references
        // (only if there are git repositories configured)
        if has_git_repos {
            let version = &value["version"].as_str().unwrap_or(empty);

            let git = &value["git"];
            let git_reference = git["url"].as_str().unwrap_or(empty);
            let git_tag_pattern = git["tag_pattern"].as_str().unwrap_or(empty);

            // when it is pinned to a specific version, we ignore
            if !git_tag_pattern.is_empty() && !version.is_empty() {
                continue;
            }

            // it has to be part of the configured git repositories to be considered
            if !git_repositories
                .iter()
                .any(|git_repo_candidate| git_reference.contains(git_repo_candidate))
            {
                continue;
            }

            // this is a bit of a "heuristic":
            // we only consider relevant if we assume a "main branch"
            let git_ref = git["ref"].as_str().unwrap_or(empty);
            if !git_ref.is_empty() && git_ref != "main" && git_ref != "master" {
                continue;
            }

            if let Some(dep) = git["path"]
                .as_str()
                .and_then(|path| DepPattern::plain(path, root_dir).ok())
            {
                collected.push(dep);
            }
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
        .flat_map(|reference|
            // we do *not* use DepPattern::plain here because since Dart 3.11 the workspace
            // references can be glob patterns too. even we don't use it to determine the
            // matching workspace _yet_ it's better to do it correctly here already
            DepPattern::new(&reference, &root.canonicalized))
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
