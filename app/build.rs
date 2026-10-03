use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=TUCLAW_COMMIT");
    println!("cargo:rerun-if-changed=../.git/HEAD");
    println!("cargo:rerun-if-changed=../.git/logs/HEAD");
    let commit = match std::env::var("TUCLAW_COMMIT") {
        Ok(commit) => commit,
        Err(_) => git_commit().unwrap_or_else(|| "unknown".to_string()),
    };
    println!("cargo:rustc-env=TUCLAW_COMMIT={commit}");
}

fn git_commit() -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let commit = String::from_utf8(output.stdout).ok()?;
    let commit = commit.trim();
    if commit.is_empty() {
        return None;
    }
    Some(commit.to_string())
}
