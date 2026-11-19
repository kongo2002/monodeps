use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::Path;

use self::cli::{Operation, Opts, OutputFormat};
use self::config::{DepPattern, Language};
use self::path::PathInfo;
use self::service::Service;

use anyhow::{Result, anyhow};
use env_logger::Env;
use yaml_rust2::{Yaml, YamlEmitter};

mod cli;
mod config;
mod dependency;
mod graph;
mod path;
mod service;
mod utils;

/// Main process entrypoint
fn main() {
    // parse CLI arguments
    let (operation, opts) = bail_out(Opts::parse());

    // by default we write all WARN logs on stderr (no timestamp or logger name)
    env_logger::Builder::from_env(Env::default().default_filter_or("warn"))
        .format_timestamp(None)
        .format_target(false)
        .init();

    match operation {
        Operation::Dependencies => dependencies(std::io::stdin().lock(), opts),
        Operation::Validate(path) => validate(&path, opts),
        Operation::Graph => graph(opts),
    }
}

/// Run the 'graph' operation of monodeps.
///
/// It will output a directed graph in dot digraph notation that can be used
/// to render a graph visualizing the service dependencies. This dependency
/// graph is not the 100% identical to the actual dependencies because it
/// cannot emulate all possible file dependencies (especially ones that are
/// based on patterns or regular expressions).
fn graph(opts: Opts) {
    if let Err(err) = graph::calculate(opts) {
        eprintln!("failed to calculate service dependency graph: {err}");
        std::process::exit(1);
    }
}

/// Run the 'dependencies' (default) operation of monodeps.
///
/// It will discover all services in the given target directory and determine
/// all dependencies based on the files given via STDIN.
fn dependencies<R>(reader: R, opts: Opts)
where
    R: BufRead,
{
    let services = service::Service::discover(&opts);

    let result = if !opts.all {
        let changed_files = bail_out(collect_changed_files(reader));
        services.and_then(|services| dependency::resolve(services, changed_files, &opts))
    } else {
        services
    };

    match result {
        Ok(svs) => output(svs, &opts),
        Err(err) => {
            eprintln!("failed to resolve dependencies: {err}");
            std::process::exit(1)
        }
    }
}

/// Run the 'validate' operation of monodeps.
///
/// It will discover the service owning the given path and print all its dependencies, including
/// the services it depends on directly or transitively.
fn validate(service_path: &str, opts: Opts) {
    if let Err(err) = write_validation(std::io::stdout(), service_path, &opts) {
        eprintln!("failed to validate service dependencies: {err}");
        std::process::exit(1);
    }
}

fn write_validation<W>(mut w: W, service_path: &str, opts: &Opts) -> Result<()>
where
    W: Write,
{
    // same discovery and lookup as change detection, so both always agree on the service
    let services: HashMap<_, _> = Service::discover(opts)?
        .into_iter()
        .map(|svc| (svc.path.canonicalized.clone(), svc))
        .collect();
    let root = &opts.target.canonicalized;
    let path = PathInfo::new(service_path, root)?;
    let service = dependency::owning_service(&services, Path::new(&path.canonicalized), root)?
        .ok_or_else(|| anyhow!("cannot find service for: {service_path}"))?;

    let peers_of = |dep: &DepPattern| -> Vec<Cow<str>> {
        let mut peers: Vec<_> = services
            .values()
            .filter(|other| {
                other.path.canonicalized != service.path.canonicalized
                    && dep.is_matched_by(&other.path.canonicalized)
            })
            .map(|other| service_loc(other, opts))
            .collect();
        peers.sort();
        peers
    };

    writeln!(w, "Service: {}", service_loc(service, opts))?;

    if !service.depsfile.dependencies.is_empty() {
        writeln!(w, "Dependencies (configured):")?;
        for dep in &service.depsfile.dependencies {
            write_dependency(&mut w, dep, None, &peers_of(dep), opts)?;
        }
    }

    if !service.auto_dependencies.is_empty() {
        writeln!(w, "Dependencies (auto-discovered):")?;
        for auto in &service.auto_dependencies {
            let pattern = &auto.pattern;
            write_dependency(
                &mut w,
                pattern,
                Some(auto.language),
                &peers_of(pattern),
                opts,
            )?;
        }
    }

    if !opts.config.global_dependencies.is_empty() {
        writeln!(w, "Dependencies (global):")?;
        for dep in &opts.config.global_dependencies {
            write_dependency(&mut w, dep, None, &[], opts)?;
        }
    }

    let mut transitive: Vec<_> = dependency::service_dependencies(&services, service)
        .into_iter()
        .map(|svc| service_loc(svc, opts))
        .collect();
    if !transitive.is_empty() {
        transitive.sort();
        writeln!(w, "Services (transitive):")?;
        for svc in transitive {
            writeln!(w, "  - {svc}")?;
        }
    }

    Ok(())
}

fn write_dependency<W>(
    w: &mut W,
    dep: &DepPattern,
    language: Option<Language>,
    peers: &[Cow<str>],
    opts: &Opts,
) -> std::io::Result<()>
where
    W: Write,
{
    let loc = match dep.plain_path() {
        Some(plain) if opts.relative => path::relative_to(plain, &opts.target.canonicalized),
        _ => dep.to_string(),
    };
    write!(w, "  - {loc}")?;

    if let Some(language) = language {
        write!(w, " [{language}]")?;
    }
    if !peers.is_empty() {
        write!(w, " -> {}", peers.join(", "))?;
    }

    writeln!(w)
}

