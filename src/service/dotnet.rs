use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use sxd_xpath::{Context, Factory, XPath};

use crate::cli::Opts;
use crate::config::DepPattern;
use crate::service::parent_dir;

use super::non_hidden_files;

struct Import {
    service_dir: String,
    name: String,
}

pub(super) struct DotnetAnalyzer {
    proj_refs: XPath,
}

impl DotnetAnalyzer {
    pub fn new() -> Result<Self> {
        let factory = Factory::new();
        let proj_refs = factory
            .build("//ProjectReference[@Include]/@Include")?
            .ok_or(anyhow!("failed to construct XML selector"))?;

        Ok(Self { proj_refs })
    }

    pub fn dependencies<P>(&self, dir: P, opts: &Opts) -> Result<Vec<DepPattern>>
    where
        P: AsRef<Path>,
    {
        let mut collected_imports = Vec::new();

        for entry in non_hidden_files(&dir) {
            let extension = entry.path().extension();
            if extension
                .filter(|ext| ext.eq_ignore_ascii_case("csproj"))
                .is_none()
            {
                continue;
            }

            log::debug!(
                "dotnet: analyzing C# project file '{}'",
                entry.path().display()
            );

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

        Ok(collected_imports)
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
    use super::DotnetAnalyzer;

    const CSPROJ01: &str = include_str!("../../tests/dotnet_proj01.csproj");

    #[test]
    fn extract_references() {
        let namespaces = vec![];
        let analyzer = DotnetAnalyzer::new().unwrap();
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
        let namespaces = vec![String::from("Common.Logging")];
        let analyzer = DotnetAnalyzer::new().unwrap();
        let mut extracted = analyzer
            .extract_project_references(CSPROJ01, &namespaces)
            .unwrap();

        // for stable comparison
        extracted.sort();

        assert_eq!(extracted.len(), 1, "number of project references");
        assert_eq!(extracted, vec![String::from("../Common.Logging"),]);
    }
}
