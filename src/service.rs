use std::collections::HashMap;
use std::fmt::Display;
use std::fs::File;
use std::io::{BufRead, BufReader, Lines};
use std::path::{Path, PathBuf};

use crate::cli::Opts;
use crate::config::{Config, DepPattern, Depsfile, DepsfileType, Language};
use crate::path::PathInfo;
use anyhow::Result;
use serde::Serialize;
use walkdir::{DirEntry, WalkDir};

use self::dotnet::DotnetAnalyzer;
use self::flutter::FlutterAnalyzer;
use self::go::GoAnalyzer;
use self::kustomize::KustomizeAnalyzer;

mod dotnet;
mod flutter;
mod go;
mod kustomize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum BuildTrigger {
    FileChange,
    Dependency(String, bool),
    PeerDependency(String, bool),
    GlobalDependency,
}

impl Display for BuildTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BuildTrigger::FileChange => f.write_str("FileChange"),
            BuildTrigger::Dependency(dep, true) => {
                f.write_fmt(format_args!("Auto-Dependency({})", dep))
            }
            BuildTrigger::Dependency(dep, false) => {
                f.write_fmt(format_args!("Dependency({})", dep))
            }
            BuildTrigger::PeerDependency(dep, true) => {
                f.write_fmt(format_args!("Auto-Peer-Dependency({})", dep))
            }
            BuildTrigger::PeerDependency(dep, false) => {
                f.write_fmt(format_args!("Peer-Dependency({})", dep))
            }
            BuildTrigger::GlobalDependency => f.write_str("Global"),
        }
    }
}

struct Analyzer {
    dotnet: Option<DotnetAnalyzer>,
    go: Option<GoAnalyzer>,
    flutter: Option<FlutterAnalyzer>,
    kustomization: Option<KustomizeAnalyzer>,
}

impl Analyzer {
    fn new(config: &Config) -> Analyzer {
        let dotnet = if config.auto_discovery_enabled(&Language::Dotnet) {
            DotnetAnalyzer::new()
                .map_err(|err| {
                    log::warn!("failed to initialize dependency analyzer for .NET: {err}");
                    err
                })
                .ok()
        } else {
            None
        };

        let go = if config.auto_discovery_enabled(&Language::Golang) {
            Some(GoAnalyzer {})
        } else {
            None
        };

        let flutter = if config.auto_discovery_enabled(&Language::Flutter) {
            Some(FlutterAnalyzer {})
        } else {
            None
        };

        let kustomization = if config.auto_discovery_enabled(&Language::Flutter) {
            Some(KustomizeAnalyzer {})
        } else {
            None
        };

        Self {
            dotnet,
            go,
            flutter,
            kustomization,
        }
    }

    fn auto_discover<P>(&self, language: &Language, dir: P, opts: &Opts) -> Vec<DepPattern>
    where
        P: AsRef<Path>,
    {
        let result = match language {
            Language::Golang => self
                .go
                .as_ref()
                .map(|analyzer| analyzer.dependencies(&dir, opts))
                .unwrap_or_else(|| Ok(Vec::new())),
            Language::Dotnet => self
                .dotnet
                .as_ref()
                .map(|analyzer| analyzer.dependencies(&dir, opts))
                .unwrap_or_else(|| Ok(Vec::new())),
            Language::Flutter => self
                .flutter
                .as_ref()
                .map(|analyzer| analyzer.dependencies(&dir))
                .unwrap_or_else(|| Ok(Vec::new())),
            Language::Kustomize => self
                .kustomization
                .as_ref()
                .map(|analyzer| analyzer.dependencies(&dir))
                .unwrap_or_else(|| Ok(Vec::new())),
            Language::Unknown => Ok(Vec::new()),
        };

        match result {
            Ok(deps) => deps,
            Err(err) => {
                let path = dir.as_ref().to_str().unwrap_or_default();
                log::warn!("failed to auto-discover dependencies: {err} [{path}]");

                Vec::new()
            }
        }
    }
}

#[derive(Debug)]
pub struct Service {
    pub path: PathInfo,
    pub depsfile: Depsfile,
    pub auto_dependencies: Vec<DepPattern>,
    pub triggers: Vec<BuildTrigger>,
}

impl Display for Service {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Service{'")?;
        f.write_str(self.path.canonicalized.as_str())?;

        f.write_str("',dependencies:[")?;
        for (idx, value) in self.depsfile.dependencies.iter().enumerate() {
            if idx > 0 {
                f.write_str(",")?;
            }
            f.write_fmt(format_args!("'{}'", value))?;
        }

        f.write_str("],auto_dependencies:[")?;
        for (idx, value) in self.auto_dependencies.iter().enumerate() {
            if idx > 0 {
                f.write_str(",")?;
            }
            f.write_fmt(format_args!("'{}'", value))?;
        }

        f.write_str("]}")
    }
}

impl Service {
    pub fn has_trigger(&self) -> bool {
        !self.triggers.is_empty()
    }

    pub fn trigger(&mut self, trigger: BuildTrigger) {
        if !self.triggers.contains(&trigger) {
            self.triggers.push(trigger)
        }
    }

