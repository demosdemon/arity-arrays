//! Thin `git` shell-outs used to resolve defaults on the operator's machine.
//! Only the output parsing is unit-tested; the process calls are exercised by
//! manual end-to-end runs.

use std::process::Command;

use anyhow::Context;

/// First non-empty trimmed line of command output, if any.
#[must_use]
pub fn parse_first_line(out: &str) -> Option<String> {
    out.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(ToOwned::to_owned)
}

fn run_git(args: &[&str]) -> anyhow::Result<String> {
    let out = Command::new("git")
        .args(args)
        .output()
        .with_context(|| format!("git {} failed to start", args.join(" ")))?;
    if !out.status.success() {
        anyhow::bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    parse_first_line(&String::from_utf8_lossy(&out.stdout))
        .with_context(|| format!("git {} produced no output", args.join(" ")))
}

/// Resolve a ref (branch/tag/SHA) to a full commit SHA.
pub fn resolve_sha(refname: &str) -> anyhow::Result<String> {
    run_git(&["rev-parse", "--verify", &format!("{refname}^{{commit}}")])
}

/// The current `HEAD` commit SHA.
pub fn head_sha() -> anyhow::Result<String> {
    resolve_sha("HEAD")
}

/// The `origin` remote's fetch URL.
pub fn origin_url() -> anyhow::Result<String> {
    run_git(&["remote", "get-url", "origin"])
}

/// Whether `sha` is contained in any `origin/*` remote-tracking branch — a
/// proxy for "reachable on the remote", since the instance clones origin.
/// Requires up-to-date remote refs locally.
pub fn remote_contains(sha: &str) -> anyhow::Result<bool> {
    let out = Command::new("git")
        .args(["branch", "-r", "--contains", sha])
        .output()
        .context("git branch -r --contains failed")?;
    Ok(out.status.success() && !String::from_utf8_lossy(&out.stdout).trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_line_trims_and_skips_blanks() {
        assert_eq!(
            parse_first_line("\n  abc123 \n def\n").as_deref(),
            Some("abc123")
        );
        assert_eq!(parse_first_line("   \n\n"), None);
    }
}
