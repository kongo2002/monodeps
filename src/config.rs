use std::fmt::Display;
use std::path::Path;

use anyhow::Result;
use regex::Regex;
use yaml_rust::Yaml;

use crate::path::PathInfo;
use crate::utils::{load_yaml, yaml_str_list};

#[derive(Default, Debug, PartialEq)]
pub struct Config {
    pub auto_discovery: AutoDiscoveryConfig,
    pub global_dependencies: Vec<String>,
}

#[derive(Default, Debug, PartialEq)]
pub struct AutoDiscoveryConfig {
    pub go: GoDepsConfig,
    pub dotnet: DotnetConfig,
}

#[derive(Default, Debug, PartialEq)]
pub struct GoDepsConfig {
    pub package_prefixes: Vec<String>,
}

#[derive(Default, Debug, PartialEq)]
pub struct DotnetConfig {
    pub package_namespaces: Vec<String>,
}

impl Config {
    pub fn new(path: &str) -> Result<Config> {
        let yaml = load_yaml(path)?;

        let auto_disc = &yaml["auto_discovery"];
        let global_dependencies = yaml_str_list(&yaml["global_dependencies"]);

        let go_disc = &auto_disc["go"];
        let go_package_prefixes = yaml_str_list(&go_disc["package_prefixes"]);

        let dotnet_disc = &auto_disc["dotnet"];
        let dotnet_package_namespaces = yaml_str_list(&dotnet_disc["package_namespaces"]);

        Ok(Config {
            auto_discovery: AutoDiscoveryConfig {
                go: GoDepsConfig {
                    package_prefixes: go_package_prefixes,
                },
                dotnet: DotnetConfig {
                    package_namespaces: dotnet_package_namespaces,
                },
            },
            global_dependencies,
        })
    }

    pub fn auto_discovery_enabled(&self, language: &Language) -> bool {
        match language {
            Language::Golang => !self.auto_discovery.go.package_prefixes.is_empty(),
            Language::Dotnet => true,
            Language::Flutter => true,
            Language::Kustomize => true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DepPattern {
    raw: PathInfo,
    pattern: Option<Regex>,
}

impl DepPattern {
    pub fn new<P>(dependency: &str, root_dir: P) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        let pattern = if dependency.contains(['?', '*']) {
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

    pub fn is_child_of(&self, canonicalized_path: &str) -> bool {
        match &self.pattern {
            Some(_) => false,
            None => self.raw.canonicalized.starts_with(canonicalized_path),
        }
    }

    pub fn hash(&self) -> Option<&str> {
        match self.pattern {
            Some(_) => None,
            None => Some(&self.raw.canonicalized),
        }
    }
}

impl Display for DepPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.pattern {
            Some(p) => f.write_str(p.as_str()),
            None => f.write_str(&self.raw.canonicalized),
        }
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

#[derive(Debug, Eq, PartialEq, Hash, Clone, Copy)]
pub enum Language {
    Golang,
    Dotnet,
    Flutter,
    Kustomize,
}

impl Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Language::Golang => f.write_str("go"),
            Language::Dotnet => f.write_str("C#"),
            Language::Flutter => f.write_str("flutter"),
            Language::Kustomize => f.write_str("kustomize"),
        }
    }
}

impl TryFrom<&str> for Language {
    type Error = String;

