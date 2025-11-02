use anyhow::{Result, anyhow, bail};
use getopts::Options;

use crate::config::{Config, DepsfileType};
use crate::path::PathInfo;

pub enum OutputFormat {
    Plain,
    Json,
}

#[derive(Debug, PartialEq)]
pub enum Operation {
    Dependencies,
    Validate(String),
}

pub struct Opts {
    pub target: PathInfo,
    pub config: Config,
    pub output: OutputFormat,
    pub verbose: bool,
    pub supported_roots: Vec<DepsfileType>,
}

impl Opts {
    pub fn parse() -> Result<(Operation, Self)> {
        let args: Vec<_> = std::env::args().collect();
        Opts::parse_args(args)
    }

    fn parse_args(args: Vec<String>) -> Result<(Operation, Self)> {
        let mut opts = Options::new();
        opts.optopt("t", "target", "target directory to operate on", "DIR");
        opts.optopt("c", "config", "configuration file", "FILE");
        opts.optopt("o", "output", "output format", "FORMAT");
        opts.optflag("", "makefile", "accept 'Makefile' as project roots");
        opts.optflag("", "justfile", "accept 'justfile' as project roots");
        opts.optflag("v", "verbose", "verbose output");
        opts.optflag("h", "help", "show help");

        let matches = opts.parse(&args[1..])?;

        let operation = matches
            .free
            .first()
            .map(|operation_str| match operation_str.as_str() {
                "validate" => {
                    if matches.free.len() != 2 {
                        bail!("missing service path for 'validate'");
                    }
                    Ok(Operation::Validate(matches.free[1].clone()))
                }
                "dependencies" => Ok(Operation::Dependencies),
                unknown => {
                    bail!("unknown operation '{unknown}' [supported: validate, dependencies]")
                }
            })
            .unwrap_or(Ok(Operation::Dependencies))?;

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

        let mut supported_roots = vec![];

        if matches.opt_present("makefile") {
            supported_roots.push(DepsfileType::Makefile);
        }

        if matches.opt_present("justfile") {
            supported_roots.push(DepsfileType::Justfile);
        }

        Ok((
            operation,
            Self {
                target,
                config,
                output,
                verbose,
                supported_roots,
            },
        ))
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
        r#"Usage: {} [OPERATION] [OPTIONS]

monodeps is a tool to help with change detection in mono-repository
setups in order to determine which services or folders are candidate
for build and publish in CI/CD environments.

The program expects a list of changed/updated files on STDIN. These
files are the base for the change detection algorithm. The program
output will be all services/folders that have to be built, based on the
respective Depsfile files in each service folder.

For instance, you could pipe the git diff output to monodeps:

    git diff-tree --no-commit-id --name-only HEAD -r | monodeps

Operations:
    dependencies    determine dependencies (default)
    validate PATH   validate the given service"#,
        exec
    );

    print!("{}", opts.usage(&brief));
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use crate::cli::Operation;

    use super::Opts;

    fn args(additional_args: Vec<&str>) -> Result<(Operation, Opts)> {
        let all_args = Vec::from(["monodeps".to_string()])
            .into_iter()
            .chain(additional_args.iter().map(|val| val.to_string()))
            .collect();

        Opts::parse_args(all_args)
    }

    #[test]
    fn empty_args() -> Result<()> {
        let (operation, _opts) = args(vec![])?;

        assert_eq!(Operation::Dependencies, operation);

        Ok(())
    }

    #[test]
    fn operation_dependencies() -> Result<()> {
        let (operation, _opts) = args(vec!["dependencies"])?;

        assert_eq!(Operation::Dependencies, operation);

        Ok(())
    }

    #[test]
    fn operation_validate() -> Result<()> {
        let (operation, _opts) = args(vec!["validate", "something"])?;

        assert_eq!(Operation::Validate("something".to_string()), operation);

        Ok(())
    }

    #[test]
    fn operation_validate_error() -> Result<()> {
        let result = args(vec!["validate"]);

        assert!(result.is_err());

        Ok(())
    }

    #[test]
    fn operation_unknown() -> Result<()> {
        let result = args(vec!["whatever"]);

        assert!(result.is_err());

        Ok(())
    }
}
