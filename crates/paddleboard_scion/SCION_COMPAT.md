# Scion Compatibility

This file tracks which version of [googlecloudplatform/scion](https://github.com/GoogleCloudPlatform/scion)
the Rust types in this crate were derived from. Update it whenever you
sync types with a new Scion release.

## Current target

- **Scion version:** 0.1.x (initial release series, April–May 2026)
- **Source commit:** `main` as of 2026-05-26
- **Key Go source files:**
  - `pkg/api/types.go` — `AgentInfo`, phase/activity enums
  - `pkg/agent/state/state.go` — agent state model
  - `cmd/scion/` — CLI command definitions and JSON output shapes

## Type mapping notes

- `AgentPhase` and `AgentActivity` enums include a `#[serde(other)] Unknown`
  variant so that new values added upstream don't break deserialization.
- All optional/`omitempty` fields in Go use `#[serde(default)]` in Rust.
- Scion's Go structs use camelCase JSON tags; Rust types use
  `#[serde(rename_all = "camelCase")]` to match.
- `AgentInfo` intentionally flattens some nested Go structs (e.g. status
  fields) for ergonomic Rust access.

## Changelog

### 2026-05-26 — Initial types
- Created `types.rs` matching Scion 0.1.x JSON output.
- Covered: `ScionVersion`, `AgentPhase`, `AgentActivity`, `AgentDetail`,
  `AgentInfo`, `TemplateInfo`.
- `compat.rs` checks installed version against `TESTED_VERSION = "0.1"`.

## How to update

1. Install the new Scion version and capture sample JSON:
   ```bash
   scion version --format json > /tmp/scion-version.json
   scion list --format json > /tmp/scion-list.json
   scion templates list --format json > /tmp/scion-templates.json
   ```
2. Compare the JSON fields against `types.rs`. Add new fields, mark
   removed fields as `#[serde(default)]` if not already.
3. Update `TESTED_VERSION` in `compat.rs`.
4. Update this file's "Current target" section.
5. Add a changelog entry.
6. Run `cargo test -p paddleboard_scion` to verify parsing still works.