/// Output the determined list of services to STDOUT.
///
/// Depending on the specified `OutputFormat` the output will be formatted in either plaintext,
/// JSON or YAML.
fn output(services: Vec<Service>, opts: &Opts) {
    match opts.output {
        OutputFormat::Plain => {
            print_services(std::io::stdout(), services, opts);
        }
        OutputFormat::Json => {
            let to_output = services
                .iter()
                .map(|svc| service_loc(svc, opts))
                .collect::<Vec<_>>();
            _ = serde_json::to_writer(std::io::stdout(), &to_output);
        }
        OutputFormat::Yaml => {
            let mut output = String::new();
            {
                let mut emitter = YamlEmitter::new(&mut output);

                let to_output = services
                    .iter()
                    .map(|svc| Yaml::String(service_loc(svc, opts).to_string()))
                    .collect::<Vec<_>>();

                let array = Yaml::Array(to_output);
                _ = emitter.dump(&array);
            }

            // we want to omit the `---` on the first line
            for line in output.lines().skip(1) {
                println!("{}", line);
            }
        }
    }
}

/// Depending on the specified `--relative` option, we output either the full (canonicalized) or
/// relative path.
fn service_loc<'a>(service: &'a Service, opts: &Opts) -> Cow<'a, str> {
    if opts.relative {
        Cow::from(service.path.relative_to(&opts.target))
    } else {
        Cow::from(&service.path.canonicalized)
    }
}

/// Print the plaintext output of the given list of services.
///
/// If specified via the `--verbose` flag, the output will include the `BuildTrigger` (source) of
/// the dependency.
fn print_services<W>(mut w: W, services: Vec<Service>, opts: &Opts)
where
    W: std::io::Write,
{
    for svc in services {
        if !opts.verbose {
            _ = w.write_fmt(format_args!("{}\n", service_loc(&svc, opts)));
        } else {
            _ = w.write_fmt(format_args!(
                "{} [{}]\n",
                service_loc(&svc, opts),
                svc.trigger
                    .as_ref()
                    .map(|t| t.to_string())
                    .unwrap_or_default()
            ));
        }
    }
}

/// Write any error to STDERR and exit with return code 1.
fn bail_out<T>(result: Result<T>) -> T {
    match result {
        Ok(inner) => inner,
        Err(err) => {
            eprintln!("{}", err);
            std::process::exit(1);
        }
    }
}

/// Read the input of changed files from STDIN, expecting one file path per line.
fn collect_changed_files<R>(reader: R) -> Result<Vec<String>>
where
    R: BufRead,
{
    let mut all = Vec::new();

    for line in reader.lines() {
        all.push(line?);
    }

    Ok(all)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use anyhow::Result;

    use crate::cli::Opts;
    use crate::config::{
        AutoDiscoveryConfig, Config, DepPattern, DotnetConfig, FlutterConfig, GoConfig,
    };
    use crate::path::PathInfo;
    use crate::{dependencies, graph, write_validation};

    fn mk_opts() -> Opts {
        Opts {
            target: PathInfo::new("./tests", "").unwrap(),
            config: Config {
                auto_discovery: AutoDiscoveryConfig {
                    go: GoConfig {
                        package_prefixes: vec![],
                    },
                    dotnet: DotnetConfig {
                        package_namespaces: vec![],
                    },
                    flutter: FlutterConfig {
                        git_repositories: vec![],
                    },
                },
                global_dependencies: vec![DepPattern::plain(".github", "./tests").unwrap()],
            },
            all: false,
            output: crate::cli::OutputFormat::Plain,
            verbose: true,
            relative: false,
            supported_roots: vec![],
        }
    }

    #[test]
    fn validate_nested_path() -> Result<()> {
        let opts = Opts {
            target: PathInfo::new("./tests/examples/peer", "")?,
            relative: true,
            ..mk_opts()
        };
        let mut output = Vec::new();
        write_validation(&mut output, "service/src/file", &opts)?;

        let output = String::from_utf8(output)?;
        assert!(output.contains("Service: ./service\n"), "{output}");
        assert!(output.contains("  - ./lib-a/src -> ./lib-a\n"), "{output}");
        assert!(
            output.contains("Services (transitive):\n  - ./lib-a\n  - ./lib-b\n"),
            "{output}"
        );
        Ok(())
    }

    #[test]
    fn validate_without_service() -> Result<()> {
        let opts = Opts {
            target: PathInfo::new("./tests/examples/peer", "")?,
            ..mk_opts()
        };

        assert!(write_validation(Vec::new(), "no-service/file", &opts).is_err());
        Ok(())
    }

    #[test]
    fn graph_does_not_fail() {
        // we just test that it does not fail
        graph(mk_opts());
    }

    #[test]
    fn test_dependencies() {
        // we are emulating STDIN
        let input = String::from("some/file\nanother file\n");
        let cursor = Cursor::new(input);

        // we just test that is does not fail
        dependencies(cursor, mk_opts());
    }

    #[test]
    fn test_dependencies_json() {
        // we are emulating STDIN
        let input = String::from("some/file\nanother file\n");
        let cursor = Cursor::new(input);
        let opts = mk_opts();
        let json_opts = Opts {
            output: crate::cli::OutputFormat::Json,
            ..opts
        };

        // we just test that is does not fail
        dependencies(cursor, json_opts);
    }

    #[test]
    fn test_dependencies_yaml() {
        // we are emulating STDIN
        let input = String::from("some/file\nanother file\n");
        let cursor = Cursor::new(input);
        let opts = mk_opts();
        let yaml_opts = Opts {
            output: crate::cli::OutputFormat::Yaml,
            ..opts
        };

        // we just test that is does not fail
        dependencies(cursor, yaml_opts);
    }
}
