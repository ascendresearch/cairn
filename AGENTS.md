# Cairn agent rules

## Development-stage versioning and compatibility

Until the user explicitly declares that Cairn has completed its first end-to-end workflow and is
establishing a public compatibility baseline, the repository is pre-release development state.
During this period:

- Do not add backward- or forward-compatibility logic, legacy aliases, fallback readers, dual
  readers/writers, import gates, data converters, or schema/protocol migration paths.
- Do not increment Cairn configuration, persisted-event, content-domain, wire-protocol, snapshot,
  artifact, or other internal format versions. The single current definition remains version 1.
- When a format or model changes, modify the V1 definition directly and update its code, tests,
  fixtures, examples, and documentation together. Development data and services may be discarded
  and rebuilt; no runtime conversion is required.
- Remove superseded development code and tests instead of retaining them solely to prove old-format
  compatibility. Tests should validate the current V1 contract and reject non-V1 input, not decode
  or transform it.
- If a requested change appears to require compatibility before that baseline exists, stop and
  surface the conflict instead of silently introducing versioning or migration machinery.

Product concepts whose names contain levels or vendor versions—such as migration validation tiers
V0–V3, model names, Rust/dependency versions, and Cargo package SemVer—are not Cairn internal format
versions and are outside this rule.
