use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

use anyhow::{Result, anyhow};
use serde::Deserialize;

use crate::cli::Opts;
use crate::config::DepPattern;
use crate::path::PathInfo;

use super::index::RepoIndex;
use super::{LanguageAnalyzer, parent_dir};

pub(super) struct PythonAnalyzer {
    root: PathInfo,
    packages: OnceLock<HashMap<String, DepPattern>>,
}

impl PythonAnalyzer {
    pub(super) fn new(root: PathInfo) -> Self {
        PythonAnalyzer {
            root,
            packages: OnceLock::new(),
        }
    }

    fn packages(&self, index: &RepoIndex) -> &HashMap<String, DepPattern> {
        self.packages
            .get_or_init(|| try_load_packages(index, &self.root.canonicalized))
    }
}

pub(super) fn file_relevant(file_name: &str) -> bool {
    file_name == "pyproject.toml"
}

impl LanguageAnalyzer for PythonAnalyzer {
    fn dependencies(
        &self,
        files: &[&Path],
        dir: &str,
        index: &RepoIndex,
        _opts: &Opts,
    ) -> Result<Vec<DepPattern>> {
        let mut deps = Vec::new();
        let all_packages = self.packages(index);

        for file in files {
            let pyproject = parse_pyproject(file)?;

            let sources = pyproject
                .tool
                .and_then(|t| t.uv)
                .and_then(|u| u.sources)
                .unwrap_or_default();

            for (name, source) in sources {
                if let Some(rel_path) = source.path {
                    // Direct path dependency — resolve relative to the service directory
                    let abs = Path::new(dir).join(&rel_path);
                    let pattern = DepPattern::plain(abs, &self.root.canonicalized)?;
                    deps.push(pattern);
                } else if source.workspace == Some(true) {
                    // Workspace member — look up by package name
                    if let Some(found) = all_packages.get(&name) {
                        deps.push(found.clone());
                    }
                }
            }
        }

        Ok(deps)
    }
}

fn try_load_packages(index: &RepoIndex, root: &str) -> HashMap<String, DepPattern> {
    load_packages(index, Path::new(root)).unwrap_or_else(|err| {
        log::warn!("python: failed to load workspace packages: {err}");
        HashMap::new()
    })
}

fn load_packages(index: &RepoIndex, root: &Path) -> Result<HashMap<String, DepPattern>> {
    let mut packages = HashMap::new();
    for path in index.named("pyproject.toml") {
        let pyproject = parse_pyproject(path)?;
        if let Some(name) = pyproject.project.and_then(|p| p.name) {
            let parent = parent_dir(path)
                .ok_or_else(|| anyhow!("cannot determine pyproject.toml directory"))?;
            let pattern = DepPattern::plain(parent, root)?;
            packages.insert(name, pattern);
        }
    }

    Ok(packages)
}

fn parse_pyproject(path: &Path) -> Result<PyProjectToml> {
    let content = fs::read_to_string(path)?;
    let pyproject: PyProjectToml = toml::from_str(&content)?;
    Ok(pyproject)
}

#[derive(Deserialize, Debug, Default)]
struct PyProjectToml {
    project: Option<ProjectSection>,
    tool: Option<ToolSection>,
}

#[derive(Deserialize, Debug)]
struct ProjectSection {
    name: Option<String>,
}

#[derive(Deserialize, Debug)]
struct ToolSection {
    uv: Option<UvSection>,
}

#[derive(Deserialize, Debug)]
struct UvSection {
    sources: Option<HashMap<String, UvSource>>,
}

#[derive(Deserialize, Debug)]
struct UvSource {
    path: Option<String>,
    workspace: Option<bool>,
}

#[cfg(test)]
mod tests {
    use crate::service::index::RepoIndex;
    use std::fs;

    use crate::cli::Opts;
    use crate::config::{AutoDiscoveryConfig, Config, DotnetConfig, FlutterConfig, GoConfig};
    use crate::path::PathInfo;

    use super::super::LanguageAnalyzer;
    use super::{PythonAnalyzer, load_packages};

    const PYPROJECT_01: &str = include_str!("../../tests/resources/pyproject_01.toml");
    const PYPROJECT_02: &str = include_str!("../../tests/resources/pyproject_02.toml");

    fn mk_opts(root: &std::path::Path) -> Opts {
        Opts {
            target: PathInfo::new(root.to_str().unwrap(), "").unwrap(),
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
            all: false,
            output: crate::cli::OutputFormat::Plain,
            verbose: false,
            relative: false,
            supported_roots: vec![],
        }
    }

    fn write(dir: &std::path::Path, name: &str, content: &str) {
        fs::write(dir.join(name), content).unwrap();
    }

