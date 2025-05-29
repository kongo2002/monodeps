use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow};
use sxd_xpath::{Context, Factory, XPath};

use crate::cli::Opts;
use crate::config::DepPattern;

use super::non_hidden_files;

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

    pub fn dependencies<P>(&self, dir: P, config: &Opts) -> Result<Vec<DepPattern>>
    where
        P: AsRef<Path>,
    {
        let mut collected_imports = HashSet::new();

        for entry in non_hidden_files(&dir) {
            let filename = entry.file_name().to_str().unwrap_or("").to_lowercase();
            if !filename.ends_with(".csproj") {
                continue;
            }

            let file_content = std::fs::read_to_string(entry.path())?;

            // the XML parser does not support UTF8 BOM
            let bom_stripped = file_content.trim_start_matches("\u{feff}");
            let imports = self.extract_project_references(bom_stripped)?;

            collected_imports.extend(imports);
        }

        Ok(collected_imports
            .into_iter()
            .flat_map(|import| DepPattern::new(&import, dir.as_ref().to_str().unwrap()))
            .collect())
    }

    fn extract_project_references(&self, content: &str) -> Result<Vec<String>> {
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
                .collect(),
            _ => vec![],
        })
    }
}

/// Convert the project file reference to the service directory
/// e.g. '../Common.Logging/Common.Logging.csproj' -> '../Common.Logging'
fn extract_project_dir(include: &str) -> Option<String> {
    let alt_separator = if std::path::MAIN_SEPARATOR_STR == "\\" {
        "/"
    } else {
        "\\"
    };

    let sanitized = include.replace(alt_separator, std::path::MAIN_SEPARATOR_STR);
    PathBuf::from(sanitized)
        .ancestors()
        .skip(1)
        .next()
        .and_then(|p| p.to_str())
        .map(|p| p.to_string())
        .filter(|p| !p.is_empty())
}

#[cfg(test)]
mod tests {
    use super::DotnetAnalyzer;

    const CSPROJ01: &str = include_str!("../../tests/dotnet_proj01.csproj");

    #[test]
    fn extract_references() {
        let analyzer = DotnetAnalyzer::new().unwrap();
        let mut extracted = analyzer.extract_project_references(CSPROJ01).unwrap();

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
}
