use anyhow::Result;
use getopts::Options;

use crate::path::PathInfo;

pub struct Opts {
    pub target: PathInfo,
    pub config: Option<String>,
}

impl Opts {
    pub fn parse() -> Result<Self> {
        let args: Vec<_> = std::env::args().collect();

        let mut opts = Options::new();
        opts.optopt("t", "target", "target directory to work on", "DIR");
        opts.optopt("c", "config", "configuration file", "FILE");
        opts.optflag("h", "help", "show help");

        let matches = opts.parse(&args[1..])?;

        // print help/usage
        if matches.opt_present("h") {
            usage(&opts, &args[0]);
            std::process::exit(0);
        }

        let target_dir = matches.opt_str("t").unwrap_or(".".to_owned());
        let target = PathInfo::new(target_dir)?;
        let config = matches.opt_str("c");

        Ok(Self { target, config })
    }
}

fn usage(opts: &Options, exec: &str) {
    let brief = format!("Usage: {} [OPTIONS]", exec);
    print!("{}", opts.usage(&brief));
}
