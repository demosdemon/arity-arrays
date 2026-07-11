//! Create the (optional) egress-only security group and launch the instance.
//! IAM is handled separately by the caller (bring-your-own or auto-create), so
//! this module never touches IAM.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "Provisioned.created_security_group is read only by this module's tests; the run flow gates teardown on cfg.creates_security_group instead"
    )
)]

use crate::cmd::ec2_bench::aws::AwsBackend;
use crate::cmd::ec2_bench::aws::LaunchSpec;
use crate::cmd::ec2_bench::aws::Tag;

/// Managed tags plus the user's billing tags. `ManagedBy`/`RunId` are the keys
/// teardown discovery and the destructive-action IAM condition rely on.
#[must_use]
pub fn managed_tags(run_id: &str, user: &[(String, String)]) -> Vec<Tag> {
    let mut tags = vec![
        Tag::new("ManagedBy", "arity-xtask-ec2-bench"),
        Tag::new("RunId", run_id),
    ];
    tags.extend(user.iter().map(|(k, v)| Tag::new(k.clone(), v.clone())));
    tags
}

/// Inputs to a launch. Everything AWS-resolvable (ami, subnet) is already
/// resolved by the caller's preflight.
#[derive(Debug)]
pub struct ProvisionInput {
    /// Run id (for tags and the SG name).
    pub run_id: String,
    /// VPC to launch in.
    pub vpc_id: String,
    /// Subnet to launch in.
    pub subnet_id: String,
    /// Resolved AMI.
    pub ami_id: String,
    /// Instance type.
    pub instance_type: String,
    /// Dedicated tenancy when true.
    pub tenancy_dedicated: bool,
    /// Spot launch when true.
    pub spot: bool,
    /// Instance-profile name to attach (already resolved).
    pub instance_profile_name: String,
    /// Rendered user-data.
    pub user_data: String,
    /// User billing tags.
    pub user_tags: Vec<(String, String)>,
    /// Existing SG, or `None` to create an egress-only one.
    pub security_group_id: Option<String>,
}

/// Result of provisioning.
#[derive(Debug)]
pub struct Provisioned {
    /// Launched instance id.
    pub instance_id: String,
    /// Public IPv4, if any.
    pub public_ip: Option<String>,
    /// SG the CLI created (and must delete), if any.
    pub created_security_group: Option<String>,
}

/// Create the SG if needed, then launch the tagged instance.
#[expect(
    clippy::future_not_send,
    reason = "AwsBackend futures are driven on a current-thread runtime (see aws.rs); Send is unnecessary"
)]
pub async fn provision<A: AwsBackend>(
    aws: &A,
    input: &ProvisionInput,
) -> anyhow::Result<Provisioned> {
    let tags = managed_tags(&input.run_id, &input.user_tags);
    let (security_group_id, created_security_group) =
        if let Some(existing) = &input.security_group_id {
            (existing.clone(), None)
        } else {
            let name = format!("arity-ec2-bench-{}", input.run_id);
            let id = aws
                .create_security_group(&name, &input.vpc_id, &tags)
                .await?;
            (id.clone(), Some(id))
        };
    let spec = LaunchSpec {
        ami_id: input.ami_id.clone(),
        instance_type: input.instance_type.clone(),
        subnet_id: input.subnet_id.clone(),
        security_group_id,
        tenancy_dedicated: input.tenancy_dedicated,
        spot: input.spot,
        instance_profile_name: input.instance_profile_name.clone(),
        user_data: input.user_data.clone(),
        tags,
    };
    let inst = aws.run_instance(&spec).await?;
    Ok(Provisioned {
        instance_id: inst.instance_id,
        public_ip: inst.public_ip,
        created_security_group,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::ec2_bench::aws::drive_ready;
    use crate::cmd::ec2_bench::aws::fake::FakeAws;

    fn input(sg: Option<String>) -> ProvisionInput {
        ProvisionInput {
            run_id: "r1".into(),
            vpc_id: "vpc-1".into(),
            subnet_id: "subnet-1".into(),
            ami_id: "ami-1".into(),
            instance_type: "c7i.2xlarge".into(),
            tenancy_dedicated: false,
            spot: true,
            instance_profile_name: "prof".into(),
            user_data: "#!/bin/bash".into(),
            user_tags: vec![("Team".into(), "perf".into())],
            security_group_id: sg,
        }
    }

    #[test]
    fn creates_sg_when_none_provided_and_tags_launch() {
        let aws = FakeAws::default();
        let out = drive_ready(provision(&aws, &input(None))).unwrap();
        assert!(out.created_security_group.is_some());
        let s = aws.state.lock().unwrap();
        let spec = &s.launched[0];
        assert!(spec.spot);
        assert!(
            spec.tags
                .iter()
                .any(|t| t.key == "ManagedBy" && t.value == "arity-xtask-ec2-bench")
        );
        assert!(
            spec.tags
                .iter()
                .any(|t| t.key == "RunId" && t.value == "r1")
        );
        assert!(
            spec.tags
                .iter()
                .any(|t| t.key == "Team" && t.value == "perf")
        );
        drop(s);
        // provision propagates the launched instance identity
        assert_eq!(out.instance_id, "i-0000");
        assert_eq!(out.public_ip.as_deref(), Some("203.0.113.1"));
    }

    #[test]
    fn reuses_provided_sg_without_creating() {
        let aws = FakeAws::default();
        let out = drive_ready(provision(&aws, &input(Some("sg-user".into())))).unwrap();
        assert_eq!(out.created_security_group, None);
        assert_eq!(
            aws.state.lock().unwrap().launched[0].security_group_id,
            "sg-user"
        );
        assert!(aws.state.lock().unwrap().security_groups.is_empty());
    }
}
