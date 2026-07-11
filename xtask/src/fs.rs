//! Thin `std::fs` wrappers that name the path in the error, so callers can
//! use `?` without losing which file was involved.

use std::path::Path;

use anyhow::Context;

pub fn read_to_string(path: impl AsRef<Path>) -> anyhow::Result<String> {
    let path = path.as_ref();
    std::fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))
}

pub fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> anyhow::Result<()> {
    let path = path.as_ref();
    std::fs::write(path, contents).with_context(|| format!("failed to write {}", path.display()))
}

pub fn create_dir_all(path: impl AsRef<Path>) -> anyhow::Result<()> {
    let path = path.as_ref();
    std::fs::create_dir_all(path)
        .with_context(|| format!("failed to create directory {}", path.display()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn read_error_names_the_path() {
        let err = super::read_to_string("definitely/not/here.json").unwrap_err();
        assert!(
            format!("{err:#}").contains("failed to read definitely/not/here.json"),
            "error should name the path: {err:#}"
        );
    }
}