    #[test]
    fn workspace_source_resolves_to_package_path() {
        let tmp = tempfile::Builder::new()
            .prefix("py_test_")
            .tempdir()
            .unwrap();
        let root = tmp.path();

        // Create workspace library
        let lib_dir = root.join("my-lib");
        fs::create_dir(&lib_dir).unwrap();
        write(&lib_dir, "pyproject.toml", "[project]\nname = \"my-lib\"\n");

        // Create service with workspace source
        let svc_dir = root.join("service");
        fs::create_dir(&svc_dir).unwrap();
        write(&svc_dir, "pyproject.toml", PYPROJECT_01);

        let opts = mk_opts(root);
        let analyzer = PythonAnalyzer::new(opts.target.clone());

        let pyproject = svc_dir.join("pyproject.toml");

        let deps = analyzer
            .dependencies(
                &[pyproject.as_path()],
                svc_dir.to_str().unwrap(),
                &RepoIndex::build(root, |_, _| true),
                &opts,
            )
            .unwrap();

        assert_eq!(deps.len(), 1);
    }

    #[test]
    fn path_source_resolves_directly() {
        let tmp = tempfile::Builder::new()
            .prefix("py_test_")
            .tempdir()
            .unwrap();
        let root = tmp.path();

        let lib_dir = root.join("shared-lib");
        fs::create_dir(&lib_dir).unwrap();

        let svc_dir = root.join("service");
        fs::create_dir(&svc_dir).unwrap();
        write(&svc_dir, "pyproject.toml", PYPROJECT_02);

        let opts = mk_opts(root);
        let analyzer = PythonAnalyzer::new(opts.target.clone());

        let pyproject = svc_dir.join("pyproject.toml");

        let deps = analyzer
            .dependencies(
                &[pyproject.as_path()],
                svc_dir.to_str().unwrap(),
                &RepoIndex::build(root, |_, _| true),
                &opts,
            )
            .unwrap();

        assert_eq!(deps.len(), 1);
    }

    #[test]
    fn no_sources_section_returns_empty() {
        let tmp = tempfile::Builder::new()
            .prefix("py_test_")
            .tempdir()
            .unwrap();
        let root = tmp.path();
        let svc_dir = root.join("service");
        fs::create_dir(&svc_dir).unwrap();
        write(
            &svc_dir,
            "pyproject.toml",
            "[project]\nname = \"bare-service\"\n",
        );

        let opts = mk_opts(root);
        let analyzer = PythonAnalyzer::new(opts.target.clone());

        let pyproject = svc_dir.join("pyproject.toml");

        let deps = analyzer
            .dependencies(
                &[pyproject.as_path()],
                svc_dir.to_str().unwrap(),
                &RepoIndex::build(root, |_, _| true),
                &opts,
            )
            .unwrap();

        assert!(deps.is_empty());
    }

    #[test]
    fn unknown_workspace_package_not_included() {
        let tmp = tempfile::Builder::new()
            .prefix("py_test_")
            .tempdir()
            .unwrap();
        let root = tmp.path();
        let svc_dir = root.join("service");
        fs::create_dir(&svc_dir).unwrap();
        // References "my-lib" as workspace = true but no package defines that name
        write(&svc_dir, "pyproject.toml", PYPROJECT_01);

        let opts = mk_opts(root);
        let analyzer = PythonAnalyzer::new(opts.target.clone());

        let pyproject = svc_dir.join("pyproject.toml");

        let deps = analyzer
            .dependencies(
                &[pyproject.as_path()],
                svc_dir.to_str().unwrap(),
                &RepoIndex::build(root, |_, _| true),
                &opts,
            )
            .unwrap();

        assert!(deps.is_empty());
    }

    #[test]
    fn load_packages_finds_named_packages() {
        let tmp = tempfile::Builder::new()
            .prefix("py_test_")
            .tempdir()
            .unwrap();
        let root = tmp.path();

        let lib_a = root.join("lib-a");
        let lib_b = root.join("lib-b");
        fs::create_dir(&lib_a).unwrap();
        fs::create_dir(&lib_b).unwrap();
        write(&lib_a, "pyproject.toml", "[project]\nname = \"lib-a\"\n");
        write(&lib_b, "pyproject.toml", "[project]\nname = \"lib-b\"\n");
        // No [project].name → should be ignored
        let no_name_dir = root.join("no-name");
        fs::create_dir(&no_name_dir).unwrap();
        write(
            &no_name_dir,
            "pyproject.toml",
            "[build-system]\nrequires = [\"hatchling\"]\n",
        );

        let packages = load_packages(&RepoIndex::build(root, |_, _| true), root).unwrap();
        assert_eq!(packages.len(), 2);
        assert!(packages.contains_key("lib-a"));
        assert!(packages.contains_key("lib-b"));
    }
}
