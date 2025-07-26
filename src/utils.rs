use std::path::Path;

use anyhow::{Result, bail};
use yaml_rust::{Yaml, YamlLoader};

/// Try to read the file at path `file` into a `Yaml` structure.
pub fn load_yaml<P>(file: P) -> Result<Yaml>
where
    P: AsRef<Path>,
{
    if !file.as_ref().exists() {
        bail!("cannot find file {}", file.as_ref().display())
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
pub fn yaml_str_list(yaml: &Yaml) -> Vec<String> {
    let empty_list = Default::default();

    yaml.as_vec()
        .unwrap_or(&empty_list)
        .into_iter()
        .flat_map(|entry| entry.as_str().map(|x| x.to_owned()))
        .filter(|value| !value.is_empty())
        .collect()
}
