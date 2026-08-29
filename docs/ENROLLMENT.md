# Worker enrollment and managed identity

Cairn workers generate and retain their own private keys. Operators transfer one short-lived JSON
bundle, not a worker key/certificate set. The bootstrap listener is separate from the control
listener: bootstrap uses pinned server-authenticated TLS, while normal control remains strict mTLS.

## Controller configuration

Configure `enrollment_service` in `controller.json`. Its `issuer_certificate` must be the same CA
trusted by the control listener's `tls.client_ca`, and `issuer_private_key` must match that
certificate. Bootstrap handshake, diagnostic, wire-message, token TTL, and issued-credential
validity values are explicit configuration or command inputs.

For the normal open-source join path, also configure
`enrollment_service.control_endpoint` with the externally routable control TCP address, WebSocket
URI, TLS server name, and server CA path. Enrollment and ordinary control may use different
listeners, names, and server certificates: set `enrollment_service.server_tls` to a dedicated
bootstrap certificate/key, while `server_ca` pins its issuing CA. Both the bootstrap identity and
ordinary-control endpoint are required. The controller embeds both public endpoint descriptions
and pinned trust material in a schema V1 bundle; no
control address is hand-entered on the worker.

In the single-lab profile, bind the ordinary control listener to `0.0.0.0:7443` and the enrollment
listener to `0.0.0.0:7444` (or deployment-selected ports). Set `public_tcp_address`,
`control_endpoint.tcp_address`, and both WebSocket URIs to the Controller DNS/IP reachable through
the operator's existing private network/VPN. Never publish `0.0.0.0`, loopback, an SSH-tunnel
endpoint, or a temporary port-forward address. The Worker initiates both enrollment and ordinary
control connections directly; Cairn does not reverse-connect to the Worker or create another VPN.

Controller configuration schema V1 has no static certificate list. Worker authentication and
scheduling consume only the append-only registry; a controller may start with an empty registry so
onboarding does not require any copied worker certificate.

The reference adapter currently reads an issuer certificate/key from files. The issuance boundary
is deliberately separate from the registry so an enterprise CA, offline signer, or SPIFFE adapter
can replace it without changing `EnrollmentId`, `WorkerId`, pool, or credential history semantics.

## Create a one-shot bundle

```bash
cairn-server enrollment create controller.json default 600000 worker.enrollment.json
```

This records an expiring offer before returning the bundle. The output path must not already exist;
on Unix it is created with mode `0600`. The controller event stream stores only the secret's
SHA-256 digest. Creating another bundle is the recovery path if the operator loses this file before
the worker submits a CSR.

## Enroll a worker

The preferred new-machine path is one command:

```bash
cairn-worker join worker.enrollment.json /var/lib/cairn/worker
```

It creates a fixed tree containing `identity/`, `scratch/`, `content/`, `transfers/`,
`worker.sqlite3`, and `content.sqlite3` when first run, plus a strict schema V1 `worker.json`.
Platform and quantitative
host resources are observed locally; the running
executable is identified by its SHA-256 digest. Timeouts, heartbeat, reconnect, resource freshness,
expectations, availability, message-size limits, assignment-material limits, and positive chunk
size remain explicit editable configuration fields. The two limits are independent and either may
be `null`; chunk size must fit the enabled encoded-message limit.
The initial worker is unavailable and draining until a real execution backend is configured and
activated. Repeating join with the same bundle validates and reuses an existing tree; it never
replaces a differing file or discards operator changes. Start it with:

```bash
cairn-worker /var/lib/cairn/worker/worker.json
```

The lower-level identity-only flow remains available for automation and diagnosis.

Transfer the single bundle through an operator-approved channel, then run on the worker:

```bash
cairn-worker enroll worker.enrollment.json /var/lib/cairn/identity
```

The worker creates the state directory with mode `0700` and stages `worker-key.pem` plus the exact
CSR before contacting the controller. The private key is `0600` and never crosses the network. On
success the directory contains:

- `worker-key.pem` — worker-local private key;
- `enrollment.csr.pem` — exact retry/recovery CSR;
- `worker.pem` — issued leaf and chain;
- `ca.pem` — pinned ordinary-control trust anchor from the bundle;
- `identity.json` — one-shot `EnrollmentId`, stable `WorkerId`, rotatable `CredentialId`,
  authenticated pool, and relative TLS paths.

No existing file with different bytes is overwritten. If the controller committed issuance but
the response was lost, rerunning the same command with the same state directory returns the exact
original credential. The same token with another CSR/key is rejected.

## Start the managed worker

Worker configuration selects the state directory rather than repeating identity files:

```json
{
  "identity": {
    "mode": "managed",
    "state_directory": "/var/lib/cairn/identity"
  }
}
```

After bootstrap, delete the transferred bundle according to local secret-handling policy. Do not
delete the staged CSR: it is non-secret recovery evidence bound to the local private key.

Worker configuration schema V1 has mandatory positive `identity_poll_interval_ms`. A running
managed worker checks the atomic identity manifest at this interval. When rotation changes the
credential, it closes the old connection and reconnects with a fresh `WorkerIncarnationId`.

