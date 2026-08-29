//! Exact Candidate proposal materialization for the existing remote build worker path.

use std::{collections::BTreeSet, io::Cursor};

use cairn_execution::{
    CapabilityName, CapabilityRequirement, CapabilityValue, CapturePolicy, CommandContract,
    ContractValueError, DOCKER_BACKEND, DiagnosticByteLimit, DockerEnvironmentError,
    DockerExecutionEnvironmentV1, DockerImageId, EvidenceByteLimit, ExecutionBackend,
    ExecutionEnvironmentArtifact, ExecutionPlatformRequirement, ExecutionTimeoutMillis,
    InputBundleArtifact, InputBundleEntry, InputBundleV1, InputFileMode, JobContract,
    JobContractArtifact, MaterialFormatError, NetworkPolicy, OutputByteLimit, PlacementRequest,
    ResourceRequest, SandboxPath, WorkerPoolName,
};
use cairn_protocol::{ContentId, JobId};
use cairn_record::{ContentStore, ContentStoreError};
use thiserror::Error;

use crate::{
    CandidateEpisodeError, CollectionCandidateProposalArtifact, CollectionCandidateProposalV1,
    validate_archived_collection_candidate_proposal,
};

const BUILD_RUNNER: &[u8] = b"#!/bin/sh\nset -eu\ncp -R /cairn/input/source/. /cairn/work/source\ncmake -S /cairn/work/source -B /cairn/work/build 1>&2\ncmake --build /cairn/work/build --parallel 1 1>&2\nprintf '%s\\n' 'PASS candidate-build=complete device=none'\n";

/// Closed product-owned environment-under-test selection for the first Candidate build.
///
/// This is not the user-selected migration target. It projects one already-probed remote build
/// lane into a domain-neutral worker placement and immutable Docker environment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CandidateBuildEnvironmentProfileV1 {
    /// Ascend CANN 9.1.0 beta 1, `dav-3510`, compilation only, with no accelerator exposed.
    AscendCann910Beta1Dav3510NoDevice,
}

impl CandidateBuildEnvironmentProfileV1 {
    fn worker_pool(self) -> Result<WorkerPoolName, CandidateBuildError> {
        match self {
            Self::AscendCann910Beta1Dav3510NoDevice => Ok(WorkerPoolName::new("npu-build")?),
        }
    }

    fn capabilities(self) -> Result<Vec<CapabilityRequirement>, CandidateBuildError> {
        let raw = match self {
            Self::AscendCann910Beta1Dav3510NoDevice => [
                ("execution.role", "build"),
                ("toolchain.architecture", "dav-3510"),
                ("toolchain.cann", "9.1.0-beta.1"),
                ("toolchain.vendor", "ascend"),
            ],
        };
        raw.into_iter()
            .map(|(name, value_text)| {
                Ok(CapabilityRequirement {
                    name: CapabilityName::new(name)?,
                    value: CapabilityValue::new(value_text)?,
                })
            })
            .collect()
    }
}

/// Exact proposal, material tree, environment, and generic contract ready for Controller archival.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedCandidateBuildJob {
    proposal: CollectionCandidateProposalV1,
    proposal_bytes: Vec<u8>,
    proposal_id: ContentId<CollectionCandidateProposalArtifact>,
    input_bundle: InputBundleV1,
    input_bundle_bytes: Vec<u8>,
    input_bundle_id: ContentId<InputBundleArtifact>,
    environment: DockerExecutionEnvironmentV1,
    environment_bytes: Vec<u8>,
    environment_id: ContentId<ExecutionEnvironmentArtifact>,
    contract: JobContract,
    contract_bytes: Vec<u8>,
    contract_id: ContentId<JobContractArtifact>,
}

impl PreparedCandidateBuildJob {
    #[must_use]
    pub const fn proposal(&self) -> &CollectionCandidateProposalV1 {
        &self.proposal
    }

