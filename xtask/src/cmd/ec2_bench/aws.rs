//! The single boundary between orchestration and AWS. Orchestration is generic
//! over `AwsBackend`; the `RealAws` adapter wraps the AWS SDK, and `FakeAws`
//! (test-only) provides deterministic in-memory behavior so the whole flow is
//! exercised without credentials or network.

use anyhow::Context;
use aws_sdk_ec2::error::ProvideErrorMetadata;

/// A resource tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    /// Tag key.
    pub key: String,
    /// Tag value.
    pub value: String,
}

impl Tag {
    /// Convenience constructor.
    #[must_use]
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

/// Everything needed to launch the instance in one `RunInstances` call.
#[derive(Debug, Clone)]
pub struct LaunchSpec {
    /// Resolved AMI id.
    pub ami_id: String,
    /// Instance type.
    pub instance_type: String,
    /// Subnet to launch into.
    pub subnet_id: String,
    /// Security group id.
    pub security_group_id: String,
    /// Dedicated tenancy when true.
    pub tenancy_dedicated: bool,
    /// Spot-market launch when true.
    pub spot: bool,
    /// Instance-profile name to attach.
    pub instance_profile_name: String,
    /// Cloud-init user-data (plain text; the adapter base64-encodes for the
    /// API).
    pub user_data: String,
    /// Tags applied to the instance and its volumes.
    pub tags: Vec<Tag>,
}

/// A launched instance.
#[derive(Debug, Clone)]
pub struct LaunchedInstance {
    /// EC2 instance id.
    pub instance_id: String,
    /// Public IPv4, if assigned.
    pub public_ip: Option<String>,
}

/// All AWS operations the orchestration needs. Internal to the crate and driven
/// on a current-thread runtime, so `Send` futures are unnecessary.
pub trait AwsBackend {
    // Identity / environment.
    async fn account_id(&self) -> anyhow::Result<String>;
    // SSM / AMI.
    async fn resolve_ami(&self, ssm_param: &str) -> anyhow::Result<String>;
    // VPC / subnet.
    async fn default_vpc_id(&self) -> anyhow::Result<Option<String>>;
    async fn first_subnet(&self, vpc_id: &str) -> anyhow::Result<Option<String>>;
    // Security groups.
    async fn create_security_group(
        &self,
        name: &str,
        vpc_id: &str,
        tags: &[Tag],
    ) -> anyhow::Result<String>;
    async fn find_security_groups_by_run(&self, run_id: &str) -> anyhow::Result<Vec<String>>;
    async fn delete_security_group(&self, id: &str) -> anyhow::Result<()>;
    // IAM (auto-create path).
    async fn create_bench_role(
        &self,
        name: &str,
        assume_doc: &str,
        perm_doc: &str,
        tags: &[Tag],
    ) -> anyhow::Result<()>;
    async fn create_instance_profile(
        &self,
        name: &str,
        role_name: &str,
        tags: &[Tag],
    ) -> anyhow::Result<()>;
    async fn delete_bench_role_and_profile(&self, name: &str) -> anyhow::Result<()>;
    // Instances.
    async fn run_instance(&self, spec: &LaunchSpec) -> anyhow::Result<LaunchedInstance>;
    async fn instance_state(&self, id: &str) -> anyhow::Result<String>;
    async fn find_instances_by_run(&self, run_id: &str) -> anyhow::Result<Vec<String>>;
    async fn terminate_instance(&self, id: &str) -> anyhow::Result<()>;
    // S3.
    async fn bucket_reachable(&self, bucket: &str) -> anyhow::Result<()>;
    async fn get_object_text(&self, bucket: &str, key: &str) -> anyhow::Result<Option<String>>;
    async fn download_object(
        &self,
        bucket: &str,
        key: &str,
        dest: &std::path::Path,
    ) -> anyhow::Result<()>;
    async fn list_objects(&self, bucket: &str, prefix: &str) -> anyhow::Result<Vec<String>>;
}

/// Real `AwsBackend` backed by the AWS SDK. Every method is a thin
/// translation: DTOs in, SDK errors out.
pub struct RealAws {
    ec2: aws_sdk_ec2::Client,
    iam: aws_sdk_iam::Client,
    s3: aws_sdk_s3::Client,
    ssm: aws_sdk_ssm::Client,
    sts: aws_sdk_sts::Client,
}

impl RealAws {
    /// Build clients from the ambient credential chain, honoring optional
    /// `--profile` / `--region` overrides. Fails when no region can be
    /// resolved from the override, the selected profile, or the environment.
    pub async fn new(profile: Option<String>, region: Option<String>) -> anyhow::Result<Self> {
        let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest());
        if let Some(p) = profile {
            loader = loader.profile_name(p);
        }
        if let Some(r) = region {
            loader = loader.region(aws_sdk_ec2::config::Region::new(r));
        }
        let conf = loader.load().await;
        // Validate that a region resolved (the SDK clients need one) without
        // retaining it: no method reads a cached region back.
        conf.region()
            .context("no AWS region; pass --region or set one in your profile")?;
        Ok(Self {
            ec2: aws_sdk_ec2::Client::new(&conf),
            iam: aws_sdk_iam::Client::new(&conf),
            s3: aws_sdk_s3::Client::new(&conf),
            ssm: aws_sdk_ssm::Client::new(&conf),
            sts: aws_sdk_sts::Client::new(&conf),
        })
    }
}

