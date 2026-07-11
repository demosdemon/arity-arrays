//! AWS EC2 benchmark runner (`xtask ec2-bench`), behind the `ec2-bench`
//! feature. See `xtask/README.md`.

mod aws;
mod git;
mod iam;
mod naming;
mod plan;
mod poll;
mod provision;
mod run;
mod teardown;
mod userdata;
mod validate;

use std::future::Future;

use clap::Args;
use clap::Subcommand;

use crate::cmd::ec2_bench::naming::RunId;
use crate::cmd::ec2_bench::naming::default_output_dir;
use crate::cmd::ec2_bench::naming::default_s3_prefix;
use crate::cmd::ec2_bench::plan::RunPlan;
use crate::cmd::ec2_bench::plan::default_max_runtime;
use crate::cmd::ec2_bench::plan::estimate_cost_usd;
use crate::cmd::ec2_bench::plan::resolve_mode;
use crate::cmd::ec2_bench::validate::RunMode;
use crate::cmd::ec2_bench::validate::Tenancy;
use crate::cmd::ec2_bench::validate::check_market;
use crate::cmd::ec2_bench::validate::is_burstable;
use crate::cmd::ec2_bench::validate::parse_tag;
use crate::cmd::ec2_bench::validate::validate_tag_key;
use crate::cmd::ec2_bench::validate::validate_toolchain;

#[derive(Debug, Subcommand)]
/// Run the A/B/B/A benchmark on a dedicated EC2 instance
pub enum Ec2Bench {
    /// Provision, run, collect, and tear down a benchmark run.
    Run(Box<RunArgs>),
    /// Idempotently delete resources left by a run.
    Teardown(TeardownArgs),
}

/// Flags for `ec2-bench run`.
#[derive(Debug, Args, Default)]
#[expect(clippy::struct_excessive_bools)]
pub struct RunArgs {
    /// Head ref (default: HEAD).
    #[arg(long)]
    pub head: Option<String>,
    /// Base ref (default: origin/main).
    #[arg(long)]
    pub base: Option<String>,
    /// Remote to clone (default: origin URL).
    #[arg(long)]
    pub repo_url: Option<String>,
    /// Rust toolchain: `nightly` or `nightly-YYYY-MM-DD`.
    #[arg(long, default_value = "nightly")]
    pub toolchain: String,
    /// Reduce sample count and skip the interleave replicate.
    #[arg(long)]
    pub quick: bool,
    /// AWS profile override.
    #[arg(long)]
    pub profile: Option<String>,
    /// AWS region override.
    #[arg(long)]
    pub region: Option<String>,
    /// VPC id (default: the account's default VPC).
    #[arg(long)]
    pub vpc_id: Option<String>,
    /// Subnet id (default: first suitable subnet).
    #[arg(long)]
    pub subnet_id: Option<String>,
    /// Existing security group (default: create an egress-only one).
    #[arg(long)]
    pub security_group_id: Option<String>,
    /// Existing instance profile (default: create a scoped one).
    #[arg(long)]
    pub instance_profile: Option<String>,
    /// EC2 instance type.
    #[arg(long, default_value = "c7i.2xlarge")]
    pub instance_type: String,
    /// Tenancy: `default` or `dedicated`.
    #[arg(long, default_value = "default")]
    pub tenancy: String,
    /// Launch a spot instance.
    #[arg(long)]
    pub spot: bool,
    /// AMI id (default: resolved from SSM for Ubuntu 26.04).
    #[arg(long)]
    pub ami_id: Option<String>,
    /// Destination S3 bucket (required).
    #[arg(long)]
    pub s3_bucket: String,
    /// Destination S3 prefix (default: `arity-bench/<run-id>/`).
    #[arg(long)]
    pub s3_prefix: Option<String>,
    /// Billing tag KEY=VALUE (repeatable).
    #[arg(long = "tag")]
    pub tags: Vec<String>,
    /// Hard wall-clock cap in minutes (default: 120 quick / 360 full).
    #[arg(long)]
    pub max_runtime: Option<u32>,
    /// Directory for downloaded artifacts (default: `bench-data/ec2-<run-id>`).
    #[arg(long)]
    pub output_dir: Option<std::path::PathBuf>,
    /// Print the plan and exit; create nothing.
    #[arg(long)]
    pub dry_run: bool,
    /// Skip the confirmation prompt.
    #[arg(long)]
    pub yes: bool,
    /// Retain auto-created resources for inspection.
    #[arg(long)]
    pub keep: bool,
}

