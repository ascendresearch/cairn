# Cairn current-V1 sanitized regression fixtures

These files are newly authored MIT-licensed public controls. They retain named historical behavior
without copying historical payloads, private deployment material, provider transcripts, or
provenance-uncleared third-party source.

`manifest.json` binds every fixture to its exact bytes, current author role, full historical commit
and public source path, obligation, replacement scope, data classification, and future consumer.
`sanitation-scan-profile.json` freezes the required public-tree scan classes. The `cairn-testkit`
contract strictly decodes all JSON, recomputes identities, validates cited paths, and runs seeded
negative controls.

No ST1 identity-graph plan or fixture is retained without a current producing and replaying
consumer. A future workflow authors its exact current-V1 fixture with that consumer. Recorded
controls do not claim live-model, CUDA, Ascend, device, or performance evidence.