    pub fn discover(opts: &Opts) -> Result<Vec<Service>> {
        let analyzer = Analyzer::new(&opts.config);
        let root_dir = &opts.target.canonicalized;
        let mut all = Vec::new();

        for entry in non_hidden_files(root_dir) {
            let filename = entry.file_name().to_str().unwrap_or("");
            let filetype = match filename {
                "Buildfile.yaml" => Some(DepsfileType::Buildfile),
                "Depsfile" => Some(DepsfileType::Depsfile),
                "justfile" => {
                    Some(DepsfileType::Justfile).filter(|x| opts.supported_roots.contains(x))
                }
                "Makefile" => {
                    Some(DepsfileType::Makefile).filter(|x| opts.supported_roots.contains(x))
                }
                _ => None,
            };

            if let Some(valid_filetype) = filetype {
                if let Some((depsfile_location, path)) = get_locations(entry, root_dir) {
                    // when the dependency file is directly in the project root there is no real
                    // reason to consider it because we would just return the full project
                    if path.canonicalized == *root_dir {
                        continue;
                    }

                    let base_depsfile =
                        Depsfile::load(valid_filetype, &depsfile_location.canonicalized, root_dir)?;

                    let depsfile = auto_discover_languages(base_depsfile, &path);

                    // TODO: we may want to filter out dependencies that are within the current
                    //  service's root directory
                    let auto_dependencies = depsfile
                        .languages
                        .iter()
                        .flat_map(|language| {
                            analyzer.auto_discover(language, &path.canonicalized, opts)
                        })
                        .collect();

                    let triggers = Vec::new();
                    let service = Service {
                        path,
                        depsfile,
                        auto_dependencies,
                        triggers,
                    };

                    all.push(service);
                }
            }
        }
        Ok(all)
    }
}

fn auto_discover_languages(depsfile: Depsfile, path: &PathInfo) -> Depsfile {
    if !depsfile.languages.is_empty() {
        return depsfile;
    }

    let mut filetype_frequencies = HashMap::new();

    for entry in non_hidden_files(&path.canonicalized) {
        if let Some(lang) = try_determine_language(&entry) {
            let val = filetype_frequencies.entry(lang.language).or_insert(0);
            *val += lang.score;
        }
    }

    let mut freq_list = filetype_frequencies.into_iter().collect::<Vec<_>>();
    freq_list.sort_by_key(|entry| -entry.1);

    let languages = freq_list
        .into_iter()
        .filter(|(_, score)| *score >= 3)
        .map(|tpl| tpl.0)
        .collect();

    Depsfile {
        languages,
        ..depsfile
    }
}

struct LanguageMatch {
    language: Language,
    score: i32,
}

fn try_determine_language(entry: &DirEntry) -> Option<LanguageMatch> {
    let extension = entry.path().extension().and_then(|x| x.to_str())?;

    match extension {
        "cs" => {
            return Some(LanguageMatch {
                language: Language::Dotnet,
                score: 1,
            });
        }
        "csproj" => {
            return Some(LanguageMatch {
                language: Language::Dotnet,
                score: 5,
            });
        }
        "go" => {
            return Some(LanguageMatch {
                language: Language::Golang,
                score: 1,
            });
        }
        "dart" => {
            return Some(LanguageMatch {
                language: Language::Flutter,
                score: 1,
            });
        }
        _ => {}
    }

    match entry.file_name().to_str().unwrap_or_default() {
        "pubspec.yaml" | "pubspec.lock" => Some(LanguageMatch {
            language: Language::Flutter,
            score: 5,
        }),
        "go.mod" | "go.sum" => Some(LanguageMatch {
            language: Language::Golang,
            score: 5,
        }),
        "kustomization.yaml" | "kustomization.yml" => Some(LanguageMatch {
            language: Language::Kustomize,
            score: 5,
        }),
        _ => None,
    }
}

fn non_hidden_files<P>(dir: P) -> impl IntoIterator<Item = DirEntry>
where
    P: AsRef<Path>,
{
    WalkDir::new(dir)
        .into_iter()
        // filter hidden files/directories
        .filter_entry(|e| {
            !e.file_name()
                .to_str()
                .map(|s| s.starts_with(".") || s == "node_modules")
                .unwrap_or(false)
        })
        // skip errors (e.g. non permission directories)
        .filter_map(|e| e.ok())
}

fn parent_dir(filename: &Path) -> Option<String> {
    let path = PathBuf::from(filename);
    path.ancestors()
        .skip(1)
        .next()
        .and_then(|p| p.to_str())
        .map(|p| p.to_string())
        .filter(|p| !p.is_empty())
}

fn get_locations(entry: DirEntry, root_dir: &str) -> Option<(PathInfo, PathInfo)> {
    let depsfile_location = entry
        .path()
        .to_str()
        .and_then(|p| PathInfo::new(p, root_dir).ok())?;

    let path_buf = entry.into_path();
    let path = path_buf.parent().and_then(|p| to_pathinfo(p, root_dir))?;

    Some((depsfile_location, path))
}

fn to_pathinfo(p: &Path, root_dir: &str) -> Option<PathInfo> {
    let path = p.to_str()?;

    PathInfo::new(path, root_dir).ok()
}

fn read_lines<P>(filename: P) -> Result<Lines<BufReader<File>>>
where
    P: AsRef<Path>,
{
    let file = File::open(filename)?;
    Ok(BufReader::new(file).lines())
}
