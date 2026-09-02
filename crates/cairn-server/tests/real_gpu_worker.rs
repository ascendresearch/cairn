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

const CUDA_REDUCTION_INPUT_ID: &str = concat!(
    "cairn:v1:sha256:execution.input-bundle.v1:",
    "76cb507be1faebb66e72726826f541d0b3c7ea8cbd2e5b657731590f16a90f86"
);

#[test]
#[ignore = "requires CAIRN_REAL_CONTROLLER_CONFIG, CAIRN_REAL_GPU_IMAGE_ID, a live controller, and the enrolled GB10 worker"]
fn scheduled_gpu_probe_returns_device_bound_evidence() {
    let config_path = env::var("CAIRN_REAL_CONTROLLER_CONFIG").expect("controller config path");
    let image = env::var("CAIRN_REAL_GPU_IMAGE_ID").expect("full GPU image ID");
    let config = load_config(Path::new(&config_path));
    let mut content =
        SqliteContentStore::open(config.content_database(), config.content_directory())
            .expect("controller content store");
    let (input_id, environment_id) = archive_probe_materials(&mut content, image);
    let job_id = JobId::new();
    let contract = gpu_contract(job_id, input_id, environment_id, "bin/probe", 30_000);
    let ids = schedule_ids();
    let scheduled = schedule_execution_contract(&config, &contract, ids).expect("schedule probe");
    assert!(matches!(
        scheduled,
        ControllerSchedulingOutcome::Scheduled { .. }
    ));

    await_gpu_success(&config, &content, job_id, ids, "NVIDIA GB10", "GPU probe");
}

#[test]
#[ignore = "requires the live GB10 worker and CAIRN_REAL_CUDA_FIXTURE_ROOT"]
fn scheduled_cuda_reduction_builds_and_passes_release_corpus() {
    let config_path = env::var("CAIRN_REAL_CONTROLLER_CONFIG").expect("controller config path");
    let fixture_root = env::var("CAIRN_REAL_CUDA_FIXTURE_ROOT").expect("CUDA fixture root");
    let image = env::var("CAIRN_REAL_GPU_IMAGE_ID").expect("full GPU image ID");
    let config = load_config(Path::new(&config_path));
    let mut content =
        SqliteContentStore::open(config.content_database(), config.content_directory())
            .expect("controller content store");
    let (input_id, environment_id) =
        archive_cuda_reduction_materials(&mut content, Path::new(&fixture_root), image);
    assert_eq!(input_id.to_string(), CUDA_REDUCTION_INPUT_ID);
    let job_id = JobId::new();
    let contract = gpu_contract(job_id, input_id, environment_id, "bin/run", 120_000);
    let ids = schedule_ids();
    let scheduled = schedule_execution_contract(&config, &contract, ids).expect("schedule CUDA");
    assert!(matches!(
        scheduled,
        ControllerSchedulingOutcome::Scheduled { .. }
    ));

    await_gpu_success(
        &config,
        &content,
        job_id,
        ids,
        "PASS fixture=cuda-reduction-v1 cases=9 input_checksum=be6c603ff51fbd74",
        "CUDA reduction",
    );
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
    let environment_id = archive_environment(content, image);
    (input_id, environment_id)
}

fn archive_cuda_reduction_materials(
    content: &mut SqliteContentStore,
    root: &Path,
    image: String,
) -> (
    ContentId<InputBundleArtifact>,
    ContentId<ExecutionEnvironmentArtifact>,
) {
    let mut entries = vec![
        directory("bin"),
        directory("source"),
        directory("source/include"),
        directory("source/src"),
        directory("source/tests"),
        InputBundleEntry::File {
            path: SandboxPath::new("bin/run").expect("runner path"),
            mode: InputFileMode::Executable,
            bytes: b"#!/bin/sh\nset -eu\ncp -R /cairn/input/source/. /cairn/work/source\nsed -i 's/CUDA_ARCHITECTURES native/CUDA_ARCHITECTURES 121/' /cairn/work/source/CMakeLists.txt\ncmake -S /cairn/work/source -B /cairn/work/build -DCMAKE_BUILD_TYPE=Release 1>&2\ncmake --build /cairn/work/build --target reduction_reference --parallel 1>&2\nexec /cairn/work/build/reduction_reference --case-set release\n".to_vec(),
        },
    ];
    for path in [
        "CMakeLists.txt",
        "include/reduce_sum.h",
        "src/reduce_sum_kernel.cu",
        "src/reduce_sum_launch.cu",
        "tests/reference_main.cpp",
    ] {
        entries.push(InputBundleEntry::File {
            path: SandboxPath::new(format!("source/{path}")).expect("fixture bundle path"),
            mode: InputFileMode::Data,
            bytes: fs::read(root.join(path)).expect("read frozen CUDA fixture file"),
        });
    }
    let bundle = InputBundleV1::new(entries).expect("CUDA input bundle");
    let input_id = content
        .put::<InputBundleArtifact>(&mut Cursor::new(bundle.to_bytes().expect("input bytes")))
        .expect("archive CUDA input")
        .content_id;
    (input_id, archive_environment(content, image))
}

