use std::borrow::Cow;

use self::cli::{Operation, Opts, OutputFormat};
use self::service::Service;

use anyhow::Result;
use env_logger::Env;
use yaml_rust::{Yaml, YamlEmitter};

mod cli;
mod config;
mod dependency;
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
        Operation::Dependencies => dependencies(opts),
        Operation::Validate(path) => validate(&path, opts),
    }
}

/// Run the 'dependencies' (default) operation of monodeps.
///
/// It will discover all services in the given target directory and determine
/// all dependencies based on the files given via STDIN.
fn dependencies(opts: Opts) {
    let changed_files = bail_out(collect_changed_files());

    match service::Service::discover(&opts)
        .and_then(|services| dependency::resolve(services, changed_files, &opts))
    {
        Ok(svs) => output(svs, &opts),
        Err(err) => {
            eprintln!("failed to resolve dependencies: {err}");
            std::process::exit(1)
        }
    }
}

/// Run the 'validate' operation of monodeps.
///
/// It will discover a service in the given target directory and determine all services, folder and
/// files that service is depending on.
fn validate(service_path: &str, opts: Opts) {
    match service::Service::try_determine(service_path, &opts) {
        Ok(svc) => {
            if !svc.depsfile.dependencies.is_empty() {
                println!("Dependencies (configured):");

                for dependency in svc.depsfile.dependencies {
                    println!("  - {}", dependency)
                }
            }

            if !svc.auto_dependencies.is_empty() {
                println!("Dependencies (auto-discovered):");

                for dependency in svc.auto_dependencies {
                    println!("  - {} [{}]", dependency.pattern, dependency.language)
                }
            }
        }
        Err(err) => {
            eprintln!("failed validate service dependencies: {err}");
            std::process::exit(1)
        }
    }
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
fn collect_changed_files() -> Result<Vec<String>> {
    let mut all = Vec::new();

    for line in std::io::stdin().lines() {
        all.push(line?);
    }

    Ok(all)
}
