use std::fmt::Display;
use std::path::Path;

use anyhow::{Result, bail};
use regex::Regex;
use yaml_rust::{Yaml, YamlLoader};

use crate::path::PathInfo;

#[derive(Default)]
pub struct Config {
    pub auto_discovery: AutoDiscoveryConfig,
    pub global_dependencies: Vec<String>,
}

#[derive(Default)]
pub struct AutoDiscoveryConfig {
    pub go: GoDepsConfig,
}

#[derive(Default)]
pub struct GoDepsConfig {
    pub package_prefixes: Vec<String>,
}

impl Config {
    pub fn new(path: &str) -> Result<Config> {
        let yaml = load_yaml(path)?;

        let auto_disc = &yaml["auto_discovery"];
        let go_disc = &auto_disc["go"];
        let package_prefixes = yaml_str_list(&go_disc["package_prefixes"]);
        let global_dependencies = yaml_str_list(&yaml["global_dependencies"]);

        Ok(Config {
            auto_discovery: AutoDiscoveryConfig {
                go: GoDepsConfig { package_prefixes },
            },
            global_dependencies,
        })
    }

    pub fn auto_discovery_enabled(&self, language: &Language) -> bool {
        match language {
            Language::Golang => !self.auto_discovery.go.package_prefixes.is_empty(),
            Language::Unknown => false,
        }
    }
}

#[derive(Debug)]
pub struct DepPattern {
    raw: PathInfo,
    pattern: Option<Regex>,
}

impl DepPattern {
    pub fn new(dependency: &str, root_dir: &str) -> Result<Self> {
        let pattern = if dependency.contains(&['?', '*']) {
            Some(to_glob_regex(dependency)?)
        } else {
            None
        };
        let raw = PathInfo::new(dependency, root_dir)?;

        Ok(Self { raw, pattern })
    }

    pub fn is_match(&self, path: &str) -> bool {
        match &self.pattern {
            Some(patt) => patt.is_match(path),
            None => path.starts_with(&self.raw.canonicalized),
        }
    }
}

impl Display for DepPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.raw.path)
    }
}

fn to_glob_regex(pattern: &str) -> Result<Regex> {
    let prepared = pattern
        .replace(".", "\\.")
        .replace("**", ".+")
        .replace("*", "[^/\\\\]+")
        .replace("?", ".");

    let rgx = Regex::new(&prepared)?;
    Ok(rgx)
}

#[derive(Debug, PartialEq)]
pub enum Language {
    Golang,
    Unknown,
}

impl From<&str> for Language {
    fn from(value: &str) -> Self {
        match value {
            "golang" => Language::Golang,
            _ => Language::Unknown,
        }
    }
}

#[derive(Debug)]
pub struct Depsfile {
    pub dependencies: Vec<DepPattern>,
    pub language: Language,
}

impl Depsfile {
    /// Attempt to load `Config` from the given file name that
    /// is expected to be a YAML file.
    pub fn load<P>(file: P, root_dir: &str) -> Result<Depsfile>
    where
        P: AsRef<Path> + Display,
    {
        let config_yaml = load_yaml(file)?;
        Depsfile::load_from_yaml(config_yaml, root_dir)
    }

    /// Try to parse the given `Yaml` into a valid `Config`
    fn load_from_yaml(config_yaml: Yaml, root_dir: &str) -> Result<Depsfile> {
        let spec = &config_yaml["spec"];
        let depends_on = &spec["dependsOn"];
        let dep_patterns = yaml_str_list(depends_on);

        let metadata = &config_yaml["metadata"];
        let language = metadata["builder"].as_str().unwrap_or("").into();

        // TODO: report/warn on invalid patterns?
        let dependencies = dep_patterns
            .into_iter()
            .flat_map(|dep| DepPattern::new(&dep, root_dir))
            .collect();

        Ok(Depsfile {
            dependencies,
            language,
        })
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

    use super::DepPattern;

    #[test]
    fn load_config_empty() {
        let config = Depsfile::load_from_yaml(Yaml::from_str(""), "");

        assert_eq!(config.is_ok(), true);
    }

    #[test]
    fn load_config_no_dependencies() {
        let mut docs = YamlLoader::load_from_str("spec:").unwrap();
        let config = Depsfile::load_from_yaml(docs.remove(0), "");

        assert_eq!(config.is_ok(), true);
    }

    #[test]
    fn dep_pattern_basic() {
        let pat = DepPattern::new("domains/foo", ".").unwrap();

        assert_eq!(pat.is_match("./domains/foo/something"), true);
        assert_eq!(pat.is_match("./domains/else/foo"), false);
    }

    #[test]
    fn dep_pattern_wildcard() {
        let pat = DepPattern::new("domains/foo/services/*/proto", ".").unwrap();

        assert_eq!(pat.is_match("./domains/foo/services/bar/proto"), true);
        assert_eq!(pat.is_match("./domains/bar/services/bar/proto"), false);
    }

    #[test]
    fn dep_pattern_dot() {
        let pat = DepPattern::new("domains/foo/services/.hidden", ".").unwrap();

        assert_eq!(pat.is_match("./domains/foo/services/.hidden/stuff"), true);
        assert_eq!(pat.is_match("./domains/foo/services/xhidden/stuff"), false);
    }

    #[test]
    fn dep_pattern_wildcard_dot() {
        let pat = DepPattern::new("domains/foo/*/.hidden", ".").unwrap();

        assert_eq!(pat.is_match("./domains/foo/services/.hidden/stuff"), true);
        assert_eq!(pat.is_match("./domains/foo/services/xhidden/stuff"), false);
    }

    #[test]
    fn dep_pattern_wildcard_question_mark() {
        let pat = DepPattern::new("domains/foo/??hidden", ".").unwrap();

        assert_eq!(pat.is_match("./domains/foo/.xhidden/stuff"), true);
        assert_eq!(pat.is_match("./domains/foo/.hidden/stuff"), false);
    }
}
