# tpt-e-node Implementation Checklist

Companion to `spec.txt`. Reconciled against the existing `tpt-*` ecosystem (see notes inline)
so this doesn't duplicate or conflict with work already in progress in sibling repos.

## Phase 0 — Prerequisite (blocking gate)

- [x] `tpt-e-link` core types implemented and buildable: `LinkMessage`, `DeviceManifest` (wire
      format), `Capability` enum, `LinkTransport` trait, framing/CRC layer.
      Tracked in `tpt-e-link/TODO.md`, **not duplicated here** — this is a hard blocker for
      everything below, since `tpt-node-core` cannot compile without it.
- [x] Decide dependency strategy for `tpt-e-link` in `tpt-e-node`'s `Cargo.toml`: path
      dependency during co-development, switch to git/crates.io once `tpt-e-link` stabilizes.

## Phase 1 — Workspace scaffold & core reactor (`tpt-node-core`)

- [x] Create Cargo workspace root (`Cargo.toml`) with members `crates/tpt-node-core`,
      `crates/tpt-node-esp32`, `crates/tpt-node-riscv`.
- [x] Exclude `templates/*` from the workspace (they're `cargo-generate` templates, not build
      members).
- [x] Add `tpt-e-link` as a path dependency of `tpt-node-core`.
- [x] Build the Reactor / event-loop skeleton in `tpt-node-core` using `embassy-executor`
      (lightweight `no_std` async executor), listening on `LinkTransport` for incoming `Command`s.
      > Note: the core reactor is a plain `async fn` (no `embassy-executor` dependency in
      > `tpt-node-core`). `embassy-executor` is listed as a workspace dependency for the HAL
      > crates (Phases 4–5) to spawn the reactor as a task.
- [x] Implement the Capability Registry API:
      `node.register_capability(Capability::LittleFS(my_fs_driver))`, etc.
- [x] Implement Command dispatch: route incoming `Command` payloads to registered capability
      handlers.
- [x] Define the local `Capability` enum aligned with `tpt-basestation`'s `DeviceCapability`
      vocabulary (`Serial`, `Ble`, `Ota`, `LittleFs`, `Flash`, `Debug`, `Probe`, `Mesh`) but
      carrying device-side driver instances, per spec.txt's example code.
- [x] Internal handler: if `Capability::OTA` is registered, `tpt-node-core` auto-handles chunked
      download, CRC verification, and flash writing.
- [x] Unit tests for reactor/registry against `tpt-e-link`'s mock transport (`std` feature).

## Phase 2 — Two-layer manifest integration

> Decision: `tpt-e-link` owns the `no_std` wire-format `DeviceManifest` (device-authoritative,
> minimal). `tpt-basestation`'s existing `hal::DeviceManifest` (different shape: `id`/`name`/
> `vid`/`pid` vs `device_id`/`architecture`) becomes a host-side projection built *from* the
> wire manifest, rather than being replaced.

- [x] `tpt-node-core` constructs the wire-format `DeviceManifest` populated with registered
      capabilities and broadcasts it on boot.
- [x] *(tpt-basestation repo)* Add a mapping function: wire `DeviceManifest` →
      `hal::DeviceManifest` projection.
- [x] *(tpt-basestation repo)* Extend the discovery pipeline (`hal/src/discovery/`) to accept a
      tpt-e-link handshake as a new discovery method alongside the existing serial regex probe.
- [x] Document the field mapping between `tpt-e-link::DeviceManifest` and
      `tpt-basestation::hal::DeviceManifest` in a shared doc (both repos should link to it).
      (`docs/manifest-mapping.md`, linked from `tpt-basestation/hal/src/discovery/mod.rs`.)

## Phase 3 — Mock-first dev experience (WebSocket, both sides)

- [x] `tpt-node-mock`: `std` Rust binary implementing `LinkTransport` over a local WebSocket
      client.
- [x] Register fake capabilities in `tpt-node-mock`, including a simulated LittleFS backed by
      the host's local hard drive.
- [x] *(tpt-basestation repo)* Add a WebSocket listener in `tpt-core` accepting tpt-e-link-framed
      messages from `tpt-node-mock` (new transport — today only serial/BLE/HTTP-OTA exist).
