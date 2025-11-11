use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::fs::File;
use std::io::{BufRead, BufReader, Lines};
use std::path::{Path, PathBuf};

use crate::cli::Opts;
use crate::config::{DepPattern, Depsfile, DepsfileType, Language};
use crate::path::PathInfo;
use anyhow::{Result, anyhow};
use serde::Serialize;
use walkdir::{DirEntry, WalkDir};

use self::dotnet::DotnetAnalyzer;
use self::flutter::FlutterAnalyzer;
use self::go::GoAnalyzer;
use self::javascript::JavaScriptAnalyzer;
use self::kustomize::KustomizeAnalyzer;
use self::proto::ProtoAnalyzer;

mod dotnet;
mod flutter;
mod go;
mod javascript;
mod kustomize;
mod proto;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum BuildTrigger {
    FileChange,
    Dependency(String, bool),
    PeerDependency(String, bool),
    GlobalDependency,
}

struct ServiceContext<'a> {
    filetype: DepsfileType,
    depsfile_location: PathInfo,
    service_location: PathInfo,
    root_dir: &'a str,
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
    javascript: Option<JavaScriptAnalyzer>,
    protobuf: Option<ProtoAnalyzer>,
}

impl Analyzer {
    fn new(opts: &Opts) -> Analyzer {
        let dotnet = if opts.config.auto_discovery_enabled(&Language::Dotnet) {
            DotnetAnalyzer::new()
                .map_err(|err| {
                    log::warn!("failed to initialize dependency analyzer for .NET: {err}");
                    err
                })
                .ok()
        } else {
            None
        };

        let go = if opts.config.auto_discovery_enabled(&Language::Golang) {
            Some(GoAnalyzer {})
        } else {
            None
        };

        let flutter = if opts.config.auto_discovery_enabled(&Language::Flutter) {
            Some(FlutterAnalyzer::new(&opts.target))
        } else {
            None
        };

        let kustomization = if opts.config.auto_discovery_enabled(&Language::Kustomize) {
            Some(KustomizeAnalyzer {})
        } else {
            None
        };

        let javascript = if opts.config.auto_discovery_enabled(&Language::JavaScript) {
            Some(JavaScriptAnalyzer::new(opts.target.clone()))
        } else {
            None
        };

        let protobuf = if opts.config.auto_discovery_enabled(&Language::Protobuf) {
            Some(ProtoAnalyzer::new(opts.target.clone()))
        } else {
            None
        };

        Self {
            dotnet,
            go,
            flutter,
            kustomization,
            javascript,
            protobuf,
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
            Language::JavaScript => self
                .javascript
                .as_ref()
                .map(|analyzer| analyzer.dependencies(&dir))
                .unwrap_or_else(|| Ok(Vec::new())),
            Language::Protobuf => self
                .protobuf
                .as_ref()
                .map(|analyzer| analyzer.dependencies(&dir))
                .unwrap_or_else(|| Ok(Vec::new())),
        };

        match result {
            Ok(deps) => deps,
            Err(err) => {
                log::warn!(
                    "failed to auto-discover dependencies: {err} [{}]",
                    dir.as_ref().display()
                );

                Vec::new()
            }
        }
    }
}

#[derive(Debug)]
pub struct Service {
    pub path: PathInfo,
    pub depsfile: Depsfile,
    pub auto_dependencies: Vec<AutoDependency>,
    pub trigger: Option<BuildTrigger>,
}

#[derive(Debug)]
pub struct AutoDependency {
    pub language: Language,
    pub pattern: DepPattern,
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

impl Display for AutoDependency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{} [{}]", self.pattern, self.language))
    }
}

impl Service {
    pub fn has_trigger(&self) -> bool {
        self.trigger.is_some()
    }

    pub fn trigger(&mut self, trigger: BuildTrigger) {
        self.trigger.replace(trigger);
    }

    pub fn try_determine(path: &str, opts: &Opts) -> Result<Service> {
        let analyzer = Analyzer::new(opts);
        let root_dir = &opts.target.canonicalized;

        let filename_candidates = vec![
            None,
            Some("Depsfile"),
            Some("Buildfile.yaml"),
            Some("justfile"),
            Some("Makefile"),
        ];

        let ctx = filename_candidates
            .into_iter()
            .flat_map(|filename| {
                let full_path = match filename {
                    None => PathBuf::from(path),
                    Some(file) => PathBuf::from(path).join(file),
                };
                ServiceContext::from_depsfile(full_path, root_dir, opts)
            })
            .next()
            .ok_or_else(|| anyhow!("cannot find service root for: {}", path))?;

        Service::discover_service(&analyzer, ctx, opts)
    }

