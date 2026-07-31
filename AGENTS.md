# AGENTS.md — tpt-e-node

## What this is

Device-side firmware SDK for the TPT ecosystem. Rust workspace with HAL crates for ESP32 and generic RISC-V, a shared `no_std` core runtime, and `cargo-generate` templates. Depends on the sibling [`tpt-e-link`](../tpt-e-link) crate for the wire protocol.

## Workspace layout

```
tpt-e-node/
├── Cargo.toml              # workspace root (resolver 2, MSRV 1.75)
├── crates/
│   ├── tpt-node-core/      # no_std + alloc: reactor, capability registry, OTA handler
│   ├── tpt-node-esp32/     # ESP32 HAL (skeleton — Phase 4)
│   └── tpt-node-riscv/     # generic RISC-V HAL (skeleton — Phase 5)
├── templates/              # cargo-generate templates (EXCLUDED from workspace)
│   ├── esp32-blinky/
│   └── riscv-sensor/
├── spec.txt                # original design doc
└── todo.md                 # authoritative implementation checklist
```

## Phase status

- **Phase 0** — complete. `tpt-e-link` compiles and is referenced via path dependency.
- **Phase 1** — complete. `tpt-node-core` has: capability registry, command dispatch, internal OTA handler, async reactor, and 18 unit/integration tests.
- **Phase 2–6** — not started.

## Key conventions

- **`tpt-node-core` is `#![no_std]` + `alloc`** — uses `heapless` for fixed-capacity collections, `alloc::boxed::Box` for trait objects.
- **`std` feature** on `tpt-node-core` enables `tpt-e-link/std` (mock transport) for host-side testing. **Tests require `--features std`**.
- **Tests use `pollster::block_on`** to drive async futures (same pattern as `tpt-e-link`'s own tests).
- **`embassy-executor`** is the target runtime for embedded; the core reactor is a plain `async fn` that works with any executor. HAL crates configure and spawn it.
- **`#![forbid(unsafe_code)]`** is set in `tpt-node-core`; keep it.
- **Trait objects use `+ Send`** bounds (`Box<dyn CommandHandler + Send>`, `Box<dyn FlashWriter + Send>`) so tests can run the reactor in a separate thread via `std::thread::scope`.
- **`device_id` is `[u8; 8]`** — exactly 8 bytes. `*b"mydevice"` works; `*b"my-device"` (9 bytes) does not.
- **`usize` vs `u32`**: when constructing `CommandPayload` args for OTA commands, cast `.len()` to `u32` before `.to_le_bytes()` — `usize` is 8 bytes on 64-bit hosts but the protocol expects 4.

## Capability model

- Local `Capability` enum mirrors `tpt-basestation`'s `DeviceCapability` (`Serial`, `Ble`, `Ota`, `LittleFs`, `Flash`, `Debug`, `Probe`, `Mesh`) plus `Telemetry`.
- Driver-bearing variants (`LittleFs`, `Telemetry`) carry `Box<dyn Driver + Send>`.
- `Capability::to_wire()` maps to `tpt-e-link`'s wire `Capability` — some variants (Flash, Debug, Mesh, Probe, Telemetry) have no wire representation.
- `Capability::Ota` is **internal** — registering it auto-creates an `OtaHandler` with a `RamFlashWriter`. The user does not supply a driver.
- Command IDs are namespaced by capability (see `cmd` module): `LITTLEFS=0x0100`, `OTA=0x0200`, `TELEMETRY=0x0300`, etc.

## OTA protocol

Three commands in the `0x02xx` range:

| Command   | ID      | Args (LE)                              |
|-----------|---------|----------------------------------------|
| `OtaStart`| `0x0200`| `expected_size: u32` + `expected_crc: u32` (8 B) |
| `OtaChunk`| `0x0201`| `offset: u32` + `data: [u8; ≤28]`      |
| `OtaFinish`| `0x0202`| *(empty)*                              |

Max chunk data is 28 bytes (4-byte offset + 32-byte `MAX_COMMAND_ARGS_LEN`). CRC uses `CRC_32_ISO_HDLC` (same as `tpt-e-link`'s frame layer). `FlashWriter` trait abstracts flash; `RamFlashWriter` is the in-RAM default.

## Commands

```sh
# Build everything
cargo build --workspace

# Test tpt-node-core (requires --features std for mock transport)
cargo test -p tpt-node-core --features std

# Check no_std compiles (requires embedded target)
cargo build -p tpt-node-core --target thumbv7em-none-eabihf

# Format + lint
cargo fmt --all
cargo clippy --workspace -- -D warnings
```

## Cross-repo relationships

- **`tpt-e-link`** (sibling, `../tpt-e-link`): wire protocol, `LinkMessage`, `DeviceManifest`, `LinkTransport` trait, `FrameDecoder`. Complete — do not re-implement.
- **`tpt-basestation`** (sibling, `../tpt-basestation`): host-side `hal::DeviceManifest` (TOML-based, different field names: `id`/`name`/`vid`/`pid` vs `tpt-e-link`'s `device_id`/`architecture`), `hal::DeviceCapability` enum, `hal::discovery` pipeline. Phase 2 integration not started.
- **`tpt-embedded-core`** (sibling, `../tpt-embedded-core`): ESP32 HAL primitives. Not yet implemented — Phases 4–5 depend on it.
