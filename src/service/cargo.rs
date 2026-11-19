use std::fs;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;

use crate::cli::Opts;
use crate::config::DepPattern;

use super::index::RepoIndex;
use super::{LanguageAnalyzer, parent_dir};

pub(super) struct CargoAnalyzer;

impl LanguageAnalyzer for CargoAnalyzer {
    fn file_relevant(&self, file_name: &str) -> bool {
        file_name == "cargo.toml"
    }

    fn dependencies(
        &self,
        files: &[&Path],
        _dir: &str,
        _index: &RepoIndex,
        _opts: &Opts,
    ) -> Result<Vec<DepPattern>> {
        let mut deps = Vec::new();

        for file in files {
            let cargo_dir = match parent_dir(file) {
                Some(d) => d,
                None => continue,
            };

            if log::log_enabled!(log::Level::Debug) {
                log::debug!("cargo: analyzing Cargo.toml '{}'", file.display());
            }

            let content = fs::read_to_string(*file)?;
            let value: toml::Value = content.parse()?;

            for section in &["dependencies", "dev-dependencies", "build-dependencies"] {
                if let Some(table) = value.get(*section) {
                    deps.extend(collect_path_deps(table, &cargo_dir));
                }
            }

            // workspace-level dependencies (root Cargo.toml with [workspace.dependencies])
            if let Some(ws_deps) = value.get("workspace").and_then(|ws| ws.get("dependencies")) {
                deps.extend(collect_path_deps(ws_deps, &cargo_dir));
            }
        }

        Ok(deps)
    }
}

fn collect_path_deps(table: &toml::Value, base: &PathBuf) -> Vec<DepPattern> {
    let entries = match table.as_table() {
        Some(t) => t,
        None => return Vec::new(),
    };

    entries
        .values()
        .filter_map(|v| {
            v.as_table()
                .and_then(|t| t.get("path"))
                .and_then(|p| p.as_str())
                .and_then(|path| DepPattern::plain(path, base).ok())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::service::index::RepoIndex;
    use std::fs;

    use anyhow::Result;
    use tempfile::TempDir;

    use crate::cli::Opts;
    use crate::config::DepPattern;
    use crate::config::{AutoDiscoveryConfig, Config, DotnetConfig, FlutterConfig, GoConfig};
    use crate::path::PathInfo;

    use super::super::LanguageAnalyzer;
    use super::CargoAnalyzer;

    fn mk_opts(target: &str) -> Result<Opts> {
        Ok(Opts {
            target: PathInfo::new(target, "")?,
            config: Config {
                auto_discovery: AutoDiscoveryConfig {
                    go: GoConfig {
                        package_prefixes: vec![],
                    },
                    dotnet: DotnetConfig {
                        package_namespaces: vec![],
                    },
                    flutter: FlutterConfig {
                        git_repositories: vec![],
                    },
                },
                global_dependencies: vec![],
            },
            output: crate::cli::OutputFormat::Plain,
            verbose: false,
            all: false,
            relative: false,
            supported_roots: vec![],
        })
    }

    fn collect_deps(cargo_toml: &str, dir: &TempDir) -> Result<Vec<DepPattern>> {
        let path = dir.path().join("Cargo.toml");
        fs::write(&path, cargo_toml)?;

        let opts = mk_opts(dir.path().to_str().unwrap())?;
        let index = RepoIndex::build(dir.path(), |_| true);
        CargoAnalyzer.dependencies(
            &[path.as_path()],
            dir.path().to_str().unwrap(),
            &index,
            &opts,
        )
    }

    #[test]
    fn path_dependency_detected() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let deps = collect_deps(
            r#"
[package]
name = "my-service"

[dependencies]
my-lib = { path = "../my-lib" }
"#,
            &dir,
        )?;

        assert_eq!(1, deps.len());
        assert!(
            deps[0]
                .as_ref()
                .to_str()
                .map(|s| s.contains("my-lib"))
                .unwrap_or(false),
            "expected dependency on my-lib"
        );
        Ok(())
    }

    #[test]
    fn version_only_dependency_ignored() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let deps = collect_deps(
            r#"
[package]
name = "my-service"

[dependencies]
serde = "1.0"
anyhow = { version = "1.0" }
"#,
            &dir,
        )?;

        assert_eq!(0, deps.len());
        Ok(())
    }

    #[test]
    fn dev_and_build_deps_detected() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let deps = collect_deps(
            r#"
[package]
name = "my-service"

[dev-dependencies]
test-helper = { path = "../test-helper" }

[build-dependencies]
build-utils = { path = "../build-utils" }
"#,
            &dir,
        )?;

        assert_eq!(2, deps.len());
        Ok(())
    }

    #[test]
    fn workspace_dependencies_detected() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let deps = collect_deps(
            r#"
[workspace]
members = ["crates/*"]

[workspace.dependencies]
shared = { path = "crates/shared" }
"#,
            &dir,
        )?;

        assert_eq!(1, deps.len());
        assert!(
            deps[0]
                .as_ref()
                .to_str()
                .map(|s| s.contains("shared"))
                .unwrap_or(false),
            "expected workspace dependency on shared"
        );
        Ok(())
    }
}
