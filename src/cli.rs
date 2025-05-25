use anyhow::Result;
use getopts::Options;

use crate::config::Config;
use crate::path::PathInfo;

pub struct Opts {
    pub target: PathInfo,
    pub config: Config,
}

impl Opts {
    pub fn parse() -> Result<Self> {
        let args: Vec<_> = std::env::args().collect();

        let mut opts = Options::new();
        opts.optopt("t", "target", "target directory to operate on", "DIR");
        opts.optopt("c", "config", "configuration file", "FILE");
        opts.optflag("h", "help", "show help");

        let matches = opts.parse(&args[1..])?;

        // print help/usage
        if matches.opt_present("h") {
            usage(&opts, &args[0]);
            std::process::exit(0);
        }

        let target_dir = matches.opt_str("t").unwrap_or(".".to_owned());
        let target = PathInfo::new(&target_dir, "")?;
        let config_path = matches.opt_str("c");

        let config = match config_path {
            Some(path) => Config::new(&path)?,
            None => {
                // try default location in current directory
                Config::new("./.monodeps.yaml").unwrap_or_default()
            }
        };

        Ok(Self { target, config })
    }
}

fn usage(opts: &Options, exec: &str) {
    let brief = format!("Usage: {} [OPTIONS]", exec);
    print!("{}", opts.usage(&brief));
}
