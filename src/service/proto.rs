use std::path::Path;
use std::sync::OnceLock;

use anyhow::Result;

use crate::cli::Opts;
use crate::config::DepPattern;
use crate::path::PathInfo;

use super::index::RepoIndex;
use super::{LanguageAnalyzer, ReferenceLineFinder};

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

    fn proto_files(&self, index: &RepoIndex) -> &Vec<PathInfo> {
        self.all_proto_files
            .get_or_init(|| try_find_all_proto_files(index, &self.root.canonicalized))
    }
}

pub(super) fn file_relevant(file_name: &str) -> bool {
    file_name.ends_with(".proto")
}

impl LanguageAnalyzer for ProtoAnalyzer {
    fn dependencies(
        &self,
        files: &[&Path],
        _dir: &str,
        index: &RepoIndex,
        _opts: &Opts,
    ) -> Result<Vec<DepPattern>> {
        let all_protos = self.proto_files(index);
        let mut dependencies = Vec::new();

        for file in files {
            let imports = extract_proto_imports(*file, all_protos)?;
            dependencies.extend(imports);
        }

        Ok(dependencies)
    }
}

fn extract_proto_imports<P>(path: P, proto_candidates: &[PathInfo]) -> Result<Vec<DepPattern>>
where
    P: AsRef<Path>,
{
    ReferenceLineFinder::new().extract_from(path, &|line, _parent| {
        extract_from_line(&line, proto_candidates)
    })
}

fn extract_from_line(line: &str, proto_candidates: &[PathInfo]) -> Option<DepPattern> {
    if !line.starts_with("import") {
        return None;
    }

    let parts: Vec<_> = line.splitn(3, "\"").collect();
    if parts.len() != 3 {
        return None;
    }

    let referenced_import = proto_candidates
        .iter()
        .find(|proto| proto.canonicalized.ends_with(parts[1]))?;

    DepPattern::plain(
        &referenced_import.canonicalized,
        &referenced_import.canonicalized,
    )
    .ok()
}

fn try_find_all_proto_files(index: &RepoIndex, root_dir: &str) -> Vec<PathInfo> {
    find_all_proto_files(index, root_dir).unwrap_or_else(|_| Vec::new())
}

fn find_all_proto_files(index: &RepoIndex, root_dir: &str) -> Result<Vec<PathInfo>> {
    index
        .files()
        .iter()
        .filter(|file| file.name.ends_with(".proto"))
        .map(|file| PathInfo::new(&file.path, root_dir))
        .collect()
}
