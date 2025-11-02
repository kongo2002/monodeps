use self::cli::{Operation, Opts, OutputFormat};
use self::service::Service;

use anyhow::Result;
use env_logger::Env;

mod cli;
mod config;
mod dependency;
mod path;
mod service;
mod utils;

fn main() {
    let (operation, opts) = bail_out(Opts::parse());

    env_logger::Builder::from_env(Env::default().default_filter_or("warn"))
        .format_timestamp(None)
        .format_target(false)
        .init();

    match operation {
        Operation::Dependencies => dependencies(opts),
        Operation::Validate(path) => validate(&path, opts),
    }
}

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

fn dependencies(opts: Opts) {
    let changed_files = bail_out(collect_changed_files());

    match service::Service::discover(&opts)
        .and_then(|services| dependency::resolve(services, changed_files, &opts))
    {
        Ok(svs) => output(svs, opts.output, opts.verbose),
        Err(err) => {
            eprintln!("failed to resolve dependencies: {err}");
            std::process::exit(1)
        }
    }
}

fn output(services: Vec<Service>, output: OutputFormat, verbose: bool) {
    match output {
        OutputFormat::Plain => {
            print_services(std::io::stdout(), services, verbose);
        }
        OutputFormat::Json => {
            let to_output = services
                .into_iter()
                .map(|svc| svc.path.canonicalized)
                .collect::<Vec<_>>();
            _ = serde_json::to_writer(std::io::stdout(), &to_output);
        }
    }
}

fn print_services<W>(mut w: W, services: Vec<Service>, verbose: bool)
where
    W: std::io::Write,
{
    for svc in services {
        if !verbose {
            _ = w.write_fmt(format_args!("{}\n", svc.path.canonicalized));
        } else {
            _ = w.write_fmt(format_args!(
                "{} [{}]\n",
                svc.path.canonicalized,
                svc.triggers
                    .into_iter()
                    .map(|t| format!("{}", t))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
}

fn bail_out<T>(result: Result<T>) -> T {
    match result {
        Ok(inner) => inner,
        Err(err) => {
            eprintln!("{}", err);
            std::process::exit(1);
        }
    }
}

fn collect_changed_files() -> Result<Vec<String>> {
    let mut all = Vec::new();

    for line in std::io::stdin().lines() {
        all.push(line?);
    }

    Ok(all)
}