    #[must_use]
    pub fn proposal_bytes(&self) -> &[u8] {
        &self.proposal_bytes
    }

    #[must_use]
    pub const fn proposal_id(&self) -> ContentId<CollectionCandidateProposalArtifact> {
        self.proposal_id
    }

    #[must_use]
    pub const fn input_bundle(&self) -> &InputBundleV1 {
        &self.input_bundle
    }

    #[must_use]
    pub fn input_bundle_bytes(&self) -> &[u8] {
        &self.input_bundle_bytes
    }

    #[must_use]
    pub const fn input_bundle_id(&self) -> ContentId<InputBundleArtifact> {
        self.input_bundle_id
    }

    #[must_use]
    pub const fn environment(&self) -> &DockerExecutionEnvironmentV1 {
        &self.environment
    }

    #[must_use]
    pub fn environment_bytes(&self) -> &[u8] {
        &self.environment_bytes
    }

    #[must_use]
    pub const fn environment_id(&self) -> ContentId<ExecutionEnvironmentArtifact> {
        self.environment_id
    }

    #[must_use]
    pub const fn contract(&self) -> &JobContract {
        &self.contract
    }

    #[must_use]
    pub fn contract_bytes(&self) -> &[u8] {
        &self.contract_bytes
    }

    #[must_use]
    pub const fn contract_id(&self) -> ContentId<JobContractArtifact> {
        self.contract_id
    }

    /// Archives the exact selected proposal and worker material roots into Controller content.
    ///
    /// The normal scheduler separately archives and authorizes the returned generic contract.
    ///
    /// # Errors
    ///
    /// Fails if storage changes any typed identity or cannot durably publish exact bytes.
    pub fn archive_materials<C: ContentStore>(
        &self,
        content: &mut C,
    ) -> Result<(), CandidateBuildError> {
        let proposal = content
            .put::<CollectionCandidateProposalArtifact>(&mut Cursor::new(&self.proposal_bytes))?
            .content_id;
        let input = content
            .put::<InputBundleArtifact>(&mut Cursor::new(&self.input_bundle_bytes))?
            .content_id;
        let environment = content
            .put::<ExecutionEnvironmentArtifact>(&mut Cursor::new(&self.environment_bytes))?
            .content_id;
        if proposal != self.proposal_id
            || input != self.input_bundle_id
            || environment != self.environment_id
        {
            return Err(CandidateBuildError::MaterialIdentityMismatch);
        }
        Ok(())
    }
}

/// Builds the exact immutable input and execution contract for one archived Candidate proposal.
///
/// # Errors
///
/// Rejects an unbound/noncanonical proposal, invalid image, path/material construction failure, or
/// any identity that cannot be represented by the current V1 execution contract.
pub fn prepare_candidate_build_job(
    job_id: JobId,
    proposal_bytes: &[u8],
    proposal_id: ContentId<CollectionCandidateProposalArtifact>,
    image: DockerImageId,
    profile: CandidateBuildEnvironmentProfileV1,
) -> Result<PreparedCandidateBuildJob, CandidateBuildError> {
    let proposal = validate_archived_collection_candidate_proposal(proposal_bytes, proposal_id)?;
    let proposal_bytes = proposal_bytes.to_vec();
    let input_bundle = candidate_input_bundle(&proposal, &proposal_bytes)?;
    let input_bundle_bytes = input_bundle.to_bytes()?;
    let input_bundle_id = ContentId::derive(&input_bundle_bytes).map_err(codec)?;
    let environment = DockerExecutionEnvironmentV1::new(image, Vec::new())?;
    let environment_bytes = environment.to_bytes()?;
    let environment_id = ContentId::derive(&environment_bytes).map_err(codec)?;
    let placement = PlacementRequest::new(
        ExecutionPlatformRequirement::default(),
        vec![profile.worker_pool()?],
        profile.capabilities()?,
    )?;
    let resources = ResourceRequest::new(ExecutionTimeoutMillis::new(120_000)?, placement)?;
    let capture = CapturePolicy::new(
        OutputByteLimit::new(4_096)?,
        OutputByteLimit::new(1_048_576)?,
        DiagnosticByteLimit::new(16_384)?,
        EvidenceByteLimit::new(16_384)?,
        Vec::new(),
    )?;
    let contract = JobContract::new(
        job_id,
        input_bundle_id,
        environment_id,
        ExecutionBackend::new(DOCKER_BACKEND)?,
        CommandContract::new(path("bin/run")?, Vec::new(), path("work")?),
        resources,
        NetworkPolicy::Disabled,
        capture,
    );
    let contract_bytes = cairn_codec::to_vec(&contract).map_err(codec)?;
    let contract_id = ContentId::derive(&contract_bytes).map_err(codec)?;
    Ok(PreparedCandidateBuildJob {
        proposal,
        proposal_bytes,
        proposal_id,
        input_bundle,
        input_bundle_bytes,
        input_bundle_id,
        environment,
        environment_bytes,
        environment_id,
        contract,
        contract_bytes,
        contract_id,
    })
}

