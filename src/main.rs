use self::cli::Opts;

mod cli;
mod path;
mod service;

fn main() {
    let opts = match Opts::parse() {
        Ok(o) => o,
        Err(err) => {
            eprintln!("{}", err);
            std::process::exit(1);
        }
    };

    if let Ok(svs) = service::Service::discover(&opts.target.canonicalized) {
        println!("{:?}", svs)
    }
}
