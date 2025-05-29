use anyhow::{Result, anyhow};
use getopts::Options;

use crate::config::Config;
use crate::path::PathInfo;

pub enum OutputFormat {
    Plain,
    Json,
}

pub struct Opts {
    pub target: PathInfo,
    pub config: Config,
    pub output: OutputFormat,
    pub verbose: bool,
}

impl Opts {
    pub fn parse() -> Result<Self> {
        let args: Vec<_> = std::env::args().collect();

        let mut opts = Options::new();
        opts.optopt("t", "target", "target directory to operate on", "DIR");
        opts.optopt("c", "config", "configuration file", "FILE");
        opts.optopt("o", "output", "output format", "FORMAT");
        opts.optflag("v", "verbose", "verbose output");
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
        let output = parse_format(
            matches
                .opt_str("o")
                .unwrap_or_else(|| String::from("plain")),
        )?;
        let verbose = matches.opt_present("v");

        let config = match config_path {
            Some(path) => Config::new(&path)?,
            None => {
                // try default location in current directory
                Config::new("./.monodeps.yaml").unwrap_or_default()
            }
        };

        Ok(Self {
            target,
            config,
            output,
            verbose,
        })
    }
}

fn parse_format(input: String) -> Result<OutputFormat> {
    match input.as_str() {
        "json" => Ok(OutputFormat::Json),
        "plain" => Ok(OutputFormat::Plain),
        _ => Err(anyhow!("invalid output format (supported: plain, json)")),
    }
}

fn usage(opts: &Options, exec: &str) {
    let brief = format!(
        r#"Usage: {} [OPTIONS]

monodeps is a tool to help with change detection in mono-repository
setups in order to determine which services or folders are candidate
for build and publish in CI/CD environments.

The program expects a list of changed/updated files on STDIN. These
files are the base for the change detection algorithm. The program
output will be all services/folders that have to be built, based on the
respective Depsfile files in each service folder.

For instance, you could pipe the git diff output to monodeps:

    git diff-tree --no-commit-id --name-only HEAD -r | monodeps"#,
        exec
    );

    print!("{}", opts.usage(&brief));
}
