use std::collections::HashMap;

use crate::cli::Opts;
use crate::config::DepPattern;
use crate::path::PathInfo;
use crate::service::{BuildTrigger, Service};
use anyhow::{Result, anyhow};

pub fn resolve(
    mut services: Vec<Service>,
    changed_files: Vec<String>,
    opts: &Opts,
) -> Result<Vec<Service>> {
    let canon_changed_files: Vec<_> = changed_files
        .into_iter()
        .flat_map(|p| PathInfo::new(&p, &opts.target.canonicalized))
        .collect();

    if log::log_enabled!(log::Level::Debug) {
        for svc in &services {
            log::debug!("discovered service: {}", svc);
        }
    }

    // 1. check global dependencies
    // if any changed file matches any global dependency every service will be returned
    for global_dep in opts
        .config
        .global_dependencies
        .iter()
        .flat_map(|d| DepPattern::new(d, &opts.target.canonicalized))
    {
        if canon_changed_files
            .iter()
            .any(|f| global_dep.is_match(&f.canonicalized))
        {
            services
                .iter_mut()
                .for_each(|svc| svc.trigger(BuildTrigger::GlobalDependency));
            return Ok(services);
        }
    }

    let mut service_map: HashMap<String, Service> = services
        .into_iter()
        .map(|svc| (svc.path.canonicalized.clone(), svc))
        .collect();

    // 2. collect all services that are directly associated to the changed files
    let mut updated = Vec::new();

    for changed_file in &canon_changed_files {
        if let Some(svc) = check_file_dependency(&mut service_map, changed_file, opts)? {
            updated.push(svc);
        } else {
            log::warn!(
                "{}: cannot find associated service - ignoring",
                changed_file.path
            );
        }
    }

    // 3. collect all services that have direct dependencies on the changed files
    updated.extend(check_direct_dependencies(
        &mut service_map,
        &canon_changed_files,
        BuildTrigger::Dependency,
    )?);

    // 4. now gather all services that depend on the services that we already found.
    // we repeat this until we find no additional peer dependencies
    loop {
        updated =
            check_direct_dependencies(&mut service_map, &updated, BuildTrigger::PeerDependency)?;
        if updated.is_empty() {
            break;
        }
    }

    // 5. return all services that have _some_ dependency
    Ok(service_map
        .into_iter()
        .filter_map(|(_, svc)| if svc.has_trigger() { Some(svc) } else { None })
        .collect())
}

fn check_direct_dependencies<T>(
    services: &mut HashMap<String, Service>,
    changed_files: &Vec<PathInfo>,
    trigger: T,
) -> Result<Vec<PathInfo>>
where
    T: Fn(String, bool) -> BuildTrigger,
{
    let mut changed = Vec::new();

    for service in (*services).values_mut() {
        if service.has_trigger() {
            continue;
        }

        if let Some((file_dependency, auto_dependency)) =
            service_has_dependency(service, changed_files)
        {
            changed.push(service.path.clone());
            service.trigger(trigger(file_dependency.path.clone(), auto_dependency));
        }
    }

    Ok(changed)
}

fn service_has_dependency<'a>(
    service: &Service,
    changed_files: &'a Vec<PathInfo>,
) -> Option<(&'a PathInfo, bool)> {
    for changed_file in changed_files {
        for dep in &service.depsfile.dependencies {
            if dep.is_match(&changed_file.canonicalized) {
                // we found _some_ dependency on that service -> return early
                return Some((changed_file, false));
            }
        }

        for dep in &service.auto_dependencies {
            if dep.pattern.is_match(&changed_file.canonicalized) {
                // we found _some_ dependency on that service -> return early
                return Some((changed_file, true));
            }
        }
    }

    None
}

fn check_file_dependency(
    services: &mut HashMap<String, Service>,
    pattern: &PathInfo,
    opts: &Opts,
) -> Result<Option<PathInfo>> {
    let file_path = std::path::PathBuf::from(&pattern.canonicalized);

    for path in file_path.ancestors().skip(1) {
        let str_path = path
            .to_str()
            .ok_or_else(|| anyhow!("cannot determine parent path for {}", file_path.display()))?;

        if let Some(entry) = services.get_mut(str_path) {
            entry.trigger(BuildTrigger::FileChange);
            return Ok(Some(entry.path.clone()));
        }

        // only walk directories until the root project directory
        if str_path == opts.target.canonicalized {
            break;
        }
    }

    Ok(None)
}