impl AwsBackend for RealAws {
    async fn account_id(&self) -> anyhow::Result<String> {
        let out = self
            .sts
            .get_caller_identity()
            .send()
            .await
            .context("failed to resolve caller identity")?;
        Ok(out.account().unwrap_or_default().to_owned())
    }

    async fn resolve_ami(&self, ssm_param: &str) -> anyhow::Result<String> {
        let out = self
            .ssm
            .get_parameter()
            .name(ssm_param)
            .send()
            .await
            .with_context(|| format!("failed to read SSM parameter {ssm_param}"))?;
        out.parameter()
            .and_then(|p| p.value().map(ToOwned::to_owned))
            .context("SSM parameter had no value")
    }

    async fn default_vpc_id(&self) -> anyhow::Result<Option<String>> {
        use aws_sdk_ec2::types::Filter;
        let out = self
            .ec2
            .describe_vpcs()
            .filters(Filter::builder().name("is-default").values("true").build())
            .send()
            .await
            .context("failed to describe VPCs")?;
        Ok(out
            .vpcs()
            .first()
            .and_then(|v| v.vpc_id().map(ToOwned::to_owned)))
    }

    async fn first_subnet(&self, vpc_id: &str) -> anyhow::Result<Option<String>> {
        use aws_sdk_ec2::types::Filter;
        let out = self
            .ec2
            .describe_subnets()
            .filters(Filter::builder().name("vpc-id").values(vpc_id).build())
            .send()
            .await
            .with_context(|| format!("failed to describe subnets in {vpc_id}"))?;
        Ok(out
            .subnets()
            .first()
            .and_then(|s| s.subnet_id().map(ToOwned::to_owned)))
    }

    async fn create_security_group(
        &self,
        name: &str,
        vpc_id: &str,
        tags: &[Tag],
    ) -> anyhow::Result<String> {
        // A fresh SG has default allow-all egress and no ingress — exactly the
        // egress-only posture the benchmark wants.
        use aws_sdk_ec2::types::ResourceType;
        let out = self
            .ec2
            .create_security_group()
            .group_name(name)
            .description("arity ec2-bench egress-only")
            .vpc_id(vpc_id)
            .tag_specifications(tag_spec(ResourceType::SecurityGroup, tags))
            .send()
            .await
            .with_context(|| format!("failed to create security group {name}"))?;
        out.group_id()
            .map(ToOwned::to_owned)
            .context("CreateSecurityGroup returned no group id")
    }

    async fn find_security_groups_by_run(&self, run_id: &str) -> anyhow::Result<Vec<String>> {
        use aws_sdk_ec2::types::Filter;
        let out = self
            .ec2
            .describe_security_groups()
            .filters(Filter::builder().name("tag:RunId").values(run_id).build())
            .send()
            .await
            .with_context(|| format!("failed to describe security groups for run {run_id}"))?;
        Ok(out
            .security_groups()
            .iter()
            .filter_map(|g| g.group_id().map(ToOwned::to_owned))
            .collect())
    }

