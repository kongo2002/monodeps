use std::collections::HashSet;

use self::cli::Opts;

use anyhow::Result;

mod cli;
mod config;
mod path;
mod service;

fn main() {
    let opts = bail_out(Opts::parse());
    let changed_files = bail_out(collect_changed_files());

    if let Ok(svs) = service::Service::discover(&opts.target.canonicalized) {
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

fn collect_changed_files() -> Result<HashSet<String>> {
    let mut all = HashSet::new();

    for line in std::io::stdin().lines() {
        all.insert(line?);
    }

    Ok(all)
}
