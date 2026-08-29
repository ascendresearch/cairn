use std::{env, fs, io::Cursor, path::Path, thread, time::Duration};

use cairn_execution::{
    CapabilityName, CapabilityRequirement, CapabilityValue, CapturePolicy, CommandContract,
    DiagnosticByteLimit, DockerExecutionEnvironmentV1, DockerImageId, EvidenceByteLimit,
    ExecutionBackend, ExecutionEnvironmentArtifact, ExecutionJob, ExecutionJobState,
    ExecutionOutcome, ExecutionPlatformRequirement, ExecutionStderrArtifact,
    ExecutionStdoutArtifact, ExecutionTimeoutMillis, InputBundleArtifact, InputBundleEntry,
    InputBundleV1, InputFileMode, JobContract, NetworkPolicy, OutputByteLimit, PlacementRequest,
    ResourceRequest, SandboxPath, TrustedExecutionEvidence, WorkerPoolName, recover_execution_job,
};
use cairn_migration::{
    CandidateBuildEnvironmentProfileV1, CollectionCandidateProposalArtifact,
    CollectionCandidateRevisionArtifact, prepare_candidate_build_job,
    prepare_candidate_revision_build_job,
};
use cairn_protocol::{
    AssignmentId, AttemptId, CommandId, ContentId, ControlMessageId, JobId, LeaseId, PlacementId,
    ReservationId,
};
use cairn_record::ContentStore;
use cairn_server::{
    ControllerScheduleCommandIds, ControllerScheduleIds, ControllerSchedulingOutcome, ServerConfig,
    release_execution_reservation, schedule_execution_contract,
};
use cairn_store_sqlite::{SqliteContentStore, SqliteEventStore};

const ASCEND_BUILD_INPUT_ID: &str = concat!(
    "cairn:v1:sha256:execution.input-bundle.v1:",
    "1c339b910017791649ab5a5f3ff19007544963a5ee547eb80b245ad45fe5075b"
);

#[test]
#[ignore = "requires the live no-device Ascend build worker and Alloyport ascend-add-v1"]
fn scheduled_ascend_add_compiles_without_device() {
    let config_path = env::var("CAIRN_REAL_CONTROLLER_CONFIG").expect("controller config path");
    let fixture_root = env::var("CAIRN_REAL_ASCEND_FIXTURE_ROOT").expect("Ascend fixture root");
    let image = env::var("CAIRN_REAL_ASCEND_IMAGE_ID").expect("full Ascend image ID");
    let config = load_config(Path::new(&config_path));
    let mut content = SqliteContentStore::open(
        &config.storage.content_database,
        &config.storage.content_directory,
    )
    .expect("controller content store");
    let (input_id, environment_id) =
        archive_ascend_materials(&mut content, Path::new(&fixture_root), image);
    assert_eq!(input_id.to_string(), ASCEND_BUILD_INPUT_ID);
    let job_id = JobId::new();
    let contract = ascend_build_contract(job_id, input_id, environment_id);
    let ids = schedule_ids();
    let scheduled = schedule_execution_contract(&config, &contract, ids).expect("schedule build");
    assert!(matches!(
        scheduled,
        ControllerSchedulingOutcome::Scheduled { .. }
    ));

    await_build(&config, &content, job_id, ids, input_id);
}

