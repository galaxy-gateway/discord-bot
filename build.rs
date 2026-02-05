// Build script to extract git commit information at compile time
use std::process::Command;

fn main() {
    // Extract last 5 git commits at compile time
    // Format: hash|commit message
    let output = Command::new("git")
        .args(["log", "--oneline", "-5", "--format=%h|%s"])
        .output();

    let commits = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    };

    println!("cargo:rustc-env=GIT_RECENT_COMMITS={}", commits);

    // Rerun build script when git state changes
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads/");
}
