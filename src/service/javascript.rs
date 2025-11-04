use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::OnceLock;

use anyhow::{Result, anyhow};
use serde::Deserialize;

use crate::config::DepPattern;
use crate::path::PathInfo;

use super::{non_hidden_files, parent_dir};

pub(super) struct JavaScriptAnalyzer {
    root: PathInfo,
    packages: OnceLock<HashMap<String, DepPattern>>,
}

impl JavaScriptAnalyzer {
    pub(super) fn new(root: PathInfo) -> Self {
        let packages = OnceLock::new();

        JavaScriptAnalyzer { packages, root }
    }

    fn packages(&self) -> &HashMap<String, DepPattern> {
        self.packages
            .get_or_init(|| try_load_packages(&self.root.canonicalized))
    }

    pub(super) fn dependencies<P>(&self, dir: P) -> Result<Vec<DepPattern>>
    where
        P: AsRef<Path>,
    {
        let mut deps = Vec::new();

        let all_packages = self.packages();
        if all_packages.is_empty() {
            return Ok(deps);
        }

        for entry in non_hidden_files(dir) {
            if !entry.file_name().eq("package.json") {
                continue;
            }

            let package_json = parse_package_json(entry.path())?;

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

fn try_load_packages<P>(root: P) -> HashMap<String, DepPattern>
where
    P: AsRef<Path>,
{
    load_packages(root).unwrap_or_else(|_| HashMap::new())
}

fn load_packages<P>(root: P) -> Result<HashMap<String, DepPattern>>
where
    P: AsRef<Path>,
{
    let mut packages = HashMap::new();
    for entry in non_hidden_files(&root) {
        if !entry.file_name().eq("package.json") {
            continue;
        }

        let from_package_json = parse_package_json(entry.path())?;
        if !from_package_json.name.is_empty() {
            let parent = parent_dir(entry.path())
                .ok_or_else(|| anyhow!("cannot determine package.json directory"))?;
            let pattern = DepPattern::new(parent, &root)?;

            packages.insert(from_package_json.name, pattern);
        }
    }

    Ok(packages)
}

#[derive(Deserialize, Debug)]
struct PackageJsonStub {
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
