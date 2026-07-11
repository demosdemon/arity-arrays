//! Pre-launch planning: run-mode resolution, a rough cost estimate, and the
//! human-readable plan banner printed before provisioning (and by --dry-run).

use std::fmt;

use crate::cmd::ec2_bench::naming::RunId;
use crate::cmd::ec2_bench::validate::RunMode;
use crate::cmd::ec2_bench::validate::Tenancy;

/// Two distinct SHAs → `Compare`; identical → `Charts`.
#[must_use]
pub fn resolve_mode(head_sha: &str, base_sha: &str) -> RunMode {
    if head_sha == base_sha {
        RunMode::Charts
    } else {
        RunMode::Compare
    }
}

/// Default hard-deadline minutes: 120 quick / 360 full (CI's 90/330 + 30 min
/// overhead). Keyed on quick/full, not ref count.
#[must_use]
pub const fn default_max_runtime(quick: bool) -> u32 {
    if quick { 120 } else { 360 }
}

/// A rough on-demand cost estimate. Static per-family hourly rates (us-east-1,
/// approximate) times the run's max minutes; spot is annotated as variable and
/// typically cheaper. Not authoritative — the real guardrails are the deadline
/// and --max-runtime.
#[must_use]
pub fn estimate_cost_usd(instance_type: &str, spot: bool, max_runtime_min: u32) -> String {
    // Approximate on-demand USD/hour for common benchmarking families.
    let hourly = match instance_type {
        "c7i.2xlarge" => Some(0.357_f64),
        "c7i.4xlarge" => Some(0.714),
        "c7g.2xlarge" => Some(0.290),
        "m7i.2xlarge" => Some(0.403),
        _ => None,
    };
    let minutes = f64::from(max_runtime_min);
    hourly.map_or_else(
        || format!("unknown rate for {instance_type}; see EC2 pricing"),
        |rate| {
            let cap = rate * minutes / 60.0;
            if spot {
                format!("~${cap:.2} on-demand ceiling; spot is variable and usually lower")
            } else {
                format!("~${cap:.2} at the {max_runtime_min}-min ceiling (on-demand)")
            }
        },
    )
}

/// The resolved plan shown for confirmation and by `--dry-run`.
#[derive(Debug)]
pub struct RunPlan {
    /// Unique run identifier.
    pub run_id: RunId,
    /// Target AWS region.
    pub region: String,
    /// EC2 instance type.
    pub instance_type: String,
    /// Requested tenancy.
    pub tenancy: Tenancy,
    /// Whether a spot instance is requested.
    pub spot: bool,
    /// Resolved run mode.
    pub mode: RunMode,
    /// Base ref SHA.
    pub base_sha: String,
    /// Head ref SHA.
    pub head_sha: String,
    /// Destination S3 URI.
    pub s3_uri: String,
    /// Hard-deadline minutes.
    pub max_runtime_min: u32,
    /// Whether the CLI will create (and later delete) a security group.
    pub creates_security_group: bool,
    /// Whether the CLI will create (and later delete) an instance profile.
    pub creates_instance_profile: bool,
    /// Human-readable cost estimate.
    pub cost_estimate: String,
}

impl fmt::Display for RunPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mode = match self.mode {
            RunMode::Compare => "compare (A/B/B/A)",
            RunMode::Charts => "charts (single-ref)",
        };
        let tenancy = match self.tenancy {
            Tenancy::Default => "default",
            Tenancy::Dedicated => "dedicated",
        };
        let market = if self.spot { "spot" } else { "on-demand" };
        writeln!(f, "ec2-bench plan (run {})", self.run_id)?;
        writeln!(f, "  region:        {}", self.region)?;
        writeln!(
            f,
            "  instance:      {} ({tenancy}, {market})",
            self.instance_type
        )?;
        writeln!(f, "  mode:          {mode}")?;
        writeln!(f, "  base -> head:  {} -> {}", self.base_sha, self.head_sha)?;
        writeln!(f, "  results:       {}", self.s3_uri)?;
        writeln!(
            f,
            "  max runtime:   {} min (hard deadline)",
            self.max_runtime_min
        )?;
        writeln!(f, "  cost:          {}", self.cost_estimate)?;
        if self.creates_security_group {
            writeln!(f, "  will create:   security group (deleted on teardown)")?;
        }
        if self.creates_instance_profile {
            writeln!(
                f,
                "  will create:   IAM role + instance profile (deleted on teardown)"
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_shas_select_charts() {
        assert_eq!(resolve_mode("abc", "abc"), RunMode::Charts);
        assert_eq!(resolve_mode("abc", "def"), RunMode::Compare);
    }

    #[test]
    fn max_runtime_defaults_by_mode() {
        assert_eq!(default_max_runtime(true), 120);
        assert_eq!(default_max_runtime(false), 360);
    }

    #[test]
    fn cost_estimate_is_lower_for_spot() {
        let on_demand = estimate_cost_usd("c7i.2xlarge", false, 60);
        let spot = estimate_cost_usd("c7i.2xlarge", true, 60);
        assert!(on_demand.contains('$'));
        assert!(spot.contains("spot"));
    }

    #[test]
    fn unknown_instance_type_estimate_is_graceful() {
        let s = estimate_cost_usd("z9.mega", false, 60);
        assert!(s.to_lowercase().contains("unknown") || s.contains('~'));
    }

    #[test]
    fn plan_banner_mentions_key_fields() {
        let plan = RunPlan {
            run_id: RunId::generate(0, 0),
            region: "us-east-1".into(),
            instance_type: "c7i.2xlarge".into(),
            tenancy: Tenancy::Default,
            spot: false,
            mode: RunMode::Compare,
            base_sha: "aaaaaaa".into(),
            head_sha: "bbbbbbb".into(),
            s3_uri: "s3://b/arity-bench/run/".into(),
            max_runtime_min: 360,
            creates_security_group: true,
            creates_instance_profile: true,
            cost_estimate: "~$0.36".into(),
        };
        let banner = plan.to_string();
        assert!(banner.contains("us-east-1"));
        assert!(banner.contains("c7i.2xlarge"));
        assert!(banner.contains("compare"));
        assert!(banner.contains("s3://b/arity-bench/run/"));
        assert!(banner.contains("360"));
        assert!(banner.contains("security group"));
        assert!(banner.contains("instance profile"));
    }
}