/// Flags for `ec2-bench teardown`.
#[derive(Debug, Args)]
pub struct TeardownArgs {
    /// Run id whose resources to delete.
    #[arg(long)]
    pub run_id: String,
    /// AWS profile override.
    #[arg(long)]
    pub profile: Option<String>,
    /// AWS region override.
    #[arg(long)]
    pub region: Option<String>,
}

impl Ec2Bench {
    pub fn run(&self) -> anyhow::Result<()> {
        match self {
            Self::Run(args) => args.run(),
            Self::Teardown(args) => args.run(),
        }
    }
}

impl RunArgs {
    fn resolve_args(&self) -> anyhow::Result<RunConfig> {
        let repo_url = match &self.repo_url {
            Some(u) => u.clone(),
            None => git::origin_url()?,
        };
        let head_sha = match &self.head {
            Some(r) => git::resolve_sha(r)?,
            None => git::head_sha()?,
        };
        let base_sha = match &self.base {
            Some(r) => git::resolve_sha(r)?,
            None => git::resolve_sha("origin/main")?,
        };
        let tooling_sha = git::head_sha()?;
        let run_id = RunId::generate(now_epoch_secs(), now_salt());
        RunConfig::build(self, repo_url, base_sha, head_sha, tooling_sha, run_id)
    }

    pub fn run(&self) -> anyhow::Result<()> {
        let cfg = self.resolve_args()?;
        if is_burstable(&cfg.instance_type) {
            eprintln!(
                "warning: {} is burstable; CPU performance will be noisy",
                cfg.instance_type
            );
        }
        eprintln!("mode: {:?}", cfg.mode);
        print!("{}", cfg.plan());
        if self.dry_run {
            return Ok(());
        }
        if !self.yes && !confirm() {
            anyhow::bail!("aborted");
        }
        for sha in cfg.repo_hashes() {
            match git::remote_contains(sha) {
                Ok(true) => {}
                Ok(false) => anyhow::bail!("{sha} is not reachable on origin; push it first"),
                Err(e) => eprintln!("warning: could not verify {sha} on origin: {e}"),
            }
        }
        block_on(async {
            let aws = aws::RealAws::new(cfg.profile.clone(), cfg.region.clone()).await?;
            run::run(&aws, &cfg).await
        })
    }
}

impl TeardownArgs {
    pub fn run(&self) -> anyhow::Result<()> {
        block_on(async {
            let aws = aws::RealAws::new(self.profile.clone(), self.region.clone()).await?;
            let td = teardown::TeardownInput {
                run_id: self.run_id.clone(),
                resource_name: format!("arity-ec2-bench-{}", self.run_id),
                delete_security_group: true,
                delete_iam: true,
            };
            teardown::teardown(&aws, &td).await
        })
    }
}

/// Drive an ec2-bench future to completion on a fresh current-thread runtime.
fn block_on<T>(fut: impl Future<Output = anyhow::Result<T>>) -> anyhow::Result<T> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(fut)
}

