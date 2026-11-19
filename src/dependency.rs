use std::collections::HashMap;
use std::path::Path;

use crate::cli::Opts;
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

    // 1. check global dependencies
    // if any changed file matches any global dependency every service will be returned
    for global_dep in opts.config.global_dependencies.iter() {
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
                changed_file.display_path
            );
        }
    }

    // 3. collect all services that have direct dependencies on the changed files
    updated.extend(check_direct_dependencies(
        &mut service_map,
        &canon_changed_files,
    )?);

    // 4. now gather all services that depend on the services that we already found.
    // we repeat this until we find no additional peer dependencies
    loop {
        updated = check_peer_dependencies(&mut service_map, &updated)?;
        if updated.is_empty() {
            break;
        }
    }

    // 5. return all services that have _some_ dependency
    Ok(service_map
        .into_values()
        .filter_map(|svc| if svc.has_trigger() { Some(svc) } else { None })
        .collect())
}

fn check_direct_dependencies(
    services: &mut HashMap<String, Service>,
    changed_files: &Vec<PathInfo>,
) -> Result<Vec<PathInfo>> {
    let mut changed = Vec::new();

    for service in (*services).values_mut() {
        if service.has_trigger() {
            continue;
        }

        if let Some((file_dependency, auto_dependency)) =
            service_has_dependency(service, changed_files)
        {
            changed.push(service.path.clone());
            service.trigger(BuildTrigger::Dependency(
                file_dependency.display_path.clone(),
                auto_dependency,
            ));
        }
    }

    Ok(changed)
}

fn check_peer_dependencies(
    services: &mut HashMap<String, Service>,
    changed_files: &Vec<PathInfo>,
) -> Result<Vec<PathInfo>> {
    let mut changed = Vec::new();

    for service in (*services).values_mut() {
        if service.has_trigger() {
            continue;
        }

        if let Some((file_dependency, auto_dependency)) =
            service_has_peer_dependency(service, changed_files)
        {
            changed.push(service.path.clone());
            service.trigger(BuildTrigger::PeerDependency(
                file_dependency.display_path.clone(),
                auto_dependency,
            ));
        }
    }

    Ok(changed)
}

fn service_has_peer_dependency<'a>(
    service: &Service,
    changed_files: &'a Vec<PathInfo>,
) -> Option<(&'a PathInfo, bool)> {
    for changed_file in changed_files {
        for dep in &service.depsfile.dependencies {
            if dep.is_matched_by(&changed_file.canonicalized) {
                // we found _some_ dependency on that service -> return early
                return Some((changed_file, false));
            }
        }

        for dep in &service.auto_dependencies {
            if dep.pattern.is_matched_by(&changed_file.canonicalized) {
                // we found _some_ dependency on that service -> return early
                return Some((changed_file, true));
            }
        }
    }

    None
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
    let Some(parent) = Path::new(&pattern.canonicalized).parent() else {
        return Ok(None);
    };
    let owner = owning_service(services, parent, &opts.target.canonicalized)?
        .map(|svc| svc.path.canonicalized.clone());

    Ok(owner.and_then(|owner| services.get_mut(&owner)).map(|svc| {
        svc.trigger(BuildTrigger::FileChange);
        svc.path.clone()
    }))
}

/// The innermost service containing `path` (or being `path` itself), not looking beyond `root`.
pub fn owning_service<'a>(
    services: &'a HashMap<String, Service>,
    path: &Path,
    root: &str,
) -> Result<Option<&'a Service>> {
    for ancestor in path.ancestors() {
        let str_path = ancestor
            .to_str()
            .ok_or_else(|| anyhow!("cannot determine parent path for {}", path.display()))?;

        if let Some(svc) = services.get(str_path) {
            return Ok(Some(svc));
        }

        if str_path == root {
            break;
        }
    }

    Ok(None)
}

/// All services `service` depends on, directly or transitively, using the same peer dependency
/// matching as `resolve`.
pub fn service_dependencies<'a>(
    services: &'a HashMap<String, Service>,
    service: &'a Service,
) -> Vec<&'a Service> {
    let mut found: Vec<&Service> = Vec::new();
    let mut pending = vec![service];

    while let Some(current) = pending.pop() {
        for other in services.values() {
            let known = other.path.canonicalized == service.path.canonicalized
                || found
                    .iter()
                    .any(|svc| svc.path.canonicalized == other.path.canonicalized);

            if !known && depends_on(current, &other.path.canonicalized) {
                found.push(other);
                pending.push(other);
            }
        }
    }

    found
}

fn depends_on(service: &Service, service_path: &str) -> bool {
    service
        .depsfile
        .dependencies
        .iter()
        .chain(service.auto_dependencies.iter().map(|auto| &auto.pattern))
        .any(|dep| dep.is_matched_by(service_path))
}