fn candidate_input_bundle(
    proposal: &CollectionCandidateProposalV1,
    proposal_bytes: &[u8],
) -> Result<InputBundleV1, CandidateBuildError> {
    let mut directories =
        BTreeSet::from(["bin".to_owned(), "meta".to_owned(), "source".to_owned()]);
    for file in proposal.submission().files() {
        let mut parent = file.path().as_str();
        while let Some((prefix, _)) = parent.rsplit_once('/') {
            directories.insert(format!("source/{prefix}"));
            parent = prefix;
        }
    }
    let mut entries = directories
        .into_iter()
        .map(|directory| {
            Ok(InputBundleEntry::Directory {
                path: path(&directory)?,
            })
        })
        .collect::<Result<Vec<_>, CandidateBuildError>>()?;
    entries.push(InputBundleEntry::File {
        path: path("bin/run")?,
        mode: InputFileMode::Executable,
        bytes: BUILD_RUNNER.to_vec(),
    });
    entries.push(InputBundleEntry::File {
        path: path("meta/candidate-proposal.json")?,
        mode: InputFileMode::Data,
        bytes: proposal_bytes.to_vec(),
    });
    for file in proposal.submission().files() {
        entries.push(InputBundleEntry::File {
            path: path(&format!("source/{}", file.path().as_str()))?,
            mode: InputFileMode::Data,
            bytes: file.source().as_str().as_bytes().to_vec(),
        });
    }
    Ok(InputBundleV1::new(entries)?)
}

fn path(value_text: &str) -> Result<SandboxPath, CandidateBuildError> {
    Ok(SandboxPath::new(value_text)?)
}

fn codec(error: impl std::fmt::Display) -> CandidateBuildError {
    CandidateBuildError::Codec(error.to_string())
}

/// Failure while binding a Candidate proposal to the remote generic execution path.
#[derive(Debug, Error)]
pub enum CandidateBuildError {
    #[error(transparent)]
    Proposal(#[from] CandidateEpisodeError),
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
}

#[cfg(test)]
mod tests {
    use cairn_execution::{InputBundleEntry, InputFileMode, NetworkPolicy};
    use cairn_protocol::{ContentId, ContentType, EpisodeId, JobId};
    use serde_json::json;

    use super::*;
    use crate::{CollectionCandidateSearchInputArtifact, SirResolvedRuntimeModelArtifact};

    fn id<T: ContentType>(label: &[u8]) -> ContentId<T> {
        ContentId::derive(label).expect("content ID")
    }