fn directory(path: &str) -> InputBundleEntry {
    InputBundleEntry::Directory {
        path: SandboxPath::new(path).expect("bundle directory"),
    }
}

fn archive_environment(
    content: &mut SqliteContentStore,
    image: String,
) -> ContentId<ExecutionEnvironmentArtifact> {
    let environment = DockerExecutionEnvironmentV1::new(
        DockerImageId::new(image).expect("full image ID"),
        Vec::new(),
    )
    .expect("Docker environment");
    content
        .put::<ExecutionEnvironmentArtifact>(&mut Cursor::new(
            environment.to_bytes().expect("environment bytes"),
        ))
        .expect("archive environment")
        .content_id
}

fn gpu_contract(
    job_id: JobId,
    input_id: ContentId<InputBundleArtifact>,
    environment_id: ContentId<ExecutionEnvironmentArtifact>,
    program: &str,
    timeout_ms: u64,
) -> JobContract {
    JobContract::new(
        job_id,
        input_id,
        environment_id,
        ExecutionBackend::new("docker-v1").expect("Docker backend"),
        CommandContract::new(
            SandboxPath::new(program).expect("program"),
            Vec::new(),
            SandboxPath::new("work").expect("working directory"),
        ),
        ResourceRequest::new(
            ExecutionTimeoutMillis::new(timeout_ms).expect("timeout"),
            PlacementRequest::new(
                ExecutionPlatformRequirement::default(),
                vec![WorkerPoolName::new("gpu").expect("GPU pool")],
                vec![
                    CapabilityRequirement {
                        name: CapabilityName::new("accelerator.architecture")
                            .expect("architecture key"),
                        value: CapabilityValue::new("sm_121").expect("architecture value"),
                    },
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
            OutputByteLimit::new(1_048_576).expect("stdout limit"),
            OutputByteLimit::new(1_048_576).expect("stderr limit"),
            DiagnosticByteLimit::new(16_384).expect("diagnostic limit"),
            EvidenceByteLimit::new(16_384).expect("evidence limit"),
            Vec::new(),
        )
        .expect("capture"),
    )
}

fn await_gpu_success(
    config: &ServerConfig,
    content: &SqliteContentStore,
    job_id: JobId,
    ids: ControllerScheduleIds,
    expected_stdout: &str,
    label: &str,
) {
    let job = ExecutionJob::new(job_id).expect("job stream");
    let events = SqliteEventStore::open(config.event_database()).expect("event store");
    for _ in 0..600 {
        match recover_execution_job(&events, content, &job).expect("recover probe") {
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
                    "{label} stderr:\n{}",
                    String::from_utf8_lossy(&stderr)
                );
                assert_eq!(
                    String::from_utf8(stdout).expect("UTF-8 stdout").trim(),
                    expected_stdout
                );
                let evidence_bytes = read_content(content, &receipt.evidence_id());
                let evidence: TrustedExecutionEvidence =
                    cairn_codec::from_slice(&evidence_bytes).expect("trusted evidence");
                assert!(
                    evidence.observations().iter().any(|observation| {
                        observation.as_str() == "docker:accelerator:nvidia:0"
                    })
                );
                eprintln!(
                    "real {label} completed: job={job_id} attempt={} receipt={receipt_id}",
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
        &mut config.event_database(),
        &mut config.content_database(),
        &mut config.content_directory(),
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
