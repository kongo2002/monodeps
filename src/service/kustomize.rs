use std::path::Path;

use anyhow::{Result, anyhow};

use crate::config::DepPattern;
use crate::utils::{load_yaml, yaml_str_list};

use super::non_hidden_files;

pub(super) struct KustomizeAnalyzer {}

impl KustomizeAnalyzer {
    pub(super) fn dependencies<P>(&self, dir: P) -> Result<Vec<DepPattern>>
    where
        P: AsRef<Path>,
    {
        let mut collected_imports = Vec::new();

        for entry in non_hidden_files(&dir) {
            let file_name = entry.file_name();
            if !file_name.eq_ignore_ascii_case("kustomization.yaml")
                && !file_name.eq_ignore_ascii_case("kustomization.yml")
            {
                continue;
            }

            log::debug!("kustomization: analyzing file '{}'", entry.path().display());

            let deps = parse_kustomization(entry.path(), &dir)?;

            collected_imports.extend(deps);
        }

        Ok(collected_imports)
    }
}

fn parse_kustomization_dir<P, B>(dir: P, base_dir: B) -> Result<Vec<DepPattern>>
where
    P: AsRef<Path>,
    B: AsRef<Path>,
{
    let yaml_candidate = dir.as_ref().join("kustomization.yaml");
    let yml_candidate = dir.as_ref().join("kustomization.yml");

    if yaml_candidate.exists() {
        parse_kustomization(yaml_candidate, base_dir)
    } else if yml_candidate.exists() {
        parse_kustomization(yml_candidate, base_dir)
    } else {
        Ok(Vec::new())
    }
}

fn parse_kustomization<P, B>(path: P, base_dir: B) -> Result<Vec<DepPattern>>
where
    P: AsRef<Path>,
    B: AsRef<Path>,
{
    let kustomization_dir = path
        .as_ref()
        .parent()
        .ok_or(anyhow!("invalid kustomization resource"))?;

    let yaml = load_yaml(&path)?;

    let resources = yaml_str_list(&yaml["resources"]);
    let bases = yaml_str_list(&yaml["bases"]);
    let components = yaml_str_list(&yaml["components"]);

    let empty_list = Vec::new();
    let patches: Vec<String> = yaml["patches"]
        .as_vec()
        .unwrap_or(&empty_list)
        .into_iter()
        .flat_map(|entry| entry["path"].as_str().map(|x| x.to_owned()))
        .filter(|value| !value.is_empty())
        .collect();

    let config_map_files: Vec<String> = yaml["configMapGenerator"]
        .as_vec()
        .unwrap_or(&empty_list)
        .into_iter()
        .flat_map(|entry| yaml_str_list(&entry["files"]))
        .collect();

    let all_references = resources
        .into_iter()
        .chain(bases.into_iter())
        .chain(components.into_iter())
        .chain(patches.into_iter())
        .chain(config_map_files.into_iter());

    let mut dependencies = Vec::new();

    for resource in all_references {
        let path = kustomization_dir.join(resource);

        if path.is_file() {
            // the reference is a file -> keep as "direct" dependency
            let path_str = path
                .to_str()
                .ok_or(anyhow!("invalid resource file: '{}'", path.display()))?;

            let pattern = DepPattern::new(path_str, &base_dir)?;

            dependencies.push(pattern);
        } else if path.is_dir() {
            // the reference is a directory so we assume a 'kustomization.yaml'
            dependencies.extend(parse_kustomization_dir(path, base_dir.as_ref())?);
        }
    }

    Ok(dependencies)
}