#[test]
#[ignore = "requires the live no-device Ascend build worker and an exact archived Candidate proposal"]
fn scheduled_exact_candidate_proposal_reaches_remote_ascend_build() {
    let config_path = env::var("CAIRN_REAL_CONTROLLER_CONFIG").expect("controller config path");
    let candidate_state =
        env::var("CAIRN_REAL_CANDIDATE_STATE_DIR").expect("Candidate episode state directory");
    let proposal_id: ContentId<CollectionCandidateProposalArtifact> =
        env::var("CAIRN_REAL_CANDIDATE_PROPOSAL_ID")
            .expect("Candidate proposal ID")
            .parse()
            .expect("typed Candidate proposal ID");
    let image = env::var("CAIRN_REAL_ASCEND_IMAGE_ID").expect("full Ascend image ID");
    let candidate_content = SqliteContentStore::open(
        Path::new(&candidate_state).join("content.db"),
        Path::new(&candidate_state).join("cas"),
    )
    .expect("Candidate content store");
    let proposal_bytes = read_content(&candidate_content, &proposal_id);
    let config = load_config(Path::new(&config_path));
    let mut controller_content = SqliteContentStore::open(
        &config.storage.content_database,
        &config.storage.content_directory,
    )
    .expect("controller content store");
    let job_id = JobId::new();
    let prepared = prepare_candidate_build_job(
        job_id,
        &proposal_bytes,
        proposal_id,
        DockerImageId::new(image).expect("full image ID"),
        CandidateBuildEnvironmentProfileV1::AscendCann910Beta1Dav3510NoDevice,
    )
    .expect("prepare exact Candidate build");
    prepared
        .archive_materials(&mut controller_content)
        .expect("archive Candidate build materials");
    let ids = schedule_ids();
    let scheduled = schedule_execution_contract(&config, prepared.contract(), ids)
        .expect("schedule Candidate build");
    assert!(matches!(
        scheduled,
        ControllerSchedulingOutcome::Scheduled { .. }
    ));

    await_candidate_build(
        &config,
        job_id,
        ids,
        &prepared.proposal_id().to_string(),
        prepared.input_bundle_id(),
        prepared.environment_id(),
        prepared.contract_id(),
    );
}

#[test]
#[ignore = "requires the live no-device Ascend build worker and an exact archived Candidate revision"]
fn scheduled_exact_candidate_revision_reaches_remote_ascend_build() {
    let config_path = env::var("CAIRN_REAL_CONTROLLER_CONFIG").expect("controller config path");
    let revision_state = env::var("CAIRN_REAL_CANDIDATE_REVISION_STATE_DIR")
        .expect("Candidate revision episode state directory");
    let revision_id: ContentId<CollectionCandidateRevisionArtifact> =
        env::var("CAIRN_REAL_CANDIDATE_REVISION_ID")
            .expect("Candidate revision ID")
            .parse()
            .expect("typed Candidate revision ID");
    let image = env::var("CAIRN_REAL_ASCEND_IMAGE_ID").expect("full Ascend image ID");
    let revision_content = SqliteContentStore::open(
        Path::new(&revision_state).join("content.db"),
        Path::new(&revision_state).join("cas"),
    )
    .expect("Candidate revision content store");
    let revision_bytes = read_content(&revision_content, &revision_id);
    let config = load_config(Path::new(&config_path));
    let mut controller_content = SqliteContentStore::open(
        &config.storage.content_database,
        &config.storage.content_directory,
    )
    .expect("controller content store");
    let job_id = JobId::new();
    let prepared = prepare_candidate_revision_build_job(
        job_id,
        &revision_bytes,
        revision_id,
        DockerImageId::new(image).expect("full image ID"),
        CandidateBuildEnvironmentProfileV1::AscendCann910Beta1Dav3510NoDevice,
    )
    .expect("prepare exact Candidate revision build");
    prepared
        .archive_materials(&mut controller_content)
        .expect("archive Candidate revision build materials");
    let ids = schedule_ids();
    let scheduled = schedule_execution_contract(&config, prepared.contract(), ids)
        .expect("schedule Candidate revision build");
    assert!(matches!(
        scheduled,
        ControllerSchedulingOutcome::Scheduled { .. }
    ));

    await_candidate_build(
        &config,
        job_id,
        ids,
        &prepared.revision_id().to_string(),
        prepared.input_bundle_id(),
        prepared.environment_id(),
        prepared.contract_id(),
    );
}