/// A fully-validated launch configuration.
#[derive(Debug)]
#[expect(clippy::struct_excessive_bools)]
pub struct RunConfig {
    run_id: RunId,
    repo_url: String,
    base_sha: String,
    head_sha: String,
    tooling_sha: String,
    toolchain: String,
    mode: RunMode,
    quick: bool,
    region: Option<String>,
    instance_type: String,
    tenancy: Tenancy,
    spot: bool,
    s3_bucket: String,
    s3_prefix: String,
    ami_id: Option<String>,
    vpc_id: Option<String>,
    subnet_id: Option<String>,
    security_group_id: Option<String>,
    instance_profile: Option<String>,
    profile: Option<String>,
    keep: bool,
    tags: Vec<(String, String)>,
    max_runtime_min: u32,
    output_dir: std::path::PathBuf,
    creates_security_group: bool,
    creates_instance_profile: bool,
}

impl RunConfig {
    /// Validate and assemble a config from parsed args and git-resolved refs.
    /// Fails on any validation error: bad tag, toolchain, or tenancy, or spot
    /// with dedicated tenancy.
    fn build(
        args: &RunArgs,
        repo_url: String,
        base_sha: String,
        head_sha: String,
        tooling_sha: String,
        run_id: RunId,
    ) -> anyhow::Result<Self> {
        validate_toolchain(&args.toolchain)?;
        let tenancy = match args.tenancy.as_str() {
            "default" => Tenancy::Default,
            "dedicated" => Tenancy::Dedicated,
            other => anyhow::bail!("--tenancy must be default|dedicated, got {other:?}"),
        };
        check_market(args.spot, tenancy)?;
        let mut tags = Vec::new();
        for raw in &args.tags {
            let (k, v) = parse_tag(raw)?;
            validate_tag_key(&k)?;
            tags.push((k, v));
        }
        let s3_prefix = args
            .s3_prefix
            .clone()
            .unwrap_or_else(|| default_s3_prefix(&run_id));
        let output_dir = args
            .output_dir
            .clone()
            .unwrap_or_else(|| default_output_dir(&run_id));
        let max_runtime_min = args
            .max_runtime
            .unwrap_or_else(|| default_max_runtime(args.quick));
        Ok(Self {
            mode: resolve_mode(&head_sha, &base_sha),
            run_id,
            repo_url,
            base_sha,
            head_sha,
            tooling_sha,
            toolchain: args.toolchain.clone(),
            quick: args.quick,
            region: args.region.clone(),
            instance_type: args.instance_type.clone(),
            tenancy,
            spot: args.spot,
            s3_bucket: args.s3_bucket.clone(),
            s3_prefix,
            ami_id: args.ami_id.clone(),
            vpc_id: args.vpc_id.clone(),
            subnet_id: args.subnet_id.clone(),
            security_group_id: args.security_group_id.clone(),
            instance_profile: args.instance_profile.clone(),
            profile: args.profile.clone(),
            keep: args.keep,
            tags,
            max_runtime_min,
            output_dir,
            creates_security_group: args.security_group_id.is_none(),
            creates_instance_profile: args.instance_profile.is_none(),
        })
    }

    /// The instance clones origin and fetches these exact shas, so refuse to
    /// launch when any is unreachable on the remote.
    fn repo_hashes(&self) -> impl IntoIterator<Item = &str> + '_ {
        [
            self.head_sha.as_str(),
            self.base_sha.as_str(),
            self.tooling_sha.as_str(),
        ]
    }

    /// The SSM public parameter for the Ubuntu 26.04 AMI (arch resolved by the
    /// adapter). Verified against Canonical's published parameters.
    const fn ssm_ami_param() -> &'static str {
        "/aws/service/canonical/ubuntu/server/26.04/stable/current/amd64/hvm/ebs-gp3/ami-id"
    }

    /// Build the user-data parameters from this config.
    fn user_data_params(&self) -> crate::cmd::ec2_bench::userdata::UserDataParams {
        crate::cmd::ec2_bench::userdata::UserDataParams {
            repo_url: self.repo_url.clone(),
            base_sha: self.base_sha.clone(),
            head_sha: self.head_sha.clone(),
            tooling_sha: self.tooling_sha.clone(),
            toolchain: self.toolchain.clone(),
            mode: self.mode,
            quick: self.quick,
            s3_bucket: self.s3_bucket.clone(),
            s3_prefix: self.s3_prefix.clone(),
            max_runtime_min: self.max_runtime_min,
        }
    }

    /// Build the display plan for confirmation / dry-run.
    fn plan(&self) -> RunPlan {
        RunPlan {
            run_id: self.run_id.clone(),
            region: self.region.clone().unwrap_or_else(|| "(ambient)".into()),
            instance_type: self.instance_type.clone(),
            tenancy: self.tenancy,
            spot: self.spot,
            mode: self.mode,
            base_sha: self.base_sha.clone(),
            head_sha: self.head_sha.clone(),
            s3_uri: format!("s3://{}/{}", self.s3_bucket, self.s3_prefix),
            max_runtime_min: self.max_runtime_min,
            creates_security_group: self.creates_security_group,
            creates_instance_profile: self.creates_instance_profile,
            cost_estimate: estimate_cost_usd(&self.instance_type, self.spot, self.max_runtime_min),
        }
    }
}

