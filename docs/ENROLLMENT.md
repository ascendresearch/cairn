# Worker enrollment and managed identity

Cairn workers generate and retain their own private keys. Operators transfer one short-lived JSON
bundle, not a worker key/certificate set. The bootstrap listener is separate from the control
listener: bootstrap uses pinned server-authenticated TLS, while normal control remains strict mTLS.

## Controller configuration

Configure `enrollment_service` in `controller.json`. Its `issuer_certificate` must be the same CA
trusted by the control listener's `tls.client_ca`, and `issuer_private_key` must match that
certificate. Bootstrap handshake, diagnostic, wire-message, token TTL, and issued-credential
validity values are explicit configuration or command inputs.

Controller configuration schema V3 requires `enrollment: []`. Worker authentication and scheduling
consume only the append-only registry; a controller may start with an empty registry so onboarding
does not require any copied worker certificate. Schema V2 is accepted only by the explicit legacy
static-import command described below. Existing pre-V4 worker-registration facts require the
controlled development-state migration/rebuild gate described in the implementation plan.

The reference adapter currently reads an issuer certificate/key from files. The issuance boundary
is deliberately separate from the registry so an enterprise CA, offline signer, or SPIFFE adapter
can replace it without changing `EnrollmentId`, `WorkerId`, pool, or credential history semantics.

## Import a legacy static deployment

Keep one resolved copy of the old schema V2 configuration long enough to perform the migration.
Stop the old controller, allocate and retain one strong command identity, then run:

```bash
cairn-server registry import-static controller.v2.json command:019c0000-0000-7000-8000-000000000010
```

The command reads every configured leaf certificate, canonicalizes the batch by `CredentialId`,
and atomically records the exact certificate fingerprint, `WorkerId`, `CredentialId`, and pool. It
does not persist source paths or certificate bytes. Repeating the same command identity with the
same resolved certificates returns the original import event; reusing it with changed input fails.
A new command cannot import an already owned credential, fingerprint, or worker.

After success, change the operational file to `schema_version: 3` and replace the static array with
`"enrollment": []`. The V3 server refuses a non-empty list, while ordinary startup refuses V2, so
there is no interval in which static configuration and registry history silently compete. Keep the
legacy file according to the operator's audit/backup policy; Cairn no longer reads it during normal
operation.

## Create a one-shot bundle

```bash
cairn-server enrollment create controller.json default 600000 worker.enrollment.json
```

This records an expiring offer before returning the bundle. The output path must not already exist;
on Unix it is created with mode `0600`. The controller event stream stores only the secret's
SHA-256 digest. Creating another bundle is the recovery path if the operator loses this file before
the worker submits a CSR.

## Enroll a worker

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
- `ca.pem` — pinned controller trust anchor from the bundle;
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

Worker configuration schema V3 adds mandatory positive `identity_poll_interval_ms`. A running
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
- Externally provisioned identities enter the same registry through the explicit static-import
  migration boundary; no separate runtime authority remains.

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
live authority. See [`IMPLEMENTATION_PLAN.md`](IMPLEMENTATION_PLAN.md).
