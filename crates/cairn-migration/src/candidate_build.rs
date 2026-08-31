//! Task-generic Candidate materialization for an exact product-owned Worker build plan.
#![allow(clippy::missing_errors_doc)]

use std::{collections::BTreeSet, io::Cursor};

use cairn_execution::{
    CapabilityRequirement, CapturePolicy, CommandContract, ContractValueError, DOCKER_BACKEND,
    DockerEnvironmentError, DockerExecutionEnvironmentV1, DockerImageId, ExecutionBackend,
    ExecutionEnvironmentArtifact, ExecutionPlatformRequirement, ExecutionTimeoutMillis,
    InputBundleArtifact, InputBundleEntry, InputBundleV1, InputFileMode, JobContract,
    JobContractArtifact, MaterialFormatError, NetworkPolicy, PlacementRequest, ResourceRequest,
    SandboxPath, WorkerPoolName,
};
use cairn_protocol::{ContentId, ContentType, JobId};
use cairn_record::{ContentStore, ContentStoreError};
use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

use crate::{CandidateProposalArtifact, CandidateProposalV1};

const GENERIC_CANDIDATE_PUBLICATION_PATH: &str = "meta/candidate-proposal.json";

/// Controller-owned build recipe. It is configuration authority, never model output.
pub enum CandidateBuildPlanArtifact {}

impl ContentType for CandidateBuildPlanArtifact {
    const DOMAIN: &'static str = "migration.candidate-build-plan.v1";
}

/// Exact immutable generic Candidate build operation selected by the product.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "CandidateBuildPlanWire")]
pub struct CandidateBuildPlanV1 {
    schema_version: u16,
    image: DockerImageId,
    runner: Vec<u8>,
    worker_pools: Vec<WorkerPoolName>,
    capabilities: Vec<CapabilityRequirement>,
    timeout: ExecutionTimeoutMillis,
    capture: CapturePolicy,
    network: NetworkPolicy,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateBuildPlanWire {
    schema_version: u16,
    image: DockerImageId,
    runner: Vec<u8>,
    worker_pools: Vec<WorkerPoolName>,
    capabilities: Vec<CapabilityRequirement>,
    timeout: ExecutionTimeoutMillis,
    capture: CapturePolicy,
    network: NetworkPolicy,
}

impl CandidateBuildPlanV1 {
    /// Creates one bounded exact product-owned build plan.
    ///
    /// # Errors
    ///
    /// Rejects empty/oversized runner bytes, empty placement, or noncanonical requirements.
    pub fn new(
        image: DockerImageId,
        runner: Vec<u8>,
        worker_pools: Vec<WorkerPoolName>,
        capabilities: Vec<CapabilityRequirement>,
        timeout: ExecutionTimeoutMillis,
        capture: CapturePolicy,
        network: NetworkPolicy,
    ) -> Result<Self, CandidateBuildError> {
        let value = Self {
            schema_version: 1,
            image,
            runner,
            worker_pools,
            capabilities,
            timeout,
            capture,
            network,
        };
        value.validate()?;
        Ok(value)
    }

    #[must_use]
    pub const fn image(&self) -> &DockerImageId {
        &self.image
    }
    #[must_use]
    pub fn runner(&self) -> &[u8] {
        &self.runner
    }
    #[must_use]
    pub fn worker_pools(&self) -> &[WorkerPoolName] {
        &self.worker_pools
    }
    #[must_use]
    pub fn capabilities(&self) -> &[CapabilityRequirement] {
        &self.capabilities
    }

    pub fn identity(&self) -> Result<ContentId<CandidateBuildPlanArtifact>, CandidateBuildError> {
        self.validate()?;
        ContentId::derive(&cairn_codec::to_vec(self).map_err(codec)?).map_err(codec)
    }

