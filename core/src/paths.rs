use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};

/// Returns the path of the application's SQLite database.
///
/// The database lives at
/// `~/Library/Application Support/tuclaw-desktop/tuclaw.sqlite`, and the
/// directory holding it is not created here.
///
/// # Errors
///
/// Returns an error if the `HOME` environment variable is unset.
///
/// # Examples
///
/// ```
/// use tuclaw_core::paths::database_path;
///
/// let path = database_path().unwrap();
/// assert!(path.ends_with("tuclaw-desktop/tuclaw.sqlite"));
/// ```
pub fn database_path() -> Result<PathBuf> {
    let home = env::var("HOME").context("HOME is not set, so the database path cannot be built")?;
    let path = PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("tuclaw-desktop")
        .join("tuclaw.sqlite");
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::database_path;

    #[test]
    fn the_path_sits_under_application_support() {
        let path = database_path().expect("HOME is set in the test environment");
        let path = path.to_string_lossy().into_owned();
        assert!(path.contains("/Library/Application Support/tuclaw-desktop/"));
        assert!(path.ends_with("tuclaw.sqlite"));
    }
}