    async fn delete_security_group(&self, id: &str) -> anyhow::Result<()> {
        // On the success path an instance self-terminates (shutdown -h now) and
        // teardown fires immediately, so its ENI may still reference this SG.
        // EC2 answers DependencyViolation until the ENI is released; bounded-
        // retry (~60s worst case) rather than leak the SG. Any other error
        // returns at once; on exhaustion surface the last error.
        let mut last: Option<anyhow::Error> = None;
        for _ in 0..12u32 {
            match self.ec2.delete_security_group().group_id(id).send().await {
                Ok(_) => return Ok(()),
                Err(e) if is_ec2_not_found(&e) => return Ok(()),
                Err(e) if is_ec2_dependency_violation(&e) => {
                    last = Some(e.into());
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
                Err(e) => {
                    return Err(anyhow::Error::from(e)
                        .context(format!("failed to delete security group {id}")));
                }
            }
        }
        Err(last
            .unwrap_or_else(|| anyhow::anyhow!("still blocked by a dependency"))
            .context(format!("failed to delete security group {id}")))
    }

    async fn create_bench_role(
        &self,
        name: &str,
        assume_doc: &str,
        perm_doc: &str,
        tags: &[Tag],
    ) -> anyhow::Result<()> {
        self.iam
            .create_role()
            .role_name(name)
            .assume_role_policy_document(assume_doc)
            .set_tags(Some(iam_tags(tags)))
            .send()
            .await
            .with_context(|| format!("failed to create role {name}"))?;
        self.iam
            .put_role_policy()
            .role_name(name)
            .policy_name(BENCH_POLICY_NAME)
            .policy_document(perm_doc)
            .send()
            .await
            .with_context(|| format!("failed to attach inline policy to role {name}"))?;
        Ok(())
    }

    async fn create_instance_profile(
        &self,
        name: &str,
        role_name: &str,
        tags: &[Tag],
    ) -> anyhow::Result<()> {
        self.iam
            .create_instance_profile()
            .instance_profile_name(name)
            .set_tags(Some(iam_tags(tags)))
            .send()
            .await
            .with_context(|| format!("failed to create instance profile {name}"))?;
        self.iam
            .add_role_to_instance_profile()
            .instance_profile_name(name)
            .role_name(role_name)
            .send()
            .await
            .with_context(|| {
                format!("failed to add role {role_name} to instance profile {name}")
            })?;
        Ok(())
    }

    async fn delete_bench_role_and_profile(&self, name: &str) -> anyhow::Result<()> {
        // Best-effort, in dependency order. Each not-found is already the
        // desired end state, so per-call errors are intentionally dropped; the
        // caller reports overall teardown completeness.
        drop(
            self.iam
                .remove_role_from_instance_profile()
                .instance_profile_name(name)
                .role_name(name)
                .send()
                .await,
        );
        drop(
            self.iam
                .delete_instance_profile()
                .instance_profile_name(name)
                .send()
                .await,
        );
        drop(
            self.iam
                .delete_role_policy()
                .role_name(name)
                .policy_name(BENCH_POLICY_NAME)
                .send()
                .await,
        );
        drop(self.iam.delete_role().role_name(name).send().await);
        Ok(())
    }

    async fn run_instance(&self, spec: &LaunchSpec) -> anyhow::Result<LaunchedInstance> {
        use aws_sdk_ec2::types::HttpTokensState;
        use aws_sdk_ec2::types::IamInstanceProfileSpecification;
        use aws_sdk_ec2::types::InstanceMarketOptionsRequest;
        use aws_sdk_ec2::types::InstanceMetadataOptionsRequest;
        use aws_sdk_ec2::types::MarketType;
        use aws_sdk_ec2::types::Placement;
        use aws_sdk_ec2::types::ResourceType;
        use aws_sdk_ec2::types::ShutdownBehavior;
        use aws_sdk_ec2::types::Tenancy;
        let mut req = self
            .ec2
            .run_instances()
            .image_id(spec.ami_id.as_str())
            .instance_type(spec.instance_type.as_str().into())
            .min_count(1)
            .max_count(1)
            .subnet_id(spec.subnet_id.as_str())
            .security_group_ids(spec.security_group_id.as_str())
            // The EC2 API's UserData field is transmitted verbatim (the SDK does
            // not encode it), so it must arrive base64-encoded exactly once.
            .user_data(super::userdata::base64_encode(spec.user_data.as_bytes()))
            .instance_initiated_shutdown_behavior(ShutdownBehavior::Terminate)
            .iam_instance_profile(
                IamInstanceProfileSpecification::builder()
                    .name(spec.instance_profile_name.as_str())
                    .build(),
            )
            .metadata_options(
                InstanceMetadataOptionsRequest::builder()
                    .http_tokens(HttpTokensState::Required)
                    .build(),
            )
            .tag_specifications(tag_spec(ResourceType::Instance, &spec.tags))
            .tag_specifications(tag_spec(ResourceType::Volume, &spec.tags));
        if spec.tenancy_dedicated {
            req = req.placement(Placement::builder().tenancy(Tenancy::Dedicated).build());
        }
        if spec.spot {
            req = req.instance_market_options(
                InstanceMarketOptionsRequest::builder()
                    .market_type(MarketType::Spot)
                    .build(),
            );
        }
        let out = run_with_iam_retry(&req).await?;
        let inst = out
            .instances()
            .first()
            .context("RunInstances returned no instance")?;
        Ok(LaunchedInstance {
            instance_id: inst.instance_id().unwrap_or_default().to_owned(),
            public_ip: inst.public_ip_address().map(ToOwned::to_owned),
        })
    }

    async fn instance_state(&self, id: &str) -> anyhow::Result<String> {
        use aws_sdk_ec2::types::Instance;
        use aws_sdk_ec2::types::InstanceState;
        let out = self
            .ec2
            .describe_instances()
            .instance_ids(id)
            .send()
            .await
            .with_context(|| format!("failed to describe instance {id}"))?;
        Ok(out
            .reservations()
            .iter()
            .flat_map(|r| r.instances().iter())
            .next()
            .and_then(Instance::state)
            .and_then(InstanceState::name)
            .map_or_else(|| "unknown".into(), |n| n.as_str().to_owned()))
    }

    async fn find_instances_by_run(&self, run_id: &str) -> anyhow::Result<Vec<String>> {
        use aws_sdk_ec2::types::Filter;
        let out = self
            .ec2
            .describe_instances()
            .filters(Filter::builder().name("tag:RunId").values(run_id).build())
            .filters(
                Filter::builder()
                    .name("instance-state-name")
                    .values("pending")
                    .values("running")
                    .values("shutting-down")
                    .values("stopping")
                    .values("stopped")
                    .build(),
            )
            .send()
            .await
            .with_context(|| format!("failed to describe instances for run {run_id}"))?;
        Ok(out
            .reservations()
            .iter()
            .flat_map(|r| r.instances().iter())
            .filter_map(|i| i.instance_id().map(ToOwned::to_owned))
            .collect())
    }

    async fn terminate_instance(&self, id: &str) -> anyhow::Result<()> {
        match self.ec2.terminate_instances().instance_ids(id).send().await {
            Ok(_) => Ok(()),
            Err(e) if is_ec2_not_found(&e) => Ok(()),
            Err(e) => {
                Err(anyhow::Error::from(e).context(format!("failed to terminate instance {id}")))
            }
        }
    }

    async fn bucket_reachable(&self, bucket: &str) -> anyhow::Result<()> {
        self.s3
            .head_bucket()
            .bucket(bucket)
            .send()
            .await
            .with_context(|| format!("failed to reach bucket {bucket}"))?;
        Ok(())
    }

    async fn get_object_text(&self, bucket: &str, key: &str) -> anyhow::Result<Option<String>> {
        use aws_sdk_s3::operation::get_object::GetObjectError;
        match self.s3.get_object().bucket(bucket).key(key).send().await {
            Ok(o) => {
                let bytes = o
                    .body
                    .collect()
                    .await
                    .with_context(|| format!("failed to read s3://{bucket}/{key}"))?
                    .into_bytes();
                Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
            }
            Err(e) if matches!(e.as_service_error(), Some(GetObjectError::NoSuchKey(_))) => {
                Ok(None)
            }
            Err(e) => {
                Err(anyhow::Error::from(e).context(format!("failed to read s3://{bucket}/{key}")))
            }
        }
    }

    async fn download_object(
        &self,
        bucket: &str,
        key: &str,
        dest: &std::path::Path,
    ) -> anyhow::Result<()> {
        let o = self
            .s3
            .get_object()
            .bucket(bucket)
            .key(key)
            .send()
            .await
            .with_context(|| format!("failed to download s3://{bucket}/{key}"))?;
        let bytes = o
            .body
            .collect()
            .await
            .with_context(|| format!("failed to download s3://{bucket}/{key}"))?
            .into_bytes();
        crate::fs::write(dest, &bytes)?;
        Ok(())
    }

    async fn list_objects(&self, bucket: &str, prefix: &str) -> anyhow::Result<Vec<String>> {
        let out = self
            .s3
            .list_objects_v2()
            .bucket(bucket)
            .prefix(prefix)
            .send()
            .await
            .with_context(|| format!("failed to list s3://{bucket}/{prefix}"))?;
        Ok(out
            .contents()
            .iter()
            .filter_map(|o| o.key().map(ToOwned::to_owned))
            .collect())
    }
}

/// IAM inline-policy name for the instance role's scoped S3 write grant. Shared
/// so creation and deletion name the same policy.
const BENCH_POLICY_NAME: &str = "arity-ec2-bench-put-object";

/// Build an EC2 tag specification applying `tags` to a resource `kind`.
fn tag_spec(
    kind: aws_sdk_ec2::types::ResourceType,
    tags: &[Tag],
) -> aws_sdk_ec2::types::TagSpecification {
    let mut b = aws_sdk_ec2::types::TagSpecification::builder().resource_type(kind);
    for t in tags {
        b = b.tags(
            aws_sdk_ec2::types::Tag::builder()
                .key(t.key.as_str())
                .value(t.value.as_str())
                .build(),
        );
    }
    b.build()
}

/// Translate the DTO tags into IAM SDK tags. Both key and value are always
/// present here, so `build` never fails and no tag is dropped.
fn iam_tags(tags: &[Tag]) -> Vec<aws_sdk_iam::types::Tag> {
    tags.iter()
        .filter_map(|t| {
            aws_sdk_iam::types::Tag::builder()
                .key(t.key.as_str())
                .value(t.value.as_str())
                .build()
                .ok()
        })
        .collect()
}

/// Retry `RunInstances` while a freshly created IAM instance profile is still
/// propagating; fail fast on any other error. The fluent builder is `Clone`, so
/// each attempt re-sends the same request.
async fn run_with_iam_retry(
    req: &aws_sdk_ec2::operation::run_instances::builders::RunInstancesFluentBuilder,
) -> anyhow::Result<aws_sdk_ec2::operation::run_instances::RunInstancesOutput> {
    let mut last: Option<anyhow::Error> = None;
    for attempt in 0..6u32 {
        match req.clone().send().await {
            Ok(o) => return Ok(o),
            Err(e) => {
                if !is_iam_profile_propagation(&e) {
                    return Err(anyhow::Error::from(e).context("failed to launch instance"));
                }
                last = Some(e.into());
                tokio::time::sleep(std::time::Duration::from_secs(u64::from(5 * (attempt + 1))))
                    .await;
            }
        }
    }
    Err(last.unwrap_or_else(|| {
        anyhow::anyhow!("RunInstances failed after IAM instance-profile propagation retries")
    }))
}

/// Whether `e` is the eventually-consistent error EC2 returns just after an IAM
/// instance profile is created, before it becomes visible to `RunInstances`.
fn is_iam_profile_propagation<E: ProvideErrorMetadata>(e: &E) -> bool {
    let code = e.code().unwrap_or_default();
    let message = e.message().unwrap_or_default();
    code.contains("IamInstanceProfile")
        || message.contains("Instance Profile")
        || message.contains("instance profile")
}

/// EC2 service error codes meaning the resource is already gone, letting an
/// idempotent teardown treat the delete/terminate as success.
fn is_ec2_not_found<E: ProvideErrorMetadata>(e: &E) -> bool {
    matches!(
        e.code(),
        Some("InvalidGroup.NotFound" | "InvalidInstanceID.NotFound")
    )
}

/// EC2 returns this while a resource still has a dependent (e.g. a terminating
/// instance's ENI still attached to its security group). It clears once the
/// dependency is released, so an idempotent teardown should retry, not fail.
fn is_ec2_dependency_violation<E: ProvideErrorMetadata>(e: &E) -> bool {
    matches!(e.code(), Some("DependencyViolation"))
}

#[cfg(test)]
pub mod fake {
    use std::sync::Mutex;