fn archive_ascend_materials(
    content: &mut SqliteContentStore,
    root: &Path,
    image: String,
) -> (
    ContentId<InputBundleArtifact>,
    ContentId<ExecutionEnvironmentArtifact>,
) {
    let entries = vec![
        directory("bin"),
        directory("source"),
        file("bin/run", InputFileMode::Executable, build_runner()),
        file(
            "source/CMakeLists.txt",
            InputFileMode::Data,
            cmake_project(),
        ),
        file(
            "source/add_custom.cpp",
            InputFileMode::Data,
            fs::read(root.join("add_custom.cpp")).expect("read Ascend C source"),
        ),
        file(
            "source/add_custom_tiling.h",
            InputFileMode::Data,
            fs::read(root.join("image/harness/project/add_custom_tiling.h"))
                .expect("read tiling header"),
        ),
    ];
    let bundle = InputBundleV1::new(entries).expect("Ascend build input bundle");
    let input_id = content
        .put::<InputBundleArtifact>(&mut Cursor::new(bundle.to_bytes().expect("input bytes")))
        .expect("archive Ascend input")
        .content_id;
    let environment = DockerExecutionEnvironmentV1::new(
        DockerImageId::new(image).expect("full image ID"),
        Vec::new(),
    )
    .expect("Docker environment");
    let environment_id = content
        .put::<ExecutionEnvironmentArtifact>(&mut Cursor::new(
            environment.to_bytes().expect("environment bytes"),
        ))
        .expect("archive environment")
        .content_id;
    (input_id, environment_id)
}

fn directory(path: &str) -> InputBundleEntry {
    InputBundleEntry::Directory {
        path: SandboxPath::new(path).expect("bundle directory"),
    }
}

fn file(path: &str, mode: InputFileMode, bytes: Vec<u8>) -> InputBundleEntry {
    InputBundleEntry::File {
        path: SandboxPath::new(path).expect("bundle file"),
        mode,
        bytes,
    }
}

fn build_runner() -> Vec<u8> {
    b"#!/bin/sh\nset -eu\ncp -R /cairn/input/source/. /cairn/work/source\ncp /cairn/work/source/add_custom.cpp /cairn/work/source/add_custom.asc\ncmake -S /cairn/work/source -B /cairn/work/build 1>&2\ncmake --build /cairn/work/build --target add_custom --parallel 1 1>&2\nprintf '%s\\n' 'PASS fixture=ascend-add-v1 build=complete device=none'\n".to_vec()
}

fn cmake_project() -> Vec<u8> {
    b"cmake_minimum_required(VERSION 3.24)\nfind_package(ASC REQUIRED)\nproject(cairn_ascend_add_build LANGUAGES ASC CXX)\nadd_library(add_custom STATIC add_custom.asc)\ntarget_include_directories(add_custom PRIVATE ${CMAKE_CURRENT_SOURCE_DIR})\ntarget_compile_options(add_custom PRIVATE $<$<COMPILE_LANGUAGE:ASC>:--npu-arch=dav-3510>)\n".to_vec()
}

fn ascend_build_contract(
    job_id: JobId,
    input_id: ContentId<InputBundleArtifact>,
    environment_id: ContentId<ExecutionEnvironmentArtifact>,
) -> JobContract {
    JobContract::new(
        job_id,
        input_id,
        environment_id,
        ExecutionBackend::new("docker-v1").expect("Docker backend"),
        CommandContract::new(
            SandboxPath::new("bin/run").expect("program"),
            Vec::new(),
            SandboxPath::new("work").expect("working directory"),
        ),
        ResourceRequest::new(
            ExecutionTimeoutMillis::new(120_000).expect("timeout"),
            PlacementRequest::new(
                ExecutionPlatformRequirement::default(),
                vec![WorkerPoolName::new("npu-build").expect("build pool")],
                build_capabilities(),
            )
            .expect("placement"),
        )
        .expect("resources"),
        NetworkPolicy::Disabled,
        CapturePolicy::new(
            OutputByteLimit::new(4_096).expect("stdout limit"),
            OutputByteLimit::new(1_048_576).expect("stderr limit"),
            DiagnosticByteLimit::new(16_384).expect("diagnostic limit"),
            EvidenceByteLimit::new(16_384).expect("evidence limit"),
            Vec::new(),
        )
        .expect("capture"),
    )
}