    fn validate(&self) -> Result<(), CandidateBuildError> {
        if self.schema_version != 1
            || self.runner.is_empty()
            || self.runner.len() > 256 * 1024
            || self.runner.contains(&0)
            || self.worker_pools.is_empty()
            || self.worker_pools.windows(2).any(|pair| pair[0] >= pair[1])
            || self.capabilities.windows(2).any(|pair| {
                cairn_codec::to_vec(&pair[0]).ok() >= cairn_codec::to_vec(&pair[1]).ok()
            })
        {
            return Err(CandidateBuildError::InvalidGenericPlan);
        }
        Ok(())
    }
}

impl TryFrom<CandidateBuildPlanWire> for CandidateBuildPlanV1 {
    type Error = CandidateBuildError;
    fn try_from(wire: CandidateBuildPlanWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            image: wire.image,
            runner: wire.runner,
            worker_pools: wire.worker_pools,
            capabilities: wire.capabilities,
            timeout: wire.timeout,
            capture: wire.capture,
            network: wire.network,
        };
        value.validate()?;
        Ok(value)
    }
}

/// Exact generic build request archived before scheduling any Worker effect.
///
/// A model-authored proposal identity cannot substitute for Controller-owned build authority.
///
/// ```compile_fail
/// use cairn_migration::{CandidateBuildRequestArtifact, CandidateProposalArtifact};
/// use cairn_protocol::ContentId;
/// fn require_build(_: ContentId<CandidateBuildRequestArtifact>) {}
/// fn invalid(proposal: ContentId<CandidateProposalArtifact>) { require_build(proposal); }
/// ```
pub enum CandidateBuildRequestArtifact {}

impl ContentType for CandidateBuildRequestArtifact {
    const DOMAIN: &'static str = "migration.candidate-build-request.v1";
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateBuildRequestV1 {
    schema_version: u16,
    proposal: ContentId<CandidateProposalArtifact>,
    plan: ContentId<CandidateBuildPlanArtifact>,
    job_id: JobId,
    input_bundle: ContentId<InputBundleArtifact>,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    contract: ContentId<JobContractArtifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidateBuildRequestWire {
    schema_version: u16,
    proposal: ContentId<CandidateProposalArtifact>,
    plan: ContentId<CandidateBuildPlanArtifact>,
    job_id: JobId,
    input_bundle: ContentId<InputBundleArtifact>,
    environment: ContentId<ExecutionEnvironmentArtifact>,
    contract: ContentId<JobContractArtifact>,
}

impl CandidateBuildRequestV1 {
    #[must_use]
    pub const fn proposal(&self) -> ContentId<CandidateProposalArtifact> {
        self.proposal
    }
    #[must_use]
    pub const fn plan(&self) -> ContentId<CandidateBuildPlanArtifact> {
        self.plan
    }
    #[must_use]
    pub const fn job_id(&self) -> JobId {
        self.job_id
    }
    #[must_use]
    pub const fn input_bundle(&self) -> ContentId<InputBundleArtifact> {
        self.input_bundle
    }
    #[must_use]
    pub const fn environment(&self) -> ContentId<ExecutionEnvironmentArtifact> {
        self.environment
    }
    #[must_use]
    pub const fn contract(&self) -> ContentId<JobContractArtifact> {
        self.contract
    }

    pub fn identity(
        &self,
    ) -> Result<ContentId<CandidateBuildRequestArtifact>, CandidateBuildError> {
        if self.schema_version != 1 {
            return Err(CandidateBuildError::InvalidGenericPlan);
        }
        ContentId::derive(&cairn_codec::to_vec(self).map_err(codec)?).map_err(codec)
    }
}

impl TryFrom<CandidateBuildRequestWire> for CandidateBuildRequestV1 {
    type Error = CandidateBuildError;

    fn try_from(wire: CandidateBuildRequestWire) -> Result<Self, Self::Error> {
        let value = Self {
            schema_version: wire.schema_version,
            proposal: wire.proposal,
            plan: wire.plan,
            job_id: wire.job_id,
            input_bundle: wire.input_bundle,
            environment: wire.environment,
            contract: wire.contract,
        };
        if value.schema_version != 1 {
            return Err(CandidateBuildError::InvalidGenericPlan);
        }
        Ok(value)
    }
}

impl<'de> Deserialize<'de> for CandidateBuildRequestV1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        CandidateBuildRequestWire::deserialize(deserializer)?
            .try_into()
            .map_err(de::Error::custom)
    }
}

