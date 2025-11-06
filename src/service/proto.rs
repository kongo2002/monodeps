use std::path::Path;
use std::sync::OnceLock;

use anyhow::Result;
use walkdir::DirEntry;

use crate::config::DepPattern;
use crate::path::PathInfo;

use super::{non_hidden_files, read_lines};

const SCAN_MAX_LINES: usize = 200;

pub(super) struct ProtoAnalyzer {
    root: PathInfo,
    all_proto_files: OnceLock<Vec<PathInfo>>,
}

impl ProtoAnalyzer {
    pub(super) fn new(root: PathInfo) -> Self {
        let all_proto_files = OnceLock::new();

        Self {
            all_proto_files,
            root,
        }
    }

    pub(super) fn dependencies<P>(&self, dir: P) -> Result<Vec<DepPattern>>
    where
        P: AsRef<Path>,
    {
        let all_protos = self.proto_files();
        let mut dependencies = Vec::new();

        for entry in non_hidden_files(dir) {
            if !is_proto(&entry) {
                continue;
            }

            let imports = extract_proto_imports(entry.path(), all_protos)?;
            dependencies.extend(imports);
        }

        Ok(dependencies)
    }

    fn proto_files(&self) -> &Vec<PathInfo> {
        self.all_proto_files
            .get_or_init(|| try_find_all_proto_files(&self.root.canonicalized))
    }
}

fn extract_proto_imports<P>(path: P, proto_candidates: &[PathInfo]) -> Result<Vec<DepPattern>>
where
    P: AsRef<Path>,
{
    let mut scanned_lines = 0usize;
    let mut imports = Vec::new();

    let lines = read_lines(path)?.map_while(Result::ok);

    for line in lines {
        scanned_lines += 1;
        if scanned_lines > SCAN_MAX_LINES {
            break;
        }

        if !line.starts_with("import") {
            continue;
        }

        if let Some(import) = extract_from_import(&line, proto_candidates) {
            imports.push(import);
        }
    }

    Ok(imports)
}

fn extract_from_import(line: &str, proto_candidates: &[PathInfo]) -> Option<DepPattern> {
    let parts: Vec<_> = line.splitn(3, "\"").collect();
    if parts.len() != 3 {
        return None;
    }

    // TODO: support transitive dependencies
    let referenced_import = proto_candidates
        .iter()
        .find(|proto| proto.canonicalized.ends_with(parts[1]))?;

    DepPattern::new(
        &referenced_import.canonicalized,
        &referenced_import.canonicalized,
    )
    .ok()
}

fn try_find_all_proto_files(root_dir: &str) -> Vec<PathInfo> {
    find_all_proto_files(root_dir).unwrap_or_else(|_| Vec::new())
}

fn find_all_proto_files(root_dir: &str) -> Result<Vec<PathInfo>> {
    let mut proto_files = Vec::new();

    for entry in non_hidden_files(root_dir) {
        if !is_proto(&entry) {
            continue;
        }

        let path_info = PathInfo::new(entry.path(), root_dir)?;
        proto_files.push(path_info);
    }

    Ok(proto_files)
}

fn is_proto(entry: &DirEntry) -> bool {
    let extension = entry.path().extension();
    extension
        .filter(|ext| ext.eq_ignore_ascii_case("proto"))
        .is_some()
}