fn build_capabilities() -> Vec<CapabilityRequirement> {
    [
        ("execution.role", "build"),
        ("toolchain.architecture", "dav-3510"),
        ("toolchain.cann", "9.1.0-beta.1"),
        ("toolchain.vendor", "ascend"),
    ]
    .into_iter()
    .map(|(name, value)| CapabilityRequirement {
        name: CapabilityName::new(name).expect("capability name"),
        value: CapabilityValue::new(value).expect("capability value"),
    })
    .collect()
}

fn await_build(
    config: &ServerConfig,
    content: &SqliteContentStore,
    job_id: JobId,
    ids: ControllerScheduleIds,
    input_id: ContentId<InputBundleArtifact>,
) {
    let job = ExecutionJob::new(job_id).expect("job stream");
    let events = SqliteEventStore::open(&config.storage.event_database).expect("event store");
    for _ in 0..1_200 {
        match recover_execution_job(&events, content, &job).expect("recover build") {
            ExecutionJobState::Completed {
                receipt_id,
                receipt,
            } => {
                let stdout = read_content::<ExecutionStdoutArtifact>(content, &receipt.stdout_id());
                let stderr = read_content::<ExecutionStderrArtifact>(content, &receipt.stderr_id());
                release_execution_reservation(config, ids.reservation_id, &CommandId::new())
                    .expect("release terminal reservation");
                assert_eq!(
                    receipt.outcome(),
                    ExecutionOutcome::Succeeded,
                    "Ascend build stderr:\n{}",
                    String::from_utf8_lossy(&stderr)
                );
                assert_eq!(
                    String::from_utf8(stdout).expect("UTF-8 stdout").trim(),
                    "PASS fixture=ascend-add-v1 build=complete device=none"
                );
                let evidence_bytes = read_content(content, &receipt.evidence_id());
                let evidence: TrustedExecutionEvidence =
                    cairn_codec::from_slice(&evidence_bytes).expect("trusted evidence");
                assert!(
                    evidence
                        .observations()
                        .iter()
                        .any(|observation| { observation.as_str() == "docker:accelerator:none" })
                );
                eprintln!(
                    "real Ascend build completed: job={job_id} input={input_id} receipt={receipt_id}"
                );
                return;
            }
            ExecutionJobState::NotStarted { diagnostic, .. }
            | ExecutionJobState::Ambiguous { diagnostic, .. } => {
                panic!("real Ascend build did not complete: {diagnostic}")
            }
            ExecutionJobState::NotFound
            | ExecutionJobState::ReadyToStart(_)
            | ExecutionJobState::InDoubt { .. } => thread::sleep(Duration::from_millis(100)),
        }
    }
    panic!("real Ascend build did not become terminal within 120 seconds");
}