## Rotate a managed credential

Set `enrollment_service.rotation_overlap_ms` to a positive duration, or `null` to disable automatic
predecessor retirement. The controller freezes this choice at successor issuance. Create a bundle
for the exact current credential:

```bash
cairn-server credential rotate controller.json credential:... 600000 worker.rotation.json
```

Transfer it to the same worker state directory and rotate:

```bash
cairn-worker rotate worker.rotation.json /var/lib/cairn/identity
```

The command verifies that the bundle names the current `WorkerId` and `CredentialId`, then stages a
fresh key and exact CSR under `rotations/<EnrollmentId UUID>/`. The directory also retains the
predecessor manifest, issued certificate, and pinned CA. No old key or certificate is overwritten.
After validating lineage and public-key binding, the worker atomically replaces only
`identity.json`. Retrying after response or local-commit loss reuses the staged CSR and recovers the
same successor.

The running worker observes the manifest switch and reconnects with a new incarnation during the
overlap. After confirming the successor session, the rotation bundle may be deleted. If overlap is
`null`, the operator must explicitly revoke the predecessor after observing successor admission.

To roll back a bad successor before the frozen deadline, first revoke that successor at the
controller, then restore the local predecessor:

```bash
cairn-server credential revoke controller.json credential:<successor>
cairn-worker rollback /var/lib/cairn/identity
```

Revoking the successor inside the overlap cancels the predecessor's pending retirement as part of
the durable projection. The local command validates worker, pool, credential lineage, material, and
deadline before atomically restoring the predecessor manifest. Reversing this order can leave only
a temporary local fallback, so the controller action is deliberately first. Rollback after the
deadline fails closed. Destroy the revoked successor's rotation bundle after rollback; exact-CSR
replay deliberately returns the original (now revoked) issuance and is not a second rotation.

## Authority and recovery properties

- The controller, not the worker, assigns `WorkerId` and pool.
- An `EnrollmentId` is single-use but exact-CSR retries remain idempotent even after expiry.
- A fresh controller process reconstructs certificate fingerprint to `WorkerId`/pool authorization
  from the append-only registry stream.
- The issued certificate serial is the strong `CredentialId`; it is not the permanent worker ID.
- Managed enrollment is the only path into registry authority; no separate static authority or
  import boundary exists.

## Revoke authority

The current managed registry supports three deliberately separate emergency actions:

```bash
cairn-server enrollment revoke controller.json enrollment:... command:...
cairn-server credential revoke controller.json credential:... command:...
cairn-server worker disable controller.json worker:... command:...
```

Enrollment revocation invalidates only an unused bootstrap authority. Credential revocation
invalidates one issued credential. Worker disablement invalidates every managed credential for the
logical worker. These commands append facts; they do not edit or delete issuance history. A running
controller observes the shared SQLite authority safely and rejects reconnect before registration;
an observed live session is closed no later than the configured positive
`authority_poll_interval_ms` (or an earlier control message/outbox poll).

Application authorization is authoritative even when TLS accepts a certificate chain, so the
baseline does not require CRL or OCSP. Imported static credentials support the same lifecycle facts
as controller-issued credentials. Every command identity is retained for exact retry; reusing it
for another target or operation fails closed.

## Re-enable and change pool

Pool assignment is controller authority, not worker-reported profile data. Change it only through
the explicit disabled state:

```bash
cairn-server worker disable controller.json worker:... command:...
cairn-server worker set-pool controller.json worker:... post-bootstrap-pool command:...
cairn-server worker enable controller.json worker:... command:...
```

The new pool must differ from the current pool. A running controller closes the disabled worker's
session. On re-enable, the next handshake reloads the current registry and cross-links the exact
registry pool-assignment event into the execution-worker history before registration. The
execution projection rejects that cross-link while the predecessor session is live, so an ordinary
reconnect can never smuggle in an implicit pool change. If a controller died before recording the
disconnect, reconnect waits for the configured `session_timeout_ms` boundary rather than replacing
live authority. The implemented enrollment/scheduler foundation is summarized in
[`dev/CURRENT_BASELINE.md`](dev/CURRENT_BASELINE.md); detailed historical delivery evidence is in Git.

## Inspect and audit registry authority

Read-only commands emit versioned strict JSON on stdout and diagnostics on stderr:

```bash
cairn-server registry list controller.json
cairn-server registry show-worker controller.json worker:...
cairn-server registry show-credential controller.json credential:...
cairn-server registry audit controller.json
```

`list` is a canonical stable-ID-ordered snapshot. Worker entries include current pool, its exact
authority revision, disabled state, and every retained credential identity. Credential entries
include fingerprint evidence, issuance/static-import provenance, predecessor/successor lineage,
retirement boundary, and effective `active`, `worker-disabled`, `retired`, or `revoked` state at the
report observation time. `audit` validates the entire parent chain and every lifecycle invariant,
then reports head identity and counts; contradictory history returns no report. Neither output
contains bearer secrets, private keys, certificate bodies, or source paths.