/// Canonical Candidate proposal, input tree, environment and contract ready for archival.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedGenericCandidateBuildJob {
    proposal: CandidateProposalV1,
    proposal_bytes: Vec<u8>,
    plan: CandidateBuildPlanV1,
    plan_bytes: Vec<u8>,
    request: CandidateBuildRequestV1,
    input_bundle: InputBundleV1,
    input_bundle_bytes: Vec<u8>,
    environment: DockerExecutionEnvironmentV1,
    environment_bytes: Vec<u8>,
    contract: JobContract,
    contract_bytes: Vec<u8>,
}

impl PreparedGenericCandidateBuildJob {
    #[must_use]
    pub const fn request(&self) -> &CandidateBuildRequestV1 {
        &self.request
    }
    #[must_use]
    pub const fn plan(&self) -> &CandidateBuildPlanV1 {
        &self.plan
    }
    #[must_use]
    pub const fn contract(&self) -> &JobContract {
        &self.contract
    }
    #[must_use]
    pub const fn input_bundle(&self) -> &InputBundleV1 {
        &self.input_bundle
    }
    #[must_use]
    pub const fn environment(&self) -> &DockerExecutionEnvironmentV1 {
        &self.environment
    }

    /// Archives every exact body and rechecks all typed identities.
    pub fn archive<S: ContentStore>(&self, content: &mut S) -> Result<(), CandidateBuildError> {
        let proposal = content
            .put::<CandidateProposalArtifact>(&mut Cursor::new(&self.proposal_bytes))?
            .content_id;
        let plan = content
            .put::<CandidateBuildPlanArtifact>(&mut Cursor::new(&self.plan_bytes))?
            .content_id;
        let input = content
            .put::<InputBundleArtifact>(&mut Cursor::new(&self.input_bundle_bytes))?
            .content_id;
        let environment = content
            .put::<ExecutionEnvironmentArtifact>(&mut Cursor::new(&self.environment_bytes))?
            .content_id;
        let contract = content
            .put::<JobContractArtifact>(&mut Cursor::new(&self.contract_bytes))?
            .content_id;
        if proposal != self.request.proposal
            || plan != self.request.plan
            || input != self.request.input_bundle
            || environment != self.request.environment
            || contract != self.request.contract
        {
            return Err(CandidateBuildError::MaterialIdentityMismatch);
        }
        Ok(())
    }
}

/// Materializes a task-generic Candidate proposal under one exact product-owned build plan.
pub fn prepare_generic_candidate_build_job(
    job_id: JobId,
    proposal_bytes: &[u8],
    proposal_id: ContentId<CandidateProposalArtifact>,
    plan: CandidateBuildPlanV1,
) -> Result<PreparedGenericCandidateBuildJob, CandidateBuildError> {
    let proposal: CandidateProposalV1 = cairn_codec::from_slice(proposal_bytes).map_err(codec)?;
    if proposal
        .identity()
        .map_err(CandidateBuildError::GenericProposal)?
        != proposal_id
    {
        return Err(CandidateBuildError::MaterialIdentityMismatch);
    }
    let plan_bytes = cairn_codec::to_vec(&plan).map_err(codec)?;
    let plan_id = plan.identity()?;
    let input_bundle = generic_candidate_input_bundle(&proposal, proposal_bytes, plan.runner())?;
    let input_bundle_bytes = input_bundle.to_bytes()?;
    let input_bundle_id = ContentId::derive(&input_bundle_bytes).map_err(codec)?;
    let environment = DockerExecutionEnvironmentV1::new(plan.image.clone(), Vec::new())?;
    let environment_bytes = environment.to_bytes()?;
    let environment_id = ContentId::derive(&environment_bytes).map_err(codec)?;
    let placement = PlacementRequest::new(
        ExecutionPlatformRequirement::default(),
        plan.worker_pools.clone(),
        plan.capabilities.clone(),
    )?;
    let resources = ResourceRequest::new(plan.timeout, placement)?;
    let contract = JobContract::new(
        job_id,
        input_bundle_id,
        environment_id,
        ExecutionBackend::new(DOCKER_BACKEND)?,
        CommandContract::new(path("bin/run")?, Vec::new(), path("work")?),
        resources,
        plan.network,
        plan.capture.clone(),
    );
    let contract_bytes = cairn_codec::to_vec(&contract).map_err(codec)?;
    let contract_id = ContentId::derive(&contract_bytes).map_err(codec)?;
    let request = CandidateBuildRequestV1 {
        schema_version: 1,
        proposal: proposal_id,
        plan: plan_id,
        job_id,
        input_bundle: input_bundle_id,
        environment: environment_id,
        contract: contract_id,
    };
    Ok(PreparedGenericCandidateBuildJob {
        proposal,
        proposal_bytes: proposal_bytes.to_vec(),
        plan,
        plan_bytes,
        request,
        input_bundle,
        input_bundle_bytes,
        environment,
        environment_bytes,
        contract,
        contract_bytes,
    })
}

