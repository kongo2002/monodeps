use std::collections::HashMap;

use crate::cli::Opts;
use crate::path::PathInfo;
use crate::service::{BuildTrigger, Service};
use anyhow::{Result, anyhow};

pub fn resolve(
    services: Vec<Service>,
    changed_files: Vec<String>,
    opts: &Opts,
) -> Result<Vec<Service>> {
    let mut service_map: HashMap<String, Service> = services
        .into_iter()
        .map(|svc| (svc.path.canonicalized.clone(), svc))
        .collect();

    // 1. collect all services that are directly associated to the changed files
    let canon_changed_files: Vec<_> = changed_files
        .into_iter()
        .flat_map(|p| PathInfo::new(&p, &opts.target.canonicalized))
        .collect();

    let mut updated = Vec::new();

    for changed_file in &canon_changed_files {
        if let Some(svc) = check_file_dependency(&mut service_map, changed_file)? {
            updated.push(svc);
        } else {
            eprintln!(
                "cannot find associated service for {} - ignoring",
                changed_file.path
            );
        }
    }

    // 2. collect all services that have direct dependencies on the changed files
    updated.extend(check_direct_dependencies(
        &mut service_map,
        &canon_changed_files,
        BuildTrigger::Dependency,
    )?);

    // 3. now gather all services that depend on the services that we already found
    loop {
        updated =
            check_direct_dependencies(&mut service_map, &updated, BuildTrigger::PeerDependency)?;
        if updated.is_empty() {
            break;
        }
    }

    // 3. return all services that have _some_ dependency
    Ok(service_map
        .into_iter()
        .filter_map(|(_, svc)| if svc.has_trigger() { Some(svc) } else { None })
        .collect())
}

fn check_direct_dependencies(
    services: &mut HashMap<String, Service>,
    changed_files: &Vec<PathInfo>,
    trigger: BuildTrigger,
) -> Result<Vec<PathInfo>> {
    let mut changed = Vec::new();

    for (_, service) in &mut *services {
        if service.has_trigger() {
            continue;
        }

        'outer: for changed_file in changed_files {
            for dep in &service.depsfile.dependencies {
                if dep.is_match(&changed_file.canonicalized) {
                    changed.push(service.path.clone());
                    break 'outer;
                }
            }
        }
    }

    for info in &changed {
        if let Some(entry) = services.get_mut(&info.canonicalized) {
            entry.trigger(trigger.clone());
        }
    }

    Ok(changed)
}

fn check_file_dependency(
    services: &mut HashMap<String, Service>,
    pattern: &PathInfo,
) -> Result<Option<PathInfo>> {
    let file_path = std::path::PathBuf::from(&pattern.canonicalized);

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
            return Ok(Some(entry.path.clone()));
        }
    }

    Ok(None)
}
