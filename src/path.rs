use anyhow::{Result, anyhow};

#[derive(Debug)]
pub struct PathInfo {
    pub path: String,
    pub canonicalized: String,
}

impl PathInfo {
    pub fn new(path: String) -> Result<Self> {
        let canonicalized = canonicalize(&path)?;
        Ok(Self {
            path,
            canonicalized,
        })
    }
}

fn canonicalize(path: &String) -> Result<String> {
    let canonicalized = std::fs::canonicalize(path)?;
    let canonical_str = canonicalized
        .to_str()
        .ok_or(anyhow!("cannot convert file path to string"))?;

    Ok(canonical_str.to_owned())
}
