use std::{fs, path::PathBuf};

pub fn path() -> PathBuf {
    kikitori_fetch::receipt_path()
}
pub fn entries() -> Vec<(String, String)> {
    fs::read_to_string(path())
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let (kind, value) = line.split_once('\t')?;
            Some((kind.to_owned(), value.to_owned()))
        })
        .collect()
}
pub fn record(kind: &str, value: &str) -> Result<(), String> {
    kikitori_fetch::record_receipt(kind, value).map_err(|e| e.to_string())
}
pub fn home() -> PathBuf {
    kikitori_core::paths::home_dir()
}
