use std::fmt::Display;
use std::path::Path;

use crate::config::Depsfile;
use crate::path::PathInfo;
use anyhow::Result;
use walkdir::{DirEntry, WalkDir};

#[derive(Debug)]
pub struct Service {
    pub path: PathInfo,
    pub depsfile: Depsfile,
    depsfile_location: PathInfo,
}

impl Service {
    pub fn discover<P: AsRef<Path>>(path: P) -> Result<Vec<Service>> {
        let mut all = Vec::new();

        let walker = WalkDir::new(path)
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
                if let Some((depsfile_location, path)) = get_locations(entry) {
                    let depsfile = Depsfile::load(&depsfile_location.canonicalized)?;
                    let service = Service {
                        path,
                        depsfile,
                        depsfile_location,
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
        f.write_fmt(format_args!("{}:", self.path.canonicalized))?;
        for dep in &self.depsfile.dependencies {
            f.write_str("\n")?;
            f.write_fmt(format_args!("  - {}", dep))?;
        }

        Ok(())
    }
}

fn get_locations(entry: DirEntry) -> Option<(PathInfo, PathInfo)> {
    let depsfile_location = entry
        .path()
        .to_str()
        .and_then(|p| PathInfo::new(p.to_string()).ok())?;

    let path_buf = entry.into_path();
    let path = path_buf.parent().and_then(to_pathinfo)?;

    Some((depsfile_location, path))
}

fn to_pathinfo(p: &Path) -> Option<PathInfo> {
    let path = p.to_str()?.to_string();

    PathInfo::new(path).ok()
}
