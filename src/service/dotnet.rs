use std::path::PathBuf;
use std::sync::OnceLock;

use anyhow::{Result, anyhow};
use sxd_xpath::{Context, Factory, XPath};
use walkdir::DirEntry;

use crate::cli::Opts;
use crate::config::DepPattern;
use crate::path::PathInfo;
use crate::service::parent_dir;

use super::{LanguageAnalyzer, non_hidden_files, parents_until_root};

const DIRECTORY_FILES: [DirectoryFile; 3] = [
    DirectoryFile::BuildProps,
    DirectoryFile::BuildTargets,
    DirectoryFile::PackagesProps,
];

#[derive(Clone, PartialEq, Debug)]
enum DirectoryFile {
    BuildProps,
    BuildTargets,
    PackagesProps,
}

impl DirectoryFile {
    fn filename(&self) -> &str {
        match self {
            DirectoryFile::BuildProps => "Directory.Build.props",
            DirectoryFile::BuildTargets => "Directory.Build.targets",
            DirectoryFile::PackagesProps => "Directory.Packages.props",
        }
    }
}

struct Import {
    service_dir: String,
    name: String,
}

pub(super) struct DotnetAnalyzer {
    root: PathInfo,
    proj_refs: XPath,
    directory_files: OnceLock<Vec<(DirectoryFile, PathBuf)>>,
}

impl DotnetAnalyzer {
    pub fn new(root: PathInfo) -> Result<Self> {
        let factory = Factory::new();
        let proj_refs = factory
            .build("//ProjectReference[@Include]/@Include")?
            .ok_or(anyhow!("failed to construct XML selector"))?;
        let directory_files = OnceLock::new();

        Ok(Self {
            root,
            proj_refs,
            directory_files,
        })
    }

    fn directory_files(&self) -> &Vec<(DirectoryFile, PathBuf)> {
        self.directory_files
            .get_or_init(|| try_find_all_directory_files(&self.root.canonicalized))
    }

    fn extract_project_references(
        &self,
        content: &str,
        package_namespaces: &[String],
    ) -> Result<Vec<String>> {
        let parsed_xml = sxd_document::parser::parse(content)?;
        let xml_doc = parsed_xml.as_document();

        let context = Context::new();
        let proj_ref = self.proj_refs.evaluate(&context, xml_doc.root())?;

        Ok(match proj_ref {
            sxd_xpath::Value::Nodeset(nodeset) => nodeset
                .into_iter()
                .flat_map(|node| {
                    node.attribute()
                        .and_then(|attr| extract_project_dir(attr.value()))
                })
                .filter(|import| {
                    package_namespaces.is_empty()
                        || package_namespaces
                            .iter()
                            .any(|package| import.name.starts_with(package))
                })
                .map(|import| import.service_dir)
                .collect(),
            _ => vec![],
        })
    }

    fn collect_directory_file_dependencies(
        &self,
        dir: &str,
        opts: &Opts,
    ) -> Result<Vec<DepPattern>> {
        let mut collected = Vec::new();
        let dir_files = self.directory_files();

        for directory in DIRECTORY_FILES {
            for parent_dir in parents_until_root(dir, &opts.target) {
                let exists = dir_files
                    .iter()
                    .find(|(dir_file, dir)| *dir_file == directory && *dir == parent_dir);

                if let Some((dir_file, directory_path)) = exists {
                    collected.push(DepPattern::new(
                        directory_path.join(dir_file.filename()),
                        dir,
                    )?);

                    // we take the first match that is closest to the service's directory
                    break;
                }
            }
        }

        Ok(collected)
    }
}

fn try_find_all_directory_files(root_dir: &str) -> Vec<(DirectoryFile, PathBuf)> {
    find_all_directory_files(root_dir).unwrap_or_else(|_| Vec::new())
}

