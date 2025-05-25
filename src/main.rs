use self::cli::Opts;

use anyhow::Result;

mod cli;
mod config;
mod dependency;
mod path;
mod service;

fn main() {
    let opts = bail_out(Opts::parse());
    let changed_files = bail_out(collect_changed_files());

    if let Ok(svs) = service::Service::discover(&opts) {
        let svs = dependency::resolve(svs, changed_files, &opts).unwrap();
        for svc in svs {
            println!("{}", svc)
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