    fn try_from(value: &str) -> std::result::Result<Self, Self::Error> {
        match value {
            "go" => Ok(Language::Golang),
            "golang" => Ok(Language::Golang),
            "dotnet" => Ok(Language::Dotnet),
            "csharp" => Ok(Language::Dotnet),
            "dart" => Ok(Language::Flutter),
            "flutter" => Ok(Language::Flutter),
            "kustomize" => Ok(Language::Kustomize),
            unknown => Err(format!("unknown language: {}", unknown)),
        }
    }
}

#[derive(PartialEq)]
pub enum DepsfileType {
    Depsfile,
    Buildfile,
    Justfile,
    Makefile,
}

#[derive(Debug)]
pub struct Depsfile {
    pub dependencies: Vec<DepPattern>,
    pub languages: Vec<Language>,
}

impl Depsfile {
    /// Attempt to load `Config` from the given file name that
    /// is expected to be a YAML file.
    pub fn load<P>(file_type: DepsfileType, file: P, root_dir: &str) -> Result<Depsfile>
    where
        P: AsRef<Path> + Copy,
    {
        match file_type {
            DepsfileType::Depsfile => {
                let config_yaml = load_yaml(file)?;
                Depsfile::depsfile_from_yaml(config_yaml, file, root_dir)
            }
            DepsfileType::Buildfile => {
                let config_yaml = load_yaml(file)?;
                Depsfile::buildfile_from_yaml(config_yaml, file, root_dir)
            }
            DepsfileType::Justfile => Ok(Depsfile::empty()),
            DepsfileType::Makefile => Ok(Depsfile::empty()),
        }
    }

    fn empty() -> Depsfile {
        Depsfile {
            dependencies: Vec::new(),
            languages: Vec::new(),
        }
    }

    fn depsfile_from_yaml<P>(config_yaml: Yaml, file: P, root_dir: &str) -> Result<Depsfile>
    where
        P: AsRef<Path> + Copy,
    {
        let languages = parse_languages(&config_yaml["languages"], file, root_dir);
        let dep_patterns = yaml_str_list(&config_yaml["dependencies"]);

        let dependencies = dep_patterns
            .into_iter()
            .flat_map(|dep| {
                let dependency = DepPattern::new(&dep, root_dir);
                if dependency.is_err() {
                    log::warn!("{}: invalid dependency '{}'", file.as_ref().display(), dep);
                }
                dependency
            })
            .collect();

        let known_keys = ["languages", "dependencies"];

        // warn about unknown configuration values
        if log::log_enabled!(log::Level::Warn) {
            config_yaml.as_hash().iter().for_each(|hash| {
                for unknown_key in hash
                    .keys()
                    .flat_map(|key| key.as_str())
                    .filter(|key| !known_keys.contains(key))
                {
                    log::warn!(
                        "{}: unknown configuration '{}'",
                        file.as_ref().display(),
                        unknown_key
                    );
                }
            });
        }

        Ok(Depsfile {
            dependencies,
            languages,
        })
    }

