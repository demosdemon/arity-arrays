//! Validation of user-supplied CLI inputs. All values here reach the
//! user-data script or AWS APIs, so validation is also a security boundary
//! (see `userdata.rs` for how values are transported without injection).

use anyhow::Context;

/// Instance tenancy requested for the launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tenancy {
    /// Shared-tenancy (default) hardware.
    Default,
    /// Dedicated tenancy: no other AWS customers on the physical host.
    Dedicated,
}

/// Which rendering the run produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RunMode {
    /// Two distinct refs: interleaved A/B/B/A `xtask compare` delta table.
    Compare,
    /// One ref (head == base): `xtask charts` output.
    Charts,
}

/// Split a `KEY=VALUE` billing tag. The value may itself contain `=`.
pub fn parse_tag(raw: &str) -> anyhow::Result<(String, String)> {
    let (key, value) = raw
        .split_once('=')
        .with_context(|| format!("tag {raw:?} must be KEY=VALUE"))?;
    if key.is_empty() {
        anyhow::bail!("tag {raw:?} has an empty key");
    }
    Ok((key.to_owned(), value.to_owned()))
}

/// Reject tag keys that collide with AWS's reserved `aws:` prefix or the
/// tool's managed keys (`ManagedBy`, `RunId`) — a collision would corrupt
/// tag-based teardown discovery and the destructive-action IAM condition.
pub fn validate_tag_key(key: &str) -> anyhow::Result<()> {
    let lower = key.to_ascii_lowercase();
    if lower.starts_with("aws:") {
        anyhow::bail!("tag key {key:?} uses the reserved aws: prefix");
    }
    if lower == "managedby" || lower == "runid" {
        anyhow::bail!("tag key {key:?} collides with a managed tag key");
    }
    Ok(())
}

/// Accept `nightly` or a pinned `nightly-YYYY-MM-DD`; reject everything else.
pub fn validate_toolchain(tc: &str) -> anyhow::Result<()> {
    if tc == "nightly" {
        return Ok(());
    }
    if let Some(date) = tc.strip_prefix("nightly-") {
        let bytes = date.as_bytes();
        let shape_ok = bytes.len() == 10
            && bytes[4] == b'-'
            && bytes[7] == b'-'
            && date
                .char_indices()
                .filter(|&(i, _)| i != 4 && i != 7)
                .all(|(_, c)| c.is_ascii_digit());
        if shape_ok {
            return Ok(());
        }
    }
    anyhow::bail!("toolchain {tc:?} must be `nightly` or `nightly-YYYY-MM-DD`")
}

/// True for burstable (`t`-family) instance types, which have variable CPU
/// performance unsuitable for benchmarking.
#[must_use]
pub fn is_burstable(instance_type: &str) -> bool {
    // t2/t3/t3a/t4g/... — the family is the segment before the first '.'.
    instance_type.split('.').next().is_some_and(|family| {
        family.starts_with('t') && family[1..].starts_with(|c: char| c.is_ascii_digit())
    })
}

/// Reject the unsupported spot + dedicated-tenancy combination.
pub fn check_market(spot: bool, tenancy: Tenancy) -> anyhow::Result<()> {
    if spot && tenancy == Tenancy::Dedicated {
        anyhow::bail!("--spot is incompatible with --tenancy dedicated");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_key_value() {
        assert_eq!(
            parse_tag("Team=perf").unwrap(),
            ("Team".into(), "perf".into())
        );
        // Values may contain '='.
        assert_eq!(
            parse_tag("expr=a=b").unwrap(),
            ("expr".into(), "a=b".into())
        );
    }

    #[test]
    fn rejects_malformed_tag() {
        assert!(parse_tag("noequals").is_err());
        assert!(parse_tag("=novalue").is_err());
    }

    #[test]
    fn rejects_reserved_and_managed_keys() {
        assert!(validate_tag_key("Team").is_ok());
        assert!(validate_tag_key("aws:cost").is_err());
        assert!(validate_tag_key("AWS:Cost").is_err());
        assert!(validate_tag_key("ManagedBy").is_err());
        assert!(validate_tag_key("managedby").is_err());
        assert!(validate_tag_key("RunId").is_err());
        assert!(validate_tag_key("runid").is_err());
    }

    #[test]
    fn validates_toolchain() {
        assert!(validate_toolchain("nightly").is_ok());
        assert!(validate_toolchain("nightly-2026-07-09").is_ok());
        assert!(validate_toolchain("stable").is_err());
        assert!(validate_toolchain("nightly-2026-7-9").is_err());
        assert!(validate_toolchain("nightly-2026-07-09; rm -rf /").is_err());
    }

    #[test]
    fn detects_burstable() {
        assert!(is_burstable("t3.micro"));
        assert!(is_burstable("t4g.large"));
        assert!(!is_burstable("c7i.2xlarge"));
        assert!(!is_burstable("m7g.xlarge"));
    }

    #[test]
    fn spot_and_dedicated_conflict() {
        assert!(check_market(false, Tenancy::Dedicated).is_ok());
        assert!(check_market(true, Tenancy::Default).is_ok());
        assert!(check_market(true, Tenancy::Dedicated).is_err());
    }

    #[test]
    fn run_mode_variants_are_distinct() {
        // No behavior to exercise yet (later CLI work selects a variant from
        // ref-count), but this keeps the type live under `cfg(test)` so the
        // module-level dead-code suppression's `not(test)` gate holds.
        assert_ne!(RunMode::Compare, RunMode::Charts);
    }
}