    use super::AwsBackend;
    use super::LaunchSpec;
    use super::LaunchedInstance;
    use super::Tag;

    /// Deterministic in-memory backend for orchestration tests.
    #[derive(Default)]
    pub struct FakeAws {
        pub state: Mutex<State>,
    }

    #[derive(Default)]
    #[expect(
        clippy::struct_field_names,
        reason = "instance_state names the value the instance_state() trait method returns; renaming it to dodge the struct's own name would break that correspondence"
    )]
    pub struct State {
        pub instances: Vec<String>,
        pub security_groups: Vec<String>,
        pub roles: Vec<String>,
        pub terminated: Vec<String>,
        pub launched: Vec<LaunchSpec>,
        /// Text served for `get_object_text`, keyed by object key suffix.
        pub objects: std::collections::BTreeMap<String, String>,
        /// State returned by `instance_state` (default "running").
        pub instance_state: Option<String>,
        /// When set, `run_instance` returns an error (to exercise failure
        /// paths).
        pub fail_run_instance: bool,
    }

    impl FakeAws {
        pub fn with_status(status_json: &str) -> Self {
            let f = Self::default();
            f.state
                .lock()
                .expect("lock")
                .objects
                .insert("status.json".into(), status_json.to_owned());
            f
        }
    }

    // Every method below is `async fn` only to satisfy `AwsBackend`; the fake
    // is a pure in-memory stub with nothing to await, so no method body ever
    // yields.
    #[expect(
        clippy::unused_async_trait_impl,
        reason = "matching AwsBackend's async fn signatures (not clippy's suggested impl Future rewrite) keeps FakeAws a drop-in stand-in for RealAws"
    )]
    impl AwsBackend for FakeAws {
        async fn account_id(&self) -> anyhow::Result<String> {
            Ok("123456789012".into())
        }
        async fn resolve_ami(&self, _p: &str) -> anyhow::Result<String> {
            Ok("ami-fake".into())
        }
        async fn default_vpc_id(&self) -> anyhow::Result<Option<String>> {
            Ok(Some("vpc-fake".into()))
        }
        async fn first_subnet(&self, _v: &str) -> anyhow::Result<Option<String>> {
            Ok(Some("subnet-fake".into()))
        }
        async fn create_security_group(
            &self,
            name: &str,
            _v: &str,
            _t: &[Tag],
        ) -> anyhow::Result<String> {
            let id = format!("sg-{name}");
            self.state
                .lock()
                .expect("lock")
                .security_groups
                .push(id.clone());
            Ok(id)
        }
        async fn find_security_groups_by_run(&self, _r: &str) -> anyhow::Result<Vec<String>> {
            Ok(self.state.lock().expect("lock").security_groups.clone())
        }
        async fn delete_security_group(&self, id: &str) -> anyhow::Result<()> {
            self.state
                .lock()
                .expect("lock")
                .security_groups
                .retain(|s| s != id);
            Ok(())
        }
        async fn create_bench_role(
            &self,
            name: &str,
            _a: &str,
            _p: &str,
            _t: &[Tag],
        ) -> anyhow::Result<()> {
            self.state.lock().expect("lock").roles.push(name.to_owned());
            Ok(())
        }
        async fn create_instance_profile(
            &self,
            _n: &str,
            _r: &str,
            _t: &[Tag],
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn delete_bench_role_and_profile(&self, name: &str) -> anyhow::Result<()> {
            self.state.lock().expect("lock").roles.retain(|r| r != name);
            Ok(())
        }
        async fn run_instance(&self, spec: &LaunchSpec) -> anyhow::Result<LaunchedInstance> {
            let mut s = self.state.lock().expect("lock");
            if s.fail_run_instance {
                anyhow::bail!("simulated run_instance failure");
            }
            let id = format!("i-{:04}", s.instances.len());
            s.instances.push(id.clone());
            s.launched.push(spec.clone());
            drop(s);
            Ok(LaunchedInstance {
                instance_id: id,
                public_ip: Some("203.0.113.1".into()),
            })
        }
        async fn instance_state(&self, _id: &str) -> anyhow::Result<String> {
            Ok(self
                .state
                .lock()
                .expect("lock")
                .instance_state
                .clone()
                .unwrap_or_else(|| "running".into()))
        }
        async fn find_instances_by_run(&self, _r: &str) -> anyhow::Result<Vec<String>> {
            Ok(self.state.lock().expect("lock").instances.clone())
        }
        async fn terminate_instance(&self, id: &str) -> anyhow::Result<()> {
            let mut s = self.state.lock().expect("lock");
            s.instances.retain(|i| i != id);
            s.terminated.push(id.to_owned());
            drop(s);
            Ok(())
        }
        async fn bucket_reachable(&self, _b: &str) -> anyhow::Result<()> {
            Ok(())
        }
        async fn get_object_text(&self, _b: &str, key: &str) -> anyhow::Result<Option<String>> {
            let s = self.state.lock().expect("lock");
            Ok(s.objects
                .iter()
                .find(|(k, _)| key.ends_with(k.as_str()))
                .map(|(_, v)| v.clone()))
        }
        async fn download_object(
            &self,
            _b: &str,
            _k: &str,
            _d: &std::path::Path,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        async fn list_objects(&self, _b: &str, _p: &str) -> anyhow::Result<Vec<String>> {
            Ok(self
                .state
                .lock()
                .expect("lock")
                .objects
                .keys()
                .cloned()
                .collect())
        }
    }
}

/// Minimal test executor: busy-polls a future to completion without a
/// runtime. Only sound for the fake's futures, which are always immediately
/// ready; a future that actually suspends would spin forever.
#[cfg(test)]
pub fn drive_ready<F: std::future::Future>(fut: F) -> F::Output {
    use std::task::Context;
    use std::task::Poll;
    use std::task::RawWaker;
    use std::task::RawWakerVTable;
    use std::task::Waker;
    fn noop(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker {
        RawWaker::new(std::ptr::null(), &VT)
    }
    static VT: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
    // SAFETY: the vtable's functions are all no-ops on a null data pointer, so
    // the waker upholds the RawWaker contract (nothing is dereferenced).
    let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VT)) };
    let mut cx = Context::from_waker(&waker);
    let mut fut = std::pin::pin!(fut);
    loop {
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeAws;
    use super::*;

    #[test]
    fn fake_launch_and_terminate_roundtrip() {
        let aws = FakeAws::default();
        drive_ready(async {
            let spec = LaunchSpec {
                ami_id: "ami-x".into(),
                instance_type: "c7i.2xlarge".into(),
                subnet_id: "subnet-x".into(),
                security_group_id: "sg-x".into(),
                tenancy_dedicated: false,
                spot: false,
                instance_profile_name: "prof".into(),
                user_data: "#!/bin/bash".into(),
                tags: vec![Tag::new("RunId", "r1")],
            };
            let inst = aws.run_instance(&spec).await.unwrap();
            assert!(
                aws.find_instances_by_run("r1")
                    .await
                    .unwrap()
                    .contains(&inst.instance_id)
            );
            aws.terminate_instance(&inst.instance_id).await.unwrap();
            assert!(aws.find_instances_by_run("r1").await.unwrap().is_empty());
        });
    }
}