    fn proposal_bytes() -> Vec<u8> {
        cairn_codec::to_vec(&json!({
            "schema_version":1,
            "search_input":id::<CollectionCandidateSearchInputArtifact>(b"search"),
            "episode_id":EpisodeId::new(),
            "model_configuration":id::<SirResolvedRuntimeModelArtifact>(b"model"),
            "submission":{
                "schema_version":1,
                "files":[
                    {"path":"CMakeLists.txt","source":"cmake_minimum_required(VERSION 3.24)\nproject(candidate LANGUAGES CXX)\nadd_library(candidate STATIC src/kernel.cpp)\n"},
                    {"path":"src/kernel.cpp","source":"int candidate() { return 7; }\n"}
                ],
                "primary_source":"src/kernel.cpp",
                "explanation":"Exact build-only fixture."
            }
        }))
        .expect("proposal bytes")
    }

    fn prepared(job_id: JobId, bytes: &[u8], image_suffix: char) -> PreparedCandidateBuildJob {
        let proposal_id = ContentId::derive(bytes).expect("proposal ID");
        prepare_candidate_build_job(
            job_id,
            bytes,
            proposal_id,
            DockerImageId::new(format!("sha256:{}", image_suffix.to_string().repeat(64)))
                .expect("image"),
            CandidateBuildEnvironmentProfileV1::AscendCann910Beta1Dav3510NoDevice,
        )
        .expect("prepared build")
    }