fn generic_candidate_input_bundle(
    proposal: &CandidateProposalV1,
    proposal_bytes: &[u8],
    runner: &[u8],
) -> Result<InputBundleV1, CandidateBuildError> {
    let submission = proposal.submission();
    let mut directories = BTreeSet::from([
        "bin".to_owned(),
        "meta".to_owned(),
        "source".to_owned(),
        "work".to_owned(),
    ]);
    for file in submission.files() {
        let mut parent = file.path().as_str();
        while let Some((prefix, _)) = parent.rsplit_once('/') {
            directories.insert(format!("source/{prefix}"));
            parent = prefix;
        }
    }
    let mut entries = directories
        .into_iter()
        .map(|value| {
            Ok(InputBundleEntry::Directory {
                path: path(&value)?,
            })
        })
        .collect::<Result<Vec<_>, CandidateBuildError>>()?;
    entries.push(InputBundleEntry::File {
        path: path("bin/run")?,
        mode: InputFileMode::Executable,
        bytes: runner.to_vec(),
    });
    entries.push(InputBundleEntry::File {
        path: path(GENERIC_CANDIDATE_PUBLICATION_PATH)?,
        mode: InputFileMode::Data,
        bytes: proposal_bytes.to_vec(),
    });
    for file in submission.files() {
        entries.push(InputBundleEntry::File {
            path: path(&format!("source/{}", file.path().as_str()))?,
            mode: InputFileMode::Data,
            bytes: file.source().as_str().as_bytes().to_vec(),
        });
    }
    Ok(InputBundleV1::new(entries)?)
}

fn path(value: &str) -> Result<SandboxPath, CandidateBuildError> {
    SandboxPath::new(value).map_err(CandidateBuildError::Contract)
}

fn codec(error: impl std::fmt::Display) -> CandidateBuildError {
    CandidateBuildError::Codec(error.to_string())
}

/// Failure while binding a generic Candidate proposal to an exact Worker operation.
#[derive(Debug, Error)]
pub enum CandidateBuildError {
    #[error(transparent)]
    GenericProposal(#[from] crate::CandidateExplorationError),
    #[error(transparent)]
    Material(#[from] MaterialFormatError),
    #[error(transparent)]
    Docker(#[from] DockerEnvironmentError),
    #[error(transparent)]
    Contract(#[from] ContractValueError),
    #[error(transparent)]
    Content(#[from] ContentStoreError),
    #[error("Candidate build codec failed: {0}")]
    Codec(String),
    #[error("archived Candidate build material identity changed")]
    MaterialIdentityMismatch,
    #[error("generic Candidate build plan is invalid or noncanonical")]
    InvalidGenericPlan,
}