fn find_all_directory_files(root_dir: &str) -> Result<Vec<(DirectoryFile, PathBuf)>> {
    let mut proto_files = Vec::new();

    for entry in non_hidden_files(root_dir) {
        if let Some(found) = to_directory_file(&entry) {
            proto_files.push(found);
        }
    }

    Ok(proto_files)
}

fn to_directory_file(entry: &DirEntry) -> Option<(DirectoryFile, PathBuf)> {
    let filename = entry.file_name();

    DIRECTORY_FILES
        .iter()
        .flat_map(|file| {
            if filename.eq(file.filename()) {
                let directory = parent_dir(entry.path())?;
                Some((file.clone(), directory))
            } else {
                None
            }
        })
        .next()
}

impl LanguageAnalyzer for DotnetAnalyzer {
    fn file_relevant(&self, file_name: &str) -> bool {
        file_name.ends_with(".csproj")
    }

    fn dependencies(
        &self,
        entries: Vec<DirEntry>,
        dir: &str,
        opts: &Opts,
    ) -> Result<Vec<DepPattern>> {
        let mut collected_imports = Vec::new();

        for entry in entries {
            if log::log_enabled!(log::Level::Debug) {
                log::debug!(
                    "dotnet: analyzing C# project file '{}'",
                    entry.path().display()
                );
            }

            let file_content = std::fs::read_to_string(entry.path())?;

            // the XML parser does not support UTF8 BOM
            let bom_stripped = file_content.trim_start_matches("\u{feff}");
            let imports = self.extract_project_references(
                bom_stripped,
                &opts.config.auto_discovery.dotnet.package_namespaces,
            )?;

            collected_imports.extend(imports.into_iter().flat_map(|import| {
                parent_dir(entry.path())
                    .and_then(|project_dir| DepPattern::new(&import, &project_dir).ok())
            }));
        }

        collected_imports.extend(self.collect_directory_file_dependencies(dir, opts)?);

        Ok(collected_imports)
    }
}

/// Convert the project file reference to the service directory
/// e.g. '../Common.Logging/Common.Logging.csproj' -> '../Common.Logging'
fn extract_project_dir(include: &str) -> Option<Import> {
    let alt_separator = if std::path::MAIN_SEPARATOR_STR == "\\" {
        "/"
    } else {
        "\\"
    };

    let sanitized = include.replace(alt_separator, std::path::MAIN_SEPARATOR_STR);

    let path = PathBuf::from(sanitized);
    let service_dir = path
        .ancestors()
        .nth(1)
        .and_then(|p| p.to_str())
        .map(|p| p.to_string())
        .filter(|p| !p.is_empty())?;

    let name = path.file_stem()?.to_str()?.to_string();

    Some(Import { service_dir, name })
}

#[cfg(test)]
mod tests {
    use crate::path::PathInfo;

    use super::DotnetAnalyzer;

    const CSPROJ01: &str = include_str!("../../tests/resources/dotnet_proj01.csproj");

    #[test]
    fn extract_references() {
        let namespaces = vec![];
        let root = PathInfo::new(".", ".").unwrap();
        let analyzer = DotnetAnalyzer::new(root).unwrap();
        let mut extracted = analyzer
            .extract_project_references(CSPROJ01, &namespaces)
            .unwrap();

        // for stable comparison
        extracted.sort();

        assert_eq!(extracted.len(), 2, "number of project references");
        assert_eq!(
            extracted,
            vec![
                String::from("../Common.Logging"),
                String::from("../Common.Tracing"),
            ]
        );
    }

    #[test]
    fn filter_references_by_namespaces() {
        let root = PathInfo::new(".", ".").unwrap();
        let namespaces = vec![String::from("Common.Logging")];
        let analyzer = DotnetAnalyzer::new(root).unwrap();
        let mut extracted = analyzer
            .extract_project_references(CSPROJ01, &namespaces)
            .unwrap();

        // for stable comparison
        extracted.sort();

        assert_eq!(extracted.len(), 1, "number of project references");
        assert_eq!(extracted, vec![String::from("../Common.Logging"),]);
    }
}
