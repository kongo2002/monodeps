use std::collections::HashMap;

use crate::path::PathInfo;
use crate::service::{BuildTrigger, Service};
use anyhow::{Result, anyhow};

pub fn resolve(services: Vec<Service>, changed_files: Vec<String>) -> Result<Vec<Service>> {
    let mut service_map: HashMap<String, Service> = services
        .into_iter()
        .map(|svc| (svc.path.canonicalized.clone(), svc))
        .collect();

    let canon_changed_files: Vec<_> = changed_files.into_iter().flat_map(PathInfo::new).collect();

    for changed_file in canon_changed_files {
        if !check_file_dependency(&mut service_map, &changed_file.canonicalized)? {
            println!(
                "cannot find associated service for {} - ignoring",
                changed_file.path
            );
        }
    }

    Ok(service_map
        .into_iter()
        .filter_map(|(_, svc)| if svc.has_trigger() { Some(svc) } else { None })
        .collect())
}

fn check_file_dependency(services: &mut HashMap<String, Service>, file: &str) -> Result<bool> {
    let file_path = std::path::PathBuf::from(file);

    // TODO: only walk directories until the root project directory
    for path in file_path.ancestors().skip(1) {
        let str_path = path.to_str().ok_or_else(|| {
            anyhow!(
                "cannot determine parent path for {}",
                file_path.to_string_lossy()
            )
        })?;

        if let Some(entry) = services.get_mut(str_path) {
            entry.trigger(BuildTrigger::FileChange);
            return Ok(true);
        }
    }

    Ok(false)
}
