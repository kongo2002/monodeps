use std::path::Path;

use crate::path::PathInfo;
use anyhow::Result;
use walkdir::{DirEntry, WalkDir};

#[derive(Debug)]
pub struct Service {
    pub depsfile: PathInfo,
    pub path: PathInfo,
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
                if let Some(service) = to_service(entry) {
                    all.push(service);
                }
            }
        }
        Ok(all)
    }
}

fn to_service(entry: DirEntry) -> Option<Service> {
    let depsfile = entry
        .path()
        .to_str()
        .and_then(|p| PathInfo::new(p.to_string()).ok())?;

    let path_buf = entry.into_path();
    let path = path_buf.parent().and_then(to_pathinfo)?;

    Some(Service { depsfile, path })
}

fn to_pathinfo(p: &Path) -> Option<PathInfo> {
    let path = p.to_str()?.to_string();

    PathInfo::new(path).ok()
}
