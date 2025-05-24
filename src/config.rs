use std::fmt::Display;
use std::path::Path;

use anyhow::{Result, bail};
use yaml_rust::{Yaml, YamlLoader};

#[derive(Debug)]
pub struct Depsfile {
    pub dependencies: Vec<String>,
}

impl Depsfile {
    /// Attempt to load `Config` from the given file name that
    /// is expected to be a YAML file.
    pub fn load<P>(file: P) -> Result<Depsfile>
    where
        P: AsRef<Path> + Display,
    {
        let config_yaml = load_yaml(file)?;
        Depsfile::load_from_yaml(config_yaml)
    }

    /// Try to parse the given `Yaml` into a valid `Config`
    fn load_from_yaml(config_yaml: Yaml) -> Result<Depsfile> {
        let spec = &config_yaml["spec"];
        let depends_on = &spec["dependsOn"];
        let dependencies = yaml_str_list(depends_on);

        // TODO: validations

        Ok(Depsfile { dependencies })
    }
}

/// Try to read the file at path `file` into a `Yaml` structure.
fn load_yaml<P>(file: P) -> Result<Yaml>
where
    P: AsRef<Path> + Display,
{
    if !file.as_ref().exists() {
        bail!("cannot find file {}", file)
    }

    let config_content = std::fs::read_to_string(file)?;
    let mut docs = YamlLoader::load_from_str(&config_content)?;

    if docs.is_empty() {
        // we just return an empty structure here which is ok
        Ok(Yaml::from_str(""))
    } else {
        // we are only interested in the first parsed "file"
        Ok(docs.remove(0))
    }
}

/// Try to extract the given `Yaml` into a list of `String`.
/// If it is anything else, it will return an empty list.
fn yaml_str_list(yaml: &Yaml) -> Vec<String> {
    let empty_list = Default::default();

    yaml.as_vec()
        .unwrap_or(&empty_list)
        .into_iter()
        .flat_map(|entry| entry.as_str().map(|x| x.to_owned()))
        .filter(|value| !value.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use yaml_rust::{Yaml, YamlLoader};

    use crate::config::Depsfile;

    #[test]
    fn load_config_empty() {
        let config = Depsfile::load_from_yaml(Yaml::from_str(""));

        assert_eq!(config.is_ok(), true);
    }

    #[test]
    fn load_config_no_dependencies() {
        let mut docs = YamlLoader::load_from_str("spec:").unwrap();
        let config = Depsfile::load_from_yaml(docs.remove(0));

        assert_eq!(config.is_ok(), true);
    }
}
