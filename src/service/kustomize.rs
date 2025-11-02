use std::collections::HashSet;
use std::path::Path;

use anyhow::{Result, anyhow};

use crate::config::DepPattern;
use crate::path::canonicalize;
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

            if log::log_enabled!(log::Level::Debug) {
                log::debug!("kustomization: analyzing file '{}'", entry.path().display());
            }

            let mut visited_files = HashSet::new();
            let deps = parse_kustomization(entry.path(), &dir, &mut visited_files)?;

            collected_imports.extend(deps);
        }

        Ok(collected_imports)
    }
}

fn parse_kustomization_dir<P, B>(
    dir: P,
    base_dir: B,
    visited: &mut HashSet<String>,
) -> Result<Vec<DepPattern>>
where
    P: AsRef<Path>,
    B: AsRef<Path>,
{
    let yaml_candidate = dir.as_ref().join("kustomization.yaml");
    let yml_candidate = dir.as_ref().join("kustomization.yml");

    if yaml_candidate.exists() {
        parse_kustomization(yaml_candidate, base_dir, visited)
    } else if yml_candidate.exists() {
        parse_kustomization(yml_candidate, base_dir, visited)
    } else {
        Ok(Vec::new())
    }
}

fn parse_kustomization<P, B>(
    path: P,
    base_dir: B,
    visited: &mut HashSet<String>,
) -> Result<Vec<DepPattern>>
where
    P: AsRef<Path>,
    B: AsRef<Path>,
{
    let kustomization_dir = path
        .as_ref()
        .parent()
        .ok_or_else(|| anyhow!("invalid kustomization resource"))?;

    let canonicalized = canonicalize(path.as_ref())?;
    if visited.contains(&canonicalized) {
        return Err(anyhow!(
            "cyclic dependency in kustomization '{}'",
            path.as_ref().display()
        ));
    }
    visited.insert(canonicalized);

    let yaml = load_yaml(&path)?;

    let resources = yaml_str_list(&yaml["resources"]);
    let bases = yaml_str_list(&yaml["bases"]);
    let components = yaml_str_list(&yaml["components"]);

    let empty_list = Vec::new();
    let patches = yaml["patches"]
        .as_vec()
        .unwrap_or(&empty_list)
        .iter()
        .flat_map(|entry| entry["path"].as_str().map(|x| x.to_owned()))
        .filter(|value| !value.is_empty());

    let config_map_files = yaml["configMapGenerator"]
        .as_vec()
        .unwrap_or(&empty_list)
        .iter()
        .flat_map(|entry| yaml_str_list(&entry["files"]));

    let all_references = resources
        .into_iter()
        .chain(bases)
        .chain(components)
        .chain(patches)
        .chain(config_map_files);

    let mut dependencies = Vec::new();

    for resource in all_references {
        let path = kustomization_dir.join(resource);

        if let Ok(metadata) = path.metadata() {
            if metadata.is_file() {
                // the reference is a file -> keep as "direct" dependency
                let path_str = path
                    .to_str()
                    .ok_or_else(|| anyhow!("invalid resource file: '{}'", path.display()))?;

                let pattern = DepPattern::new(path_str, &base_dir)?;

                dependencies.push(pattern);
            } else if metadata.is_dir() {
                // the reference is a directory so we assume a 'kustomization.yaml'
                dependencies.extend(parse_kustomization_dir(path, base_dir.as_ref(), visited)?);
            }
        }
    }

    Ok(dependencies)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::TempDir;

    fn create_kustomization(dir: &Path, name: &str, content: &str) -> Result<()> {
        let path = dir.join(name);
        let mut file = File::create(path)?;
        file.write_all(content.as_bytes())?;
        Ok(())
    }

    fn tmp() -> Result<TempDir> {
        Ok(tempfile::Builder::default().prefix("mdtest").tempdir()?)
    }

    #[test]
    fn test_simple_kustomization() -> Result<()> {
        let dir = tmp()?;
        let base_dir = dir.path();

        create_kustomization(
            base_dir,
            "kustomization.yaml",
            r#"
resources:
  - resource1.yaml
  - resource2.yaml
"#,
        )?;
        File::create(base_dir.join("resource1.yaml"))?;
        File::create(base_dir.join("resource2.yaml"))?;

        let analyzer = KustomizeAnalyzer {};
        let deps = analyzer.dependencies(base_dir)?;

        assert_eq!(deps.len(), 2);

        Ok(())
    }

    #[test]
    fn test_directory_dependencies() -> Result<()> {
        let dir = tmp()?;
        let base_dir = dir.path();
        let sub_dir = base_dir.join("sub");
        fs::create_dir(&sub_dir)?;

        create_kustomization(
            base_dir,
            "kustomization.yaml",
            r#"
resources:
  - sub
"#,
        )?;

        create_kustomization(
            &sub_dir,
            "kustomization.yaml",
            r#"
resources:
  - sub_resource.yaml
"#,
        )?;
        File::create(sub_dir.join("sub_resource.yaml"))?;

        let analyzer = KustomizeAnalyzer {};
        let deps = analyzer.dependencies(base_dir)?;

        assert_eq!(deps.len(), 2);

        Ok(())
    }

    #[test]
    fn test_bases_and_components() -> Result<()> {
        let dir = tmp()?;
        let base_dir = dir.path();
        let base_dep_dir = base_dir.join("base");
        let component_dep_dir = base_dir.join("component");
        fs::create_dir(&base_dep_dir)?;
        fs::create_dir(&component_dep_dir)?;

        create_kustomization(
            base_dir,
            "kustomization.yaml",
            r#"
bases:
  - base

components:
  - component
"#,
        )?;

        create_kustomization(
            &base_dep_dir,
            "kustomization.yaml",
            r#"
resources:
  - base_resource.yaml
"#,
        )?;
        File::create(base_dep_dir.join("base_resource.yaml"))?;

        create_kustomization(
            &component_dep_dir,
            "kustomization.yaml",
            r#"
resources:
  - component_resource.yaml
"#,
        )?;
        File::create(component_dep_dir.join("component_resource.yaml"))?;

        let analyzer = KustomizeAnalyzer {};
        let deps = analyzer.dependencies(base_dir)?;

        assert_eq!(deps.len(), 4);

        Ok(())
    }

    #[test]
    fn test_patches_and_config_map_generator() -> Result<()> {
        let dir = tmp()?;
        let base_dir = dir.path();

        create_kustomization(
            base_dir,
            "kustomization.yaml",
            r#"
patches:
  - path: patch1.yaml
  - path: patch2.yaml

configMapGenerator:
  - name: my-config
    files:
      - config.properties
"#,
        )?;
        File::create(base_dir.join("patch1.yaml"))?;
        File::create(base_dir.join("patch2.yaml"))?;
        File::create(base_dir.join("config.properties"))?;

        let analyzer = KustomizeAnalyzer {};
        let deps = analyzer.dependencies(base_dir)?;

        assert_eq!(deps.len(), 3);

        Ok(())
    }

    #[test]
    fn test_cyclic_dependency() -> Result<()> {
        let dir = tmp()?;
        let base_dir = dir.path();
        let sub_dir = base_dir.join("sub");
        fs::create_dir(&sub_dir)?;

        create_kustomization(
            base_dir,
            "kustomization.yaml",
            r#"
resources:
  - sub
"#,
        )?;

        create_kustomization(
            &sub_dir,
            "kustomization.yaml",
            &format!(
                r#"
resources:
  - {}
"#,
                base_dir.to_str().unwrap()
            ),
        )?;

        let analyzer = KustomizeAnalyzer {};
        let result = analyzer.dependencies(base_dir);

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("cyclic dependency")
        );

        Ok(())
    }

    #[test]
    fn test_missing_kustomization() -> Result<()> {
        let dir = tmp()?;
        let base_dir = dir.path();
        let sub_dir = base_dir.join("sub");
        fs::create_dir(&sub_dir)?;

        create_kustomization(
            base_dir,
            "kustomization.yaml",
            r#"
resources:
  - sub
"#,
        )?;

        let analyzer = KustomizeAnalyzer {};
        let deps = analyzer.dependencies(base_dir)?;

        assert!(deps.is_empty());

        Ok(())
    }

    #[test]
    fn test_empty_kustomization() -> Result<()> {
        let dir = tmp()?;
        let base_dir = dir.path();

        create_kustomization(base_dir, "kustomization.yaml", "")?;

        let analyzer = KustomizeAnalyzer {};
        let deps = analyzer.dependencies(base_dir)?;

        assert!(deps.is_empty());

        Ok(())
    }

    #[test]
    fn test_non_existent_file() -> Result<()> {
        let dir = tmp()?;
        let base_dir = dir.path();

        create_kustomization(
            base_dir,
            "kustomization.yaml",
            r#"
resources:
  - non-existent-resource.yaml
"#,
        )?;

        let analyzer = KustomizeAnalyzer {};
        let deps = analyzer.dependencies(base_dir)?;

        assert!(deps.is_empty());

        Ok(())
    }
}