    /// Try to parse the given `Yaml` into a valid `Config`
    fn buildfile_from_yaml<P>(config_yaml: Yaml, file: P, root_dir: &str) -> Result<Depsfile>
    where
        P: AsRef<Path> + Copy,
    {
        let spec = &config_yaml["spec"];
        let depends_on = &spec["dependsOn"];
        let dep_patterns = yaml_str_list(depends_on);

        let metadata = &config_yaml["metadata"];

        let languages: Vec<Language> = metadata["builder"]
            .as_str()
            .map(|value| value.try_into().ok())
            .into_iter()
            .flatten()
            .collect();

        let dependencies = dep_patterns
            .into_iter()
            .flat_map(|dep| {
                let dependency = DepPattern::new(&dep, root_dir);
                if dependency.is_err() {
                    log::warn!("{}: invalid dependency '{}'", file.as_ref().display(), dep);
                }
                dependency
            })
            .collect();

        Ok(Depsfile {
            dependencies,
            languages,
        })
    }
}

fn parse_languages<P>(value: &Yaml, file: P, root_dir: &str) -> Vec<Language>
where
    P: AsRef<Path>,
{
    let values = yaml_str_list(value);
    values
        .into_iter()
        .filter_map(|value| match value.as_str().try_into() {
            Ok(language) => Some(language),
            Err(err) => {
                let path = file.as_ref().to_str().unwrap_or(root_dir);
                log::warn!("{path}: {err}");
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Write;
    use std::path::Path;

    use anyhow::Result;
    use tempfile::TempDir;
    use yaml_rust::{Yaml, YamlLoader};

    use crate::config::{
        AutoDiscoveryConfig, Depsfile, DepsfileType, DotnetConfig, GoDepsConfig, Language,
    };

    use super::{Config, DepPattern};

    fn absolute(path: &str) -> String {
        std::path::absolute(path)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned()
    }

    fn create_file(dir: &Path, name: &str, content: &str) -> Result<()> {
        let path = dir.join(name);
        let mut file = File::create(path)?;
        file.write_all(content.as_bytes())?;
        Ok(())
    }

    fn tmp() -> Result<TempDir> {
        Ok(tempfile::Builder::default().prefix("mdtest").tempdir()?)
    }

    #[test]
    fn load_config() -> Result<()> {
        let dir = tmp()?;
        let config_name = "config.yaml";

        create_file(
            &dir.path(),
            config_name,
            r#"
auto_discovery:
  go:
    package_prefixes:
      - foo/bar
  dotnet:
    package_namespaces:
      - Foo.Bar
global_dependencies:
  - justfile
"#,
        )?;

        let result = Config::new(dir.path().join(config_name).to_str().unwrap())?;

        assert_eq!(
            Config {
                auto_discovery: AutoDiscoveryConfig {
                    go: GoDepsConfig {
                        package_prefixes: vec!["foo/bar".to_string()]
                    },
                    dotnet: DotnetConfig {
                        package_namespaces: vec!["Foo.Bar".to_string()]
                    }
                },
                global_dependencies: vec!["justfile".to_string()]
            },
            result
        );

        Ok(())
    }

    #[test]
    fn load_depsfile_empty() {
        let depsfile = Depsfile::depsfile_from_yaml(Yaml::from_str(""), "", "");

        assert_eq!(depsfile.is_ok(), true);
    }

    #[test]
    fn load_depsfile() -> Result<()> {
        let dir = tmp()?;
        let file_name = "Depsfile";

        create_file(
            &dir.path(),
            file_name,
            r#"
languages:
  - go
  - dotnet
dependencies:
  - ../shared/auth
"#,
        )?;

        let depsfile = Depsfile::load(DepsfileType::Depsfile, &dir.path().join(file_name), ".")?;

        assert_eq!(vec![Language::Golang, Language::Dotnet], depsfile.languages);
        assert_eq!(1, depsfile.dependencies.len());

        Ok(())
    }

    #[test]
    fn load_buildfile() -> Result<()> {
        let dir = tmp()?;
        let file_name = "Buildfile.yaml";

        create_file(
            &dir.path(),
            file_name,
            r#"
spec:
  dependsOn:
    - ../shared/auth
metadata:
  builder: go
"#,
        )?;

        let depsfile = Depsfile::load(DepsfileType::Buildfile, &dir.path().join(file_name), ".")?;

        assert_eq!(vec![Language::Golang], depsfile.languages);
        assert_eq!(1, depsfile.dependencies.len());

        Ok(())
    }

    #[test]
    fn load_buildfile_unknown_language() -> Result<()> {
        let dir = tmp()?;
        let file_name = "Buildfile.yaml";

        create_file(
            &dir.path(),
            file_name,
            r#"
spec:
  dependsOn:
    - ../shared/auth
metadata:
  builder: whatever
"#,
        )?;

        let depsfile = Depsfile::load(DepsfileType::Buildfile, &dir.path().join(file_name), ".")?;

        assert_eq!(true, depsfile.languages.is_empty());
        assert_eq!(1, depsfile.dependencies.len());

        Ok(())
    }

    #[test]
    fn load_config_no_dependencies() {
        let mut docs = YamlLoader::load_from_str("spec:").unwrap();
        let config = Depsfile::depsfile_from_yaml(docs.remove(0), "", "");

        assert_eq!(config.is_ok(), true);
    }

    #[test]
    fn dep_pattern_basic() {
        let pat = DepPattern::new("domains/foo", ".").unwrap();

        assert_eq!(pat.is_match(&absolute("./domains/foo/something")), true);
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

        assert_eq!(
            pat.is_match(&absolute("./domains/foo/services/.hidden/stuff")),
            true
        );
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
