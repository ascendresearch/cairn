use std::{env, fs, io::Cursor, path::Path, thread, time::Duration};

use cairn_execution::{
    CapabilityName, CapabilityRequirement, CapabilityValue, CapturePolicy, CommandContract,
    DiagnosticByteLimit, DockerExecutionEnvironmentV1, DockerImageId, EvidenceByteLimit,
    ExecutionBackend, ExecutionEnvironmentArtifact, ExecutionJob, ExecutionJobState,
    ExecutionOutcome, ExecutionPlatformRequirement, ExecutionStdoutArtifact,
    ExecutionTimeoutMillis, InputBundleArtifact, InputBundleEntry, InputBundleV1, InputFileMode,
    JobContract, NetworkPolicy, OutputByteLimit, PlacementRequest, ResourceRequest, SandboxPath,
    TrustedExecutionEvidence, WorkerPoolName, recover_execution_job,
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

#[test]
#[ignore = "requires CAIRN_REAL_CONTROLLER_CONFIG, CAIRN_REAL_GPU_IMAGE_ID, a live controller, and the enrolled GB10 worker"]
fn scheduled_gpu_probe_returns_device_bound_evidence() {
    let config_path = env::var("CAIRN_REAL_CONTROLLER_CONFIG").expect("controller config path");
    let image = env::var("CAIRN_REAL_GPU_IMAGE_ID").expect("full GPU image ID");
    let config = load_config(Path::new(&config_path));
    let mut content = SqliteContentStore::open(
        &config.storage.content_database,
        &config.storage.content_directory,
    )
    .expect("controller content store");
    let (input_id, environment_id) = archive_probe_materials(&mut content, image);
    let job_id = JobId::new();
    let contract = gpu_probe_contract(job_id, input_id, environment_id);
    let ids = schedule_ids();
    let scheduled = schedule_execution_contract(&config, &contract, ids).expect("schedule probe");
    assert!(matches!(
        scheduled,
        ControllerSchedulingOutcome::Scheduled { .. }
    ));

    await_gpu_probe(&config, &content, job_id, ids);
}

fn load_config(path: &Path) -> ServerConfig {
    let mut config =
        serde_json::from_slice(&fs::read(path).expect("read controller configuration"))
            .expect("decode controller configuration");
    resolve_storage(&mut config, path);
    config
}

fn archive_probe_materials(
    content: &mut SqliteContentStore,
    image: String,
) -> (
    ContentId<InputBundleArtifact>,
    ContentId<ExecutionEnvironmentArtifact>,
) {
    let input = InputBundleV1::new(vec![
        InputBundleEntry::Directory {
            path: SandboxPath::new("bin").expect("bin path"),
        },
        InputBundleEntry::File {
            path: SandboxPath::new("bin/probe").expect("probe path"),
            mode: InputFileMode::Executable,
            bytes: b"#!/bin/sh\nexec nvidia-smi --query-gpu=name --format=csv,noheader\n".to_vec(),
        },
    ])
    .expect("input bundle");
    let input_id = content
        .put::<InputBundleArtifact>(&mut Cursor::new(input.to_bytes().expect("input bytes")))
        .expect("archive input")
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

fn gpu_probe_contract(
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
            SandboxPath::new("bin/probe").expect("program"),
            Vec::new(),
            SandboxPath::new("work").expect("working directory"),
        ),
        ResourceRequest::new(
            ExecutionTimeoutMillis::new(30_000).expect("timeout"),
            PlacementRequest::new(
                ExecutionPlatformRequirement::default(),
                vec![WorkerPoolName::new("gpu").expect("GPU pool")],
                vec![
                    CapabilityRequirement {
                        name: CapabilityName::new("accelerator.device").expect("device key"),
                        value: CapabilityValue::new("0").expect("device value"),
                    },
                    CapabilityRequirement {
                        name: CapabilityName::new("accelerator.vendor").expect("vendor key"),
                        value: CapabilityValue::new("nvidia").expect("vendor value"),
                    },
                ],
            )
            .expect("placement"),
        )
        .expect("resources"),
        NetworkPolicy::Disabled,
        CapturePolicy::new(
            OutputByteLimit::new(4_096).expect("stdout limit"),
            OutputByteLimit::new(4_096).expect("stderr limit"),
            DiagnosticByteLimit::new(4_096).expect("diagnostic limit"),
            EvidenceByteLimit::new(16_384).expect("evidence limit"),
            Vec::new(),
        )
        .expect("capture"),
    )
}

fn await_gpu_probe(
    config: &ServerConfig,
    content: &SqliteContentStore,
    job_id: JobId,
    ids: ControllerScheduleIds,
) {
    let job = ExecutionJob::new(job_id).expect("job stream");
    let events = SqliteEventStore::open(&config.storage.event_database).expect("event store");
    for _ in 0..600 {
        match recover_execution_job(&events, content, &job).expect("recover probe") {
            ExecutionJobState::Completed {
                receipt_id,
                receipt,
            } => {
                assert_eq!(receipt.outcome(), ExecutionOutcome::Succeeded);
                let stdout = read_content::<ExecutionStdoutArtifact>(content, &receipt.stdout_id());
                assert_eq!(
                    String::from_utf8(stdout).expect("UTF-8 stdout").trim(),
                    "NVIDIA GB10"
                );
                let evidence_bytes = read_content(content, &receipt.evidence_id());
                let evidence: TrustedExecutionEvidence =
                    cairn_codec::from_slice(&evidence_bytes).expect("trusted evidence");
                assert!(
                    evidence.observations().iter().any(|observation| {
                        observation.as_str() == "docker:accelerator:nvidia:0"
                    })
                );
                release_execution_reservation(config, ids.reservation_id, &CommandId::new())
                    .expect("release terminal reservation");
                eprintln!(
                    "real GPU execution completed: job={job_id} attempt={} receipt={receipt_id}",
                    receipt.attempt_id()
                );
                return;
            }
            ExecutionJobState::NotStarted { diagnostic, .. }
            | ExecutionJobState::Ambiguous { diagnostic, .. } => {
                panic!("real GPU execution did not complete: {diagnostic}")
            }
            ExecutionJobState::NotFound
            | ExecutionJobState::ReadyToStart(_)
            | ExecutionJobState::InDoubt { .. } => thread::sleep(Duration::from_millis(100)),
        }
    }
    panic!("real GPU execution did not become terminal within 60 seconds");
}

fn resolve_storage(config: &mut ServerConfig, config_path: &Path) {
    let base = config_path.parent().unwrap_or_else(|| Path::new("."));
    for path in [
        &mut config.storage.event_database,
        &mut config.storage.content_database,
        &mut config.storage.content_directory,
    ] {
        if path.is_relative() {
            *path = base.join(&*path);
        }
    }
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
    id: &cairn_protocol::ContentId<T>,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    content.write_to(id, &mut bytes).expect("read content");
    bytes
}