    fn discover_service(analyzer: &Analyzer, ctx: ServiceContext, opts: &Opts) -> Result<Service> {
        // read/parse dependency file (depsfile, buildfile...) and extract
        // any potential explicitly listed dependencies
        let base_depsfile = Depsfile::load(
            ctx.filetype,
            &ctx.depsfile_location.canonicalized,
            ctx.root_dir,
        )?;

        // try to determine what languages we can auto-discover
        let depsfile = auto_discover_languages(base_depsfile, &ctx.service_location);

        // try to determine all dependencies of languages we detected
        // in this service folder
        let mut unique_auto_dep_paths = HashSet::new();
        let auto_dependencies = depsfile
            .languages
            .iter()
            .flat_map(|language| {
                analyzer
                    .auto_discover(language, &ctx.service_location.canonicalized, opts)
                    .into_iter()
                    .map(|pattern| AutoDependency {
                        language: *language,
                        pattern,
                    })
            })
            .filter(|auto_dep| {
                // auto-discovered dependencies could be "anywhere", that's why we filter
                // out all that are directly below this service directory
                not_within_service(&ctx.service_location, &auto_dep.pattern)
                    // moreover we filter out all obvious duplicates
                    && auto_dep
                        .pattern
                        .hash()
                        .map(|hash| unique_auto_dep_paths.insert(hash.to_owned()))
                        .unwrap_or(true)
            })
            .collect();

        Ok(Service {
            path: ctx.service_location,
            depsfile,
            auto_dependencies,
            trigger: None,
        })
    }

    pub fn discover(opts: &Opts) -> Result<Vec<Service>> {
        let analyzer = Analyzer::new(opts);
        let root_dir = &opts.target.canonicalized;
        let mut contexts = HashMap::new();

        // first we collect all "distinct" service contexts
        for entry in non_hidden_files(root_dir) {
            if let Some(ctx) = ServiceContext::from_depsfile(entry.into_path(), root_dir, opts) {
                // when the dependency file is directly in the project root there is no real
                // reason to consider it because we would just return the full project
                if ctx.service_location.canonicalized == *root_dir {
                    continue;
                }

                match contexts.entry(ctx.service_location.canonicalized.clone()) {
                    Entry::Vacant(free) => {
                        free.insert(ctx);
                    }
                    Entry::Occupied(exists) => exists.into_mut().merge(ctx, opts),
                };
            }
        }

        // afterwards we are resolving all service contexts into actual services
        contexts
            .into_values()
            .map(|ctx| Service::discover_service(&analyzer, ctx, opts))
            .collect()
    }
}

