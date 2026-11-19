use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::OnceLock;

use anyhow::{Result, anyhow};
use serde::Deserialize;

use crate::cli::Opts;
use crate::config::DepPattern;
use crate::path::PathInfo;

use super::index::RepoIndex;
use super::{LanguageAnalyzer, parent_dir};

pub(super) struct JavaScriptAnalyzer {
    root: PathInfo,
    packages: OnceLock<HashMap<String, DepPattern>>,
}

impl JavaScriptAnalyzer {
    pub(super) fn new(root: PathInfo) -> Self {
        let packages = OnceLock::new();

        JavaScriptAnalyzer { packages, root }
    }

    fn packages(&self, index: &RepoIndex) -> &HashMap<String, DepPattern> {
        self.packages
            .get_or_init(|| try_load_packages(index, &self.root.canonicalized))
    }
}

impl LanguageAnalyzer for JavaScriptAnalyzer {
    fn file_relevant(&self, file_name: &str) -> bool {
        file_name == "package.json"
    }

    fn dependencies(
        &self,
        files: &[&Path],
        _dir: &str,
        index: &RepoIndex,
        _opts: &Opts,
    ) -> Result<Vec<DepPattern>> {
        let mut deps = Vec::new();

        let all_packages = self.packages(index);
        if all_packages.is_empty() {
            return Ok(deps);
        }

        for file in files {
            let package_json = parse_package_json(file)?;

            let all_dependencies = package_json
                .dev_dependencies
                .keys()
                .chain(package_json.dependencies.keys());

            for dependency in all_dependencies {
                if let Some(found) = all_packages.get(dependency) {
                    deps.push(found.clone());
                }
            }
        }

        Ok(deps)
    }
}

fn try_load_packages<P>(index: &RepoIndex, root: P) -> HashMap<String, DepPattern>
where
    P: AsRef<Path>,
{
    load_packages(index, root).unwrap_or_else(|err| {
        log::warn!("javascript: failed to load workspace packages: {err}");
        HashMap::new()
    })
}

fn load_packages<P>(index: &RepoIndex, root: P) -> Result<HashMap<String, DepPattern>>
where
    P: AsRef<Path>,
{
    let mut packages = HashMap::new();
    for path in index.named("package.json") {
        let from_package_json = parse_package_json(path)?;
        if !from_package_json.name.is_empty() {
            let parent = parent_dir(path)
                .ok_or_else(|| anyhow!("cannot determine package.json directory"))?;
            let pattern = DepPattern::plain(parent, &root)?;

            packages.insert(from_package_json.name, pattern);
        }
    }

    Ok(packages)
}

#[derive(Deserialize, Debug)]
struct PackageJsonStub {
    #[serde(default)]
    name: String,

    #[serde(default, rename = "devDependencies")]
    dev_dependencies: HashMap<String, String>,

    #[serde(default)]
    dependencies: HashMap<String, String>,
}

fn parse_package_json(path: &Path) -> Result<PackageJsonStub> {
    let handle = File::open(path)?;
    let reader = BufReader::new(handle);
    let package_json: PackageJsonStub = serde_json::from_reader(reader)?;

    Ok(package_json)
}
