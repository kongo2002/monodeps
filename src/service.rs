use std::fmt::Display;
use std::path::Path;

use crate::cli::Opts;
use crate::config::{DepPattern, Depsfile};
use crate::path::PathInfo;
use anyhow::Result;
use walkdir::{DirEntry, WalkDir};

mod go;

#[derive(Debug, Clone, PartialEq)]
pub enum BuildTrigger {
    FileChange,
    Dependency(String, bool),
    PeerDependency(String, bool),
}

#[derive(Debug)]
pub struct Service {
    pub path: PathInfo,
    pub depsfile: Depsfile,
    pub auto_dependencies: Vec<DepPattern>,
    triggers: Vec<BuildTrigger>,
}

impl Service {
    pub fn has_trigger(&self) -> bool {
        !self.triggers.is_empty()
    }

    pub fn trigger(&mut self, trigger: BuildTrigger) {
        if !self.triggers.contains(&trigger) {
            self.triggers.push(trigger)
        }
    }

    pub fn discover(opts: &Opts) -> Result<Vec<Service>> {
        let root_dir = &opts.target.canonicalized;
        let mut all = Vec::new();

        let walker = WalkDir::new(root_dir)
            .into_iter()
            // filter hidden files/directories
            .filter_entry(|e| {
                !e.file_name()
                    .to_str()
                    .map(|s| s.starts_with("."))
                    .unwrap_or(false)
            })
            // skip errors (e.g. non permission directories)
            .filter_map(|e| e.ok());

        for entry in walker {
            let filename = entry.file_name().to_str().unwrap_or("").to_lowercase();
            let is_depsfile = filename == "buildfile.yaml" || filename == "depsfile";

            if is_depsfile {
                if let Some((depsfile_location, path)) = get_locations(entry, root_dir) {
                    let depsfile = Depsfile::load(&depsfile_location.canonicalized, root_dir)?;

                    let mut auto_dependencies = Vec::new();

                    if !opts.config.auto_discovery.go.package_prefixes.is_empty() {
                        auto_dependencies.extend(go::dependencies(&path.canonicalized, &opts)?);
                    }

                    println!("auto disc: {}", auto_dependencies.len());

                    let service = Service {
                        path,
                        depsfile,
                        auto_dependencies,
                        triggers: Vec::new(),
                    };

                    all.push(service);
                }
            }
        }
        Ok(all)
    }
}

impl Display for Service {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{} [", self.path.canonicalized))?;

        let mut idx = 0;
        for trigger in &self.triggers {
            if idx > 0 {
                f.write_str(",")?;
            }
            f.write_fmt(format_args!("{:?}", trigger))?;
            idx += 1;
        }
        f.write_str("]")?;

        Ok(())
    }
}

fn get_locations(entry: DirEntry, root_dir: &str) -> Option<(PathInfo, PathInfo)> {
    let depsfile_location = entry
        .path()
        .to_str()
        .and_then(|p| PathInfo::new(p, root_dir).ok())?;

    let path_buf = entry.into_path();
    let path = path_buf.parent().and_then(|p| to_pathinfo(p, root_dir))?;

    Some((depsfile_location, path))
}

fn to_pathinfo(p: &Path, root_dir: &str) -> Option<PathInfo> {
    let path = p.to_str()?;

    PathInfo::new(path, root_dir).ok()
}
