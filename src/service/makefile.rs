use std::path::Path;

use regex::Regex;
use walkdir::DirEntry;

use anyhow::Result;

use crate::cli::Opts;
use crate::config::DepPattern;

use super::{LanguageAnalyzer, ReferenceFinder};

pub(super) struct MakefileAnalyzer {
    variable_rgx: Regex,
}

impl MakefileAnalyzer {
    pub fn new() -> Result<MakefileAnalyzer> {
        let variable_rgx = Regex::new(r"\$\([^)]+\)")?;

        Ok(Self { variable_rgx })
    }

    fn extract_imports<P>(&self, path: P) -> Result<Vec<DepPattern>>
    where
        P: AsRef<Path>,
    {
        let mut finder = ReferenceFinder::new();

        finder.extract_from(path, &|line, parent_dir| {
            self.extract_from_line(&line, parent_dir)
        })
    }

    fn extract_from_line(&self, line: &str, dir: &Path) -> Vec<DepPattern> {
        if !line.starts_with("include") {
            return Vec::new();
        }

        line.split(" ")
            .skip(1)
            .flat_map(|include_path| {
                // we skip include paths that include a Makefile variable (e.g. `$(FOOBAR)`)
                if !self.variable_rgx.is_match(include_path) {
                    DepPattern::plain(include_path, dir).ok()
                } else {
                    None
                }
            })
            .collect()
    }
}

impl LanguageAnalyzer for MakefileAnalyzer {
    fn file_relevant(&self, file_name: &str) -> bool {
        file_name == "makefile"
    }

    fn dependencies(
        &self,
        entries: Vec<DirEntry>,
        _dir: &str,
        _opts: &Opts,
    ) -> Result<Vec<DepPattern>> {
        let mut dependencies = Vec::new();

        for entry in entries {
            dependencies.extend(self.extract_imports(entry.path())?);
        }

        Ok(dependencies)
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use std::path::Path;

    use crate::config::DepPattern;

    use super::MakefileAnalyzer;

    fn from_line(line: &str) -> Result<Vec<DepPattern>> {
        let analyzer = MakefileAnalyzer::new()?;
        let path = Path::new(".");

        let patterns = analyzer.extract_from_line(line, path);
        Ok(patterns)
    }

    #[test]
    fn extract_no_include() -> Result<()> {
        let extract = from_line(".PHONY: test")?;

        assert_eq!(0, extract.len());
        Ok(())
    }

    #[test]
    fn extract_basic_include() -> Result<()> {
        let extract = from_line("include ../foo.mk")?;

        assert_eq!(1, extract.len());
        Ok(())
    }

    #[test]
    fn extract_multiple_includes() -> Result<()> {
        let extract = from_line("include ../foo.mk ../bar/Makefile")?;

        assert_eq!(2, extract.len());
        Ok(())
    }

    #[test]
    fn extract_exclude_variables() -> Result<()> {
        let extract = from_line("include $(ROOT_DIR)/include.mk")?;

        assert_eq!(0, extract.len());
        Ok(())
    }
}