/// Prompt on stderr for launch confirmation (stdout is reserved for
/// artifacts, and a redirected stdout must not swallow the prompt).
fn confirm() -> bool {
    eprint!("proceed? [y/N] ");
    let mut line = String::new();
    drop(std::io::stdin().read_line(&mut line));
    matches!(line.trim(), "y" | "Y" | "yes")
}

fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn now_salt() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos())
}

/// Test fixture: a valid `RunArgs` baseline (nightly, c7i.2xlarge, default
/// tenancy, bucket "b"), mutated by the caller, built with fixed repo/sha
/// inputs and `RunId::generate(0, 0)`.
#[cfg(test)]
fn try_config(
    head: &str,
    base: &str,
    mutate: impl FnOnce(&mut RunArgs),
) -> anyhow::Result<RunConfig> {
    let mut args = RunArgs {
        toolchain: "nightly".into(),
        instance_type: "c7i.2xlarge".into(),
        tenancy: "default".into(),
        s3_bucket: "b".into(),
        ..RunArgs::default()
    };
    mutate(&mut args);
    RunConfig::build(
        &args,
        "u".into(),
        base.into(),
        head.into(),
        head.into(),
        RunId::generate(0, 0),
    )
}

/// `try_config` with distinct head/base refs, unwrapped.
#[cfg(test)]
fn test_config(mutate: impl FnOnce(&mut RunArgs)) -> RunConfig {
    try_config("head", "base", mutate).expect("valid test config")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_resolves_mode_and_defaults() {
        let cfg = test_config(|_| {});
        assert_eq!(cfg.mode, RunMode::Compare);
        assert_eq!(cfg.max_runtime_min, 360);
        assert_eq!(cfg.s3_prefix, "arity-bench/19700101t000000z-000000/");
        assert!(cfg.creates_security_group);
        assert!(cfg.creates_instance_profile);
    }

    #[test]
    fn build_rejects_spot_dedicated() {
        let result = try_config("head", "base", |a| {
            a.spot = true;
            a.tenancy = "dedicated".into();
        });
        assert!(result.is_err());
    }

    #[test]
    fn build_rejects_bad_tag() {
        assert!(try_config("head", "base", |a| a.tags = vec!["RunId=oops".into()]).is_err());
    }

    #[test]
    fn equal_refs_select_charts_mode() {
        let cfg = try_config("same", "same", |_| {}).expect("valid test config");
        assert_eq!(cfg.mode, RunMode::Charts);
    }

    #[test]
    fn build_rejects_bad_tenancy() {
        assert!(try_config("head", "base", |a| a.tenancy = "weird".into()).is_err());
    }

    #[test]
    fn supplied_ids_suppress_resource_creation() {
        let cfg = test_config(|a| {
            a.security_group_id = Some("sg-123".into());
            a.instance_profile = Some("prof".into());
        });
        assert!(!cfg.creates_security_group);
        assert!(!cfg.creates_instance_profile);
    }
}