- [x] *(tpt-basestation repo)* Wire WebSocket-discovered devices into the existing
      `DeviceCardDeck` / capability-gated UI (`gui/src/cards/registry.tsx`).
- [x] End-to-end check: `tpt-node-mock` boots, handshakes over WebSocket, and BaseStation's UI
      shows the correct capability cards (e.g. Filesystem card unlocks for `LittleFs`).
      > Covered by `crates/tpt-node-mock/tests/e2e.rs`: spins up BaseStation's real WS listener
      > and discovery pipeline, drives a LittleFS write command end-to-end, and asserts capability
      > gating.

## Phase 4 — ESP32 HAL (`tpt-node-esp32`, built on `tpt-embedded-core`)

> Decision: reuse `tpt-embedded-core`'s formally-verified, `no_std` ESP32 primitives instead of
> wrapping `esp-hal` from scratch.

- [ ] Add `tpt-e-typestate-hal` (covers `esp32`/`esp32s3`/`esp32c3`/`esp32c6`, i.e. both Xtensa
      and RISC-V ESP32 variants) as a dependency of `tpt-node-esp32`.
- [ ] Evaluate and pull in relevant sibling crates as needed: `tpt-e-chronos` (ring buffer /
      DMA handoff), `tpt-e-cipher` (constant-time crypto), `tpt-e-slumber` (deep-sleep state
      machine).
- [ ] Implement `tpt-e-link::LinkTransport` for UART using `tpt-e-typestate-hal` primitives.
- [ ] Implement BLE transport for ESP32 (new work — not covered by `tpt-embedded-core`).
- [ ] Wire the internal OTA handler (from Phase 1) to real ESP32 partition tables / flash
      writes.
- [ ] Implement LittleFS flash bindings for ESP32.
- [ ] Milestone: a physical ESP32-C3 (RISC-V core) completes a full handshake with a mock
      BaseStation.

## Phase 5 — Generic RISC-V HAL (`tpt-node-riscv`)

- [ ] Implement using `riscv-rt` + `embedded-hal` traits, providing a UART transport for
      generic (non-ESP32) RISC-V chips such as GD32V or SiFive.
- [ ] Confirm no reuse is needed from `tpt-embedded-core` (that crate's HAL is ESP32-specific).

## Phase 6 — Templates & docs

- [ ] `cargo-generate` template: `templates/esp32-blinky`.
- [ ] `cargo-generate` template: `templates/riscv-sensor`.
- [ ] Write the "5-Minute Quickstart" documentation.
- [ ] Verify: a hobbyist can clone a template, flash it, and see the device instantly
      recognized and mapped in `tpt-basestation`.

## Key files / repos referenced

- `tpt-e-node/spec.txt` — source design doc (this repo)
- `tpt-e-link/spec.txt`, `tpt-e-link/TODO.md` — external blocking prerequisite (Phase 0)
- `crates/tpt-node-core/src/{reactor,capability,ota,error}.rs` — reactor, capability registry,
  OTA handler (Phase 1, done)
- `crates/tpt-node-mock/src/{lib,main,drivers}.rs`, `crates/tpt-node-mock/tests/e2e.rs` —
  WebSocket mock device + host-backed LittleFS, end-to-end tested against a real BaseStation
  WS listener (Phase 3, done)
- `tpt-basestation/hal/src/manifest.rs`, `manifest_registry.rs`, `capability.rs`,
  `discovery/` — wire→hal manifest projection and handshake discovery (Phase 2, done)
- `tpt-basestation/core/src/ws.rs` — WebSocket listener accepting tpt-e-link-framed messages
  (Phase 3, done)
- `tpt-basestation/gui/src/cards/registry.tsx`, `gui/src/hooks/useCapabilityGate.ts` —
  capability-gated UI wired to WebSocket-discovered devices (Phase 3, done)
- `tpt-embedded-core/crates/tpt-e-typestate-hal` (+ `tpt-e-chronos`, `tpt-e-cipher`,
  `tpt-e-slumber`) — HAL/primitives to depend on for `tpt-node-esp32` (Phase 4, not started)