    fn assert_remote_build_contract(prepared: &PreparedCandidateBuildJob) {
        assert_eq!(
            prepared.contract().input_bundle_id(),
            prepared.input_bundle_id()
        );
        assert_eq!(
            prepared.contract().environment_id(),
            prepared.environment_id()
        );
        assert_eq!(prepared.contract().backend().as_str(), DOCKER_BACKEND);
        assert_eq!(prepared.contract().network(), NetworkPolicy::Disabled);
        assert_eq!(
            prepared.environment().image().as_str(),
            format!("sha256:{}", "a".repeat(64))
        );
        assert_eq!(prepared.contract().command().program().as_str(), "bin/run");
        assert_eq!(
            prepared.contract().command().working_directory().as_str(),
            "work"
        );
        assert!(
            prepared
                .contract()
                .resources()
                .quantitative()
                .accelerator()
                .is_none()
        );
        assert_eq!(prepared.contract().resources().timeout().get(), 120_000);
        assert_eq!(prepared.contract().capture().stdout_limit().get(), 4_096);
        assert_eq!(
            prepared.contract().capture().stderr_limit().get(),
            1_048_576
        );
        assert_eq!(
            prepared.contract().capture().diagnostic_limit().get(),
            16_384
        );
        assert_eq!(prepared.contract().capture().evidence_limit().get(), 16_384);
        assert!(prepared.contract().capture().expected_outputs().is_empty());
        assert_eq!(
            prepared
                .contract()
                .resources()
                .placement()
                .allowed_worker_pools()[0]
                .as_str(),
            "npu-build"
        );
        let capabilities = prepared
            .contract()
            .resources()
            .placement()
            .capabilities()
            .iter()
            .map(|requirement| (requirement.name.as_str(), requirement.value.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(
            capabilities,
            vec![
                ("execution.role", "build"),
                ("toolchain.architecture", "dav-3510"),
                ("toolchain.cann", "9.1.0-beta.1"),
                ("toolchain.vendor", "ascend"),
            ]
        );
    }

    #[test]
    fn exact_proposal_becomes_remote_no_device_build_contract_without_rewriting_source() {
        let bytes = proposal_bytes();
        let prepared = prepared(JobId::new(), &bytes, 'a');
        assert_eq!(prepared.proposal_bytes(), bytes);
        assert_remote_build_contract(&prepared);

        let files = prepared
            .input_bundle()
            .entries()
            .iter()
            .filter_map(|entry| match entry {
                InputBundleEntry::File { path, mode, bytes } => {
                    Some((path.as_str(), *mode, bytes.as_slice()))
                }
                InputBundleEntry::Directory { .. } => None,
            })
            .collect::<Vec<_>>();
        assert!(files.contains(&(
            "source/CMakeLists.txt",
            InputFileMode::Data,
            b"cmake_minimum_required(VERSION 3.24)\nproject(candidate LANGUAGES CXX)\nadd_library(candidate STATIC src/kernel.cpp)\n".as_slice()
        )));
        assert!(files.contains(&(
            "source/src/kernel.cpp",
            InputFileMode::Data,
            b"int candidate() { return 7; }\n".as_slice()
        )));
        assert!(files.contains(&(
            "meta/candidate-proposal.json",
            InputFileMode::Data,
            prepared.proposal_bytes()
        )));
        let runner = files
            .iter()
            .find(|(path, _, _)| *path == "bin/run")
            .expect("runner")
            .2;
        assert_eq!(runner, BUILD_RUNNER);
        for forbidden in [
            b".asc".as_slice(),
            b"add_custom".as_slice(),
            b"tiling".as_slice(),
        ] {
            assert!(
                !runner
                    .windows(forbidden.len())
                    .any(|window| window == forbidden)
            );
        }
    }

    #[test]
    fn proposal_publication_identity_and_material_identities_fail_closed() {
        let bytes = proposal_bytes();
        let wrong = id::<CollectionCandidateProposalArtifact>(b"wrong proposal");
        assert!(matches!(
            prepare_candidate_build_job(
                JobId::new(),
                &bytes,
                wrong,
                DockerImageId::new(format!("sha256:{}", "b".repeat(64))).expect("image"),
                CandidateBuildEnvironmentProfileV1::AscendCann910Beta1Dav3510NoDevice,
            ),
            Err(CandidateBuildError::Proposal(
                CandidateEpisodeError::ProposalBindingMismatch
            ))
        ));
        let noncanonical = [bytes.as_slice(), b"\n"].concat();
        let noncanonical_id = ContentId::derive(&noncanonical).expect("noncanonical ID");
        assert!(
            prepare_candidate_build_job(
                JobId::new(),
                &noncanonical,
                noncanonical_id,
                DockerImageId::new(format!("sha256:{}", "b".repeat(64))).expect("image"),
                CandidateBuildEnvironmentProfileV1::AscendCann910Beta1Dav3510NoDevice,
            )
            .is_err()
        );
        let mut non_v1: serde_json::Value = cairn_codec::from_slice(&bytes).expect("proposal JSON");
        non_v1["schema_version"] = serde_json::json!(2);
        let non_v1 = cairn_codec::to_vec(&non_v1).expect("canonical non-V1 bytes");
        let non_v1_id = ContentId::derive(&non_v1).expect("non-V1 ID");
        assert!(
            prepare_candidate_build_job(
                JobId::new(),
                &non_v1,
                non_v1_id,
                DockerImageId::new(format!("sha256:{}", "b".repeat(64))).expect("image"),
                CandidateBuildEnvironmentProfileV1::AscendCann910Beta1Dav3510NoDevice,
            )
            .is_err()
        );

        let job_id = JobId::new();
        let first = prepared(job_id, &bytes, 'a');
        let changed_image = prepared(job_id, &bytes, 'b');
        assert_eq!(first.input_bundle_id(), changed_image.input_bundle_id());
        assert_ne!(first.environment_id(), changed_image.environment_id());
        assert_ne!(first.contract_id(), changed_image.contract_id());

        let mut changed_proposal: serde_json::Value =
            cairn_codec::from_slice(&bytes).expect("proposal JSON");
        changed_proposal["submission"]["files"][1]["source"] =
            serde_json::json!("int candidate() { return 8; }\n");
        let changed_proposal =
            cairn_codec::to_vec(&changed_proposal).expect("changed proposal bytes");
        let changed_proposal = prepared(job_id, &changed_proposal, 'a');
        assert_eq!(first.environment_id(), changed_proposal.environment_id());
        assert_ne!(first.proposal_id(), changed_proposal.proposal_id());
        assert_ne!(first.input_bundle_id(), changed_proposal.input_bundle_id());
        assert_ne!(first.contract_id(), changed_proposal.contract_id());
    }
}
