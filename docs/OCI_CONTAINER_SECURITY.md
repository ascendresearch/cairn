# OCI container security boundary

- Status: F2d-b typed contract and fixed launch plan implemented; runtime mutation not implemented
- Backend claim: `oci-container-v1`
- Scope: untrusted CPU-only candidate and oracle processes

This document freezes the security boundary before Cairn invokes a Docker-compatible runtime. The
current code validates identities, lifecycle observations, and OCI environment bytes and renders a
canonical create argv without a shell. It does not invoke a runtime or activate a container
executor and must not yet be treated as an operational isolation claim.

## Threat model

The subject may control every byte in its input bundle, executable, arguments, environment values,
stdout, stderr, declared outputs, and the contents of its selected image. It may attempt filesystem
escape, symlink and special-file substitution, fork or memory exhaustion, network access, runtime
API access, credential discovery, output forgery, container-name collision, and execution replay.

The trusted computing base includes the Cairn worker/supervisor, its immutable configuration, the
configured container runtime and daemon, the host kernel, canonical identity/schema code, worker
CAS/journal, and evidence ingestion code. `oci-container-v1` does not defend against a compromised
kernel, runtime, daemon, worker binary, or operator account.

The subject must not be able to observe or modify:

- worker TLS private keys, certificates, enrollment/rotation state, or configuration;
- worker/controller SQLite databases, content stores, transfer staging, or trusted evidence;
- the runtime socket or runtime state directory;
- unrelated host files, processes, IPC objects, devices, or network interfaces;
- another attempt's input, writable workspace, outputs, temporary files, or container lifecycle.

F2d admits no accelerator, GPU, NPU, generic device, privileged, host-network, host-PID, or arbitrary
mount path. Device-aware containers are a separate F2e design and cannot be enabled by weakening
this policy.

## Frozen typed contract

`cairn-execution` now owns provider-neutral types for:

- `OciImageDigest`: exactly `sha256:<64 lowercase hex>`; mutable tags are unrepresentable;
- `ContainerName`: derived only as `cairn-attempt-<AttemptId UUID>`;
- `RuntimeContainerId`: one full 64-character lowercase runtime identity, never a short prefix;
- `ContainerPhase`: `absent`, `created`, `running`, or `exited`;
- `ContainerMountRole`: input, work, output, or temporary;
- `ContainerSandboxPolicy::cpu-untrusted-v1`: a code-owned policy identity;
- `ContainerBinding`: exact attempt, job, contract, input, environment, and policy identities;
- `ContainerInspection`: a tagged state in which present phases cannot omit runtime identity or
  binding and the absent phase cannot invent them;
- `OciExecutionEnvironmentV1`: strict canonical JSON containing one image digest and a canonical
  environment-variable set;
- `ContainerRuntime`: the initial read-only resolution/inspection port returning typed observations
  rather than Docker/Podman output.

The OCI environment bytes occupy the existing typed `ExecutionEnvironmentArtifact` content domain.
The job's backend determines which strict decoder is entitled to interpret those bytes. There is no
fallback from OCI format to local-process format.

## Fixed launch policy

`cairn-worker` now derives one `ContainerLaunchPlan` from the exact canonical contract,
worker-verified material identities and bytes, a backend-owned absolute state root, and strong
positive resource ceilings. The plan is data only: it contains no runtime executable and grants no
create, start, stop, or delete authority. Its Docker-compatible renderer emits one argv vector and
never constructs a shell command.

The fixed argv requires a read-only image root, numeric non-root subject `65532:65532`, denied
network, all capabilities dropped, `no-new-privileges`, private cgroup/PID/IPC namespaces, bounded
PIDs/CPU/memory-and-swap, and no health check or restart policy. The only host bind is the exact
attempt input directory mounted read-only at `/cairn/input`. `/cairn/work`, `/cairn/output`, and
`/tmp` are separate size-bounded tmpfs mounts owned by the subject; output and temporary mounts are
also `noexec`. The working directory is exactly `/cairn/work`, declared outputs must remain below
`/cairn/output`, and the contract program replaces any image-defined entrypoint. The image is
addressed only by its immutable digest with pulling disabled.

The plan rejects non-OCI backends, enabled network, every accelerator/device request, an
insufficient configured CPU/memory/work/output ceiling, reserved input paths, a non-executable
program, identity mismatch, and unsafe state-root syntax. Worker credentials, journal, CAS,
SQLite, runtime socket, runtime state, unrelated host paths, arbitrary mounts, devices, and
privilege switches cannot be supplied as plan fields.

Operator configuration in later slices may select a trusted runtime executable and backend-owned
state root and may set documented numeric ceilings. It cannot add mounts, capabilities, devices,
namespace sharing, or network access to `cpu-untrusted-v1`. Numeric ceilings used for one accepted
job cannot be disabled and must satisfy the contract minima. Because a Docker CLI flag cannot force
a daemon into a private user namespace, activation must later preflight a rootless runtime or
daemon-level user-namespace remapping; the plan never emits `--userns=host`. F2d-c must also stage
the read-only input tree with permissions readable by the remapped non-root subject without making
worker-owned state visible, and must reject image/runtime inspection that would synthesize
additional mounts (including image-declared volumes) or otherwise weaken the fixed plan.

## Recovery boundary

One `AttemptId` has one deterministic `ContainerName`. Before Cairn reattaches to a present
container, every immutable `ContainerBinding` field must match. A name collision with different
labels is hostile/conflicting state: Cairn does not start, delete, rename, or reuse it.

The intended lifecycle is:

```text
Absent → Created → Running → Exited
```

An unavailable runtime before creation is `NotStarted`. Uncertainty after create or start is
`Ambiguous` until inspection of the exact name, full runtime ID, and binding proves a phase. A
worker restart may recover the same container; it must never create a second subject for an already
started `AttemptId`. Cleanup becomes eligible only after terminal evidence is durable and cleanup
failure cannot authorize re-execution.

## Acceptance boundary

F2d-b ordinary tests now cover the exact golden create argv, absence of privilege-downgrade flags,
the single read-only host mount, identity/layout controls, device/network rejection, and resource
ceiling failure. F2d is not complete until offline fake-runtime tests also cover phase recovery,
binding conflicts, ambiguous mutation, bounded capture, and cleanup ordering, and opt-in real-host
tests prove filesystem/network/device isolation on both release architectures. Until then,
`cairn-worker` must not advertise or activate `oci-container-v1`.