fn await_candidate_build(
    config: &ServerConfig,
    job_id: JobId,
    ids: ControllerScheduleIds,
    publication: &str,
    input_id: ContentId<InputBundleArtifact>,
    environment_id: ContentId<ExecutionEnvironmentArtifact>,
    contract_id: ContentId<cairn_execution::JobContractArtifact>,
) {
    let job = ExecutionJob::new(job_id).expect("job stream");
    for _ in 0..1_200 {
        let content = SqliteContentStore::open(
            &config.storage.content_database,
            &config.storage.content_directory,
        )
        .expect("controller content store");
        let events = SqliteEventStore::open(&config.storage.event_database).expect("event store");
        match recover_execution_job(&events, &content, &job).expect("recover Candidate build") {
            ExecutionJobState::Completed {
                receipt_id,
                receipt,
            } => {
                let stdout =
                    read_content::<ExecutionStdoutArtifact>(&content, &receipt.stdout_id());
                let stderr =
                    read_content::<ExecutionStderrArtifact>(&content, &receipt.stderr_id());
                let evidence_bytes = read_content(&content, &receipt.evidence_id());
                let evidence: TrustedExecutionEvidence =
                    cairn_codec::from_slice(&evidence_bytes).expect("trusted evidence");
                release_execution_reservation(config, ids.reservation_id, &CommandId::new())
                    .expect("release terminal reservation");
                assert_eq!(receipt.job_id(), job_id);
                assert_eq!(receipt.attempt_id(), ids.attempt_id);
                assert_eq!(receipt.contract_id(), contract_id);
                assert!(
                    evidence
                        .observations()
                        .iter()
                        .any(|observation| observation.as_str() == "docker:accelerator:none")
                );
                match receipt.outcome() {
                    ExecutionOutcome::Succeeded => assert_eq!(
                        String::from_utf8(stdout).expect("UTF-8 stdout").trim(),
                        "PASS candidate-build=complete device=none"
                    ),
                    ExecutionOutcome::SubjectFailed => eprintln!(
                        "exact Candidate build divergence (untrusted stderr):\n{}",
                        diagnostic_preview(&stderr)
                    ),
                    outcome => panic!(
                        "Candidate build has no subject conclusion: {outcome:?}\n{}",
                        diagnostic_preview(&stderr)
                    ),
                }
                assert_recoverable_candidate_terminal(config, &job, receipt_id);
                eprintln!(
                    "real Candidate build terminal: job={job_id} attempt={} publication={} input={} environment={} contract={} receipt={receipt_id} outcome={:?}",
                    ids.attempt_id,
                    publication,
                    input_id,
                    environment_id,
                    contract_id,
                    receipt.outcome()
                );
                return;
            }
            ExecutionJobState::NotStarted { diagnostic, .. }
            | ExecutionJobState::Ambiguous { diagnostic, .. } => {
                panic!("real Candidate build did not complete: {diagnostic}")
            }
            ExecutionJobState::NotFound
            | ExecutionJobState::ReadyToStart(_)
            | ExecutionJobState::InDoubt { .. } => thread::sleep(Duration::from_millis(100)),
        }
    }
    panic!("real Candidate build did not become terminal within 120 seconds");
}

fn assert_recoverable_candidate_terminal(
    config: &ServerConfig,
    job: &ExecutionJob,
    expected_receipt: ContentId<cairn_execution::ExecutionReceiptArtifact>,
) {
    let content = SqliteContentStore::open(
        &config.storage.content_database,
        &config.storage.content_directory,
    )
    .expect("reopened controller content store");
    let events =
        SqliteEventStore::open(&config.storage.event_database).expect("reopened event store");
    let ExecutionJobState::Completed { receipt_id, .. } =
        recover_execution_job(&events, &content, job).expect("recover terminal after reopen")
    else {
        panic!("Candidate terminal was not recoverable after reopening Controller stores");
    };
    assert_eq!(receipt_id, expected_receipt);
}

fn diagnostic_preview(bytes: &[u8]) -> String {
    String::from_utf8_lossy(&bytes[..bytes.len().min(8_192)]).into_owned()
}

fn load_config(path: &Path) -> ServerConfig {
    let mut config: ServerConfig =
        serde_json::from_slice(&fs::read(path).expect("read controller configuration"))
            .expect("decode controller configuration");
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    for storage_path in [
        &mut config.storage.event_database,
        &mut config.storage.content_database,
        &mut config.storage.content_directory,
    ] {
        if storage_path.is_relative() {
            *storage_path = base.join(&*storage_path);
        }
    }
    config
}

fn schedule_ids() -> ControllerScheduleIds {
    ControllerScheduleIds {
        attempt_id: AttemptId::new(),
        placement_id: PlacementId::new(),
        reservation_id: ReservationId::new(),
        assignment_id: AssignmentId::new(),
        lease_id: LeaseId::new(),
        offer_message_id: ControlMessageId::new(),
        start_message_id: ControlMessageId::new(),
        commands: ControllerScheduleCommandIds {
            authorize_attempt: CommandId::new(),
            reserve_placement: CommandId::new(),
            grant_assignment: CommandId::new(),
            enqueue_offer: CommandId::new(),
        },
    }
}

fn read_content<T: cairn_protocol::ContentType>(
    content: &SqliteContentStore,
    id: &ContentId<T>,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    content.write_to(id, &mut bytes).expect("read content");
    bytes
}