fn not_within_service(service_dir: &PathInfo, pattern: &DepPattern) -> bool {
    !pattern.is_child_of(&service_dir.canonicalized)
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

    let languages = filetype_frequencies
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
        "proto" => {
            return Some(LanguageMatch {
                language: Language::Protobuf,
                score: 3,
            });
        }
        "js" | "jsx" | "tsx" | "ts" => {
            return Some(LanguageMatch {
                language: Language::JavaScript,
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
        "package.json" => Some(LanguageMatch {
            language: Language::JavaScript,
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

fn parent_dir(filename: &Path) -> Option<PathBuf> {
    let path = PathBuf::from(filename);
    path.ancestors().nth(1).map(|x| x.to_owned())
}

fn map_depsfile(filename: &str, opts: &Opts) -> Option<DepsfileType> {
    let filetype = match filename {
        "Depsfile" => Some(DepsfileType::Depsfile),
        "Buildfile.yaml" => Some(DepsfileType::Buildfile),
        "justfile" => Some(DepsfileType::Justfile),
        "Makefile" => Some(DepsfileType::Makefile),
        _ => None,
    };

    filetype.filter(|ft| opts.is_supported(ft))
}

impl ServiceContext<'_> {
    fn from_depsfile<'a>(
        path: PathBuf,
        root_dir: &'a str,
        opts: &Opts,
    ) -> Option<ServiceContext<'a>> {
        let filetype = map_depsfile(path.file_name()?.to_str()?, opts)?;

        if !path.is_file() {
            return None;
        }

        let depsfile_location = PathInfo::new(&path, root_dir).ok()?;
        let service_location = path
            .parent()
            .and_then(|p| PathInfo::new(p, root_dir).ok())?;

        Some(ServiceContext {
            filetype,
            depsfile_location,
            service_location,
            root_dir,
        })
    }

    /// Merge will combine the information from two ServiceContexts
    /// and keep the most "important" values, depending on their
    /// precedence, mostly `Depsfile` being the most preferred.
    fn merge(&mut self, other: ServiceContext, opts: &Opts) {
        if self.filetype == DepsfileType::Depsfile || !opts.is_supported(&other.filetype) {
            return;
        }

        if !opts.is_supported(&self.filetype) || self.filetype > other.filetype {
            self.depsfile_location = other.depsfile_location;
            self.service_location = other.service_location;
            self.filetype = other.filetype;
        }
    }
}

fn read_lines<P>(filename: P) -> Result<Lines<BufReader<File>>>
where
    P: AsRef<Path>,
{
    let file = File::open(filename)?;
    Ok(BufReader::new(file).lines())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use anyhow::Result;

    use crate::cli::Opts;
    use crate::config::{AutoDiscoveryConfig, Config, DepsfileType, DotnetConfig, GoDepsConfig};
    use crate::path::PathInfo;
    use crate::service::ServiceContext;
    use crate::{dependency, print_services};

    use super::Service;

    fn expect_output(services: Vec<Service>, expected_services: Vec<&str>) -> Result<()> {
        let mut cursor = Cursor::new(Vec::new());
        let opts = mk_opts("./tests/examples/full")?;
        print_services(&mut cursor, services, &opts);

        let output = String::from_utf8(cursor.into_inner())?;

        for expected in expected_services {
            assert!(output.contains(expected), "output contains '{}'", expected);
        }

        Ok(())
    }

    fn get_service(services: Vec<Service>, name: &str) -> Option<Service> {
        services
            .into_iter()
            .find(|svc| svc.path.display_path.ends_with(name))
    }

    fn mk_opts(target: &str) -> Result<Opts> {
        let opts = Opts {
            target: PathInfo::new(target, "")?,
            config: Config {
                auto_discovery: AutoDiscoveryConfig {
                    go: GoDepsConfig {
                        package_prefixes: vec!["dev.azure.com/foo/bar".to_string()],
                    },
                    dotnet: DotnetConfig {
                        package_namespaces: vec![],
                    },
                },
                global_dependencies: vec![],
            },
            output: crate::cli::OutputFormat::Plain,
            verbose: true,
            relative: false,
            supported_roots: vec![],
        };

        Ok(opts)
    }

    #[test]
    fn discover_services_not_exist() -> Result<()> {
        let opts = mk_opts("does_not_exist")?;
        let services = Service::discover(&opts)?;

        assert_eq!(true, services.is_empty());
        Ok(())
    }

    #[test]
    fn discover_services_depsfile() -> Result<()> {
        let opts = mk_opts("./tests/examples/full")?;
        let services = Service::discover(&opts)?;

        // just 2 Depsfile
        assert_eq!(2, services.len());
        Ok(())
    }

    #[test]
    fn discover_services_duplicate_files() -> Result<()> {
        let opts = mk_opts("./tests/examples/full")?;
        let justfile_opts = Opts {
            supported_roots: vec![DepsfileType::Justfile],
            ..opts
        };
        let services = Service::discover(&justfile_opts)?;

        // 2 Depsfile + 4 justfiles
        //
        // technically we have 2 Depsfiles and 5! justfiles,
        // however we want the Depsfile in service-e to take precedence
        assert_eq!(6, services.len());

        let service_e = get_service(services, "service-e");

        assert!(service_e.is_some(), "service-e was not discovered");

        // - service-f
        assert_eq!(1, service_e.unwrap().depsfile.dependencies.len());

        Ok(())
    }

    #[test]
    fn discover_services_justfile() -> Result<()> {
        let opts = mk_opts("./tests/examples/full")?;
        let justfile_opts = Opts {
            supported_roots: vec![DepsfileType::Justfile],
            ..opts
        };
        let services = Service::discover(&justfile_opts)?;

        // 2 Depsfile + 4 justfiles
        assert_eq!(6, services.len());

        let service_a = get_service(services, "service-a");

        assert!(service_a.is_some(), "service-a was not discovered");

        // - shared/something
        // - pkg/some
        assert_eq!(2, service_a.unwrap().auto_dependencies.len());

        Ok(())
    }

    #[test]
    fn discover_services_makefile() -> Result<()> {
        let opts = mk_opts("./tests/examples/full")?;
        let makefile_opts = Opts {
            supported_roots: vec![DepsfileType::Makefile],
            ..opts
        };
        let services = Service::discover(&makefile_opts)?;

        // 2 Depsfile + 1 Makefile
        assert_eq!(3, services.len());

        Ok(())
    }

    #[test]
    fn resolve_dependencies_shared() -> Result<()> {
        let opts = mk_opts("./tests/examples/full")?;
        let all_opts = Opts {
            supported_roots: vec![DepsfileType::Makefile, DepsfileType::Justfile],
            ..opts
        };
        let services = Service::discover(&all_opts)?;

        // 2 Depsfile + 1 Makefile + 4 justfile
        assert_eq!(7, services.len());

        let deps = dependency::resolve(services, vec!["shared/something".to_string()], &all_opts)?;

        // - shared
        // - service-a
        // - service-c
        assert_eq!(3, deps.len());
        expect_output(deps, vec!["service-a", "service-c", "shared"])?;

        Ok(())
    }

    #[test]
    fn resolve_dependencies_k8s_patch() -> Result<()> {
        let opts = mk_opts("./tests/examples/full")?;
        let justfile_opts = Opts {
            supported_roots: vec![DepsfileType::Justfile],
            ..opts
        };
        let services = Service::discover(&justfile_opts)?;

        // 2 Depsfile + 4 justfile
        assert_eq!(6, services.len());

        let deps = dependency::resolve(
            services,
            vec!["k8s/base/patch.yaml".to_string()],
            &justfile_opts,
        )?;

        // - service-d
        assert_eq!(1, deps.len());
        expect_output(deps, vec!["service-d"])?;

        Ok(())
    }

    #[test]
    fn resolve_dependencies_k8s() -> Result<()> {
        let opts = mk_opts("./tests/examples/full")?;
        let justfile_opts = Opts {
            supported_roots: vec![DepsfileType::Justfile],
            ..opts
        };
        let services = Service::discover(&justfile_opts)?;

        // 2 Depsfile + 4 justfile
        assert_eq!(6, services.len());

        let deps = dependency::resolve(
            services,
            vec!["k8s/base/kustomization.yaml".to_string()],
            &justfile_opts,
        )?;

        // - service-d
        assert_eq!(1, deps.len());
        expect_output(deps, vec!["service-d"])?;

        Ok(())
    }

    #[test]
    fn merge_correct_filetype_order() {
        assert_eq!(true, DepsfileType::Depsfile < DepsfileType::Buildfile);
        assert_eq!(true, DepsfileType::Depsfile < DepsfileType::Justfile);
        assert_eq!(true, DepsfileType::Depsfile < DepsfileType::Makefile);
        assert_eq!(true, DepsfileType::Buildfile < DepsfileType::Justfile);
    }

    #[test]
    fn merge_overwrites_justfile() -> Result<()> {
        let opts = mk_opts("./tests/examples/full")?;
        let all_opts = Opts {
            supported_roots: vec![DepsfileType::Makefile, DepsfileType::Justfile],
            ..opts
        };

        let mut justfile_ctx = ServiceContext {
            filetype: DepsfileType::Justfile,
            depsfile_location: PathInfo::new(".", ".")?,
            service_location: PathInfo::new(".", ".")?,
            root_dir: ".",
        };

        let depsfile_ctx = ServiceContext {
            filetype: DepsfileType::Depsfile,
            depsfile_location: PathInfo::new(".", ".")?,
            service_location: PathInfo::new(".", ".")?,
            root_dir: ".",
        };

        justfile_ctx.merge(depsfile_ctx, &all_opts);

        assert_eq!(DepsfileType::Depsfile, justfile_ctx.filetype);

        Ok(())
    }

    #[test]
    fn resolve_dependencies_one_service() -> Result<()> {
        let opts = mk_opts("./tests/examples/full")?;
        let all_opts = Opts {
            supported_roots: vec![DepsfileType::Makefile, DepsfileType::Justfile],
            ..opts
        };
        let services = Service::discover(&all_opts)?;

        // 2 Depsfile + 1 Makefile + 4 justfile
        assert_eq!(7, services.len());

        let deps = dependency::resolve(
            services,
            vec![
                "service-c/something".to_string(),
                "non-existing-folder/something".to_string(),
            ],
            &all_opts,
        )?;

        // - service-c
        assert_eq!(1, deps.len());
        expect_output(deps, vec!["service-c"])?;

        Ok(())
    }

    #[test]
    fn resolve_dependencies_global() -> Result<()> {
        let opts = mk_opts("./tests/examples/full")?;
        let all_opts = Opts {
            supported_roots: vec![DepsfileType::Makefile, DepsfileType::Justfile],
            config: Config {
                auto_discovery: Default::default(),
                global_dependencies: vec![".gitlab".to_string()],
            },
            ..opts
        };
        let services = Service::discover(&all_opts)?;

        // 2 Depsfile + 1 Makefile + 4 justfile
        assert_eq!(7, services.len());

        let deps = dependency::resolve(
            services,
            vec![".gitlab/pipeline.yml".to_string()],
            &all_opts,
        )?;

        // - service-a
        // - service-b
        // - service-c
        // - service-d
        // - service-e
        // - service-f
        // - shared
        assert_eq!(7, deps.len());
        expect_output(
            deps,
            vec![
                "service-a",
                "service-b",
                "service-c",
                "service-d",
                "service-e",
                "service-f",
                "shared",
            ],
        )?;

        Ok(())
    }
}
