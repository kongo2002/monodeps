use self::cli::{Opts, OutputFormat};
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
    let opts = bail_out(Opts::parse());
    let changed_files = bail_out(collect_changed_files());

    env_logger::Builder::from_env(Env::default().default_filter_or("warn"))
        .format_timestamp(None)
        .format_target(false)
        .init();

    match service::Service::discover(&opts)
        .and_then(|services| dependency::resolve(services, changed_files, &opts))
    {
        Ok(svs) => output(svs, opts.output, opts.verbose),
        Err(err) => eprintln!("failed to resolve dependencies: {err}"),
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
