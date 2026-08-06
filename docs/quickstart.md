# 5-Minute Quickstart

Get a TPT-ready device flashed and recognized by `tpt-basestation` in about
five minutes. This walks the hobbyist path; engineers can jump to
[Developing](#developing-on-the-host-no-hardware) to build real firmware.

## What you need

- A Rust toolchain (1.75+): <https://rustup.rs>
- One of:
  - An **ESP32** board (ESP32 / ESP32-S3 / ESP32-C3 / ESP32-C6) with a USB-UART
    or native USB port, **or**
  - A **RISC-V** board such as a **GD32VF103** (Longan Nano) or SiFive E310.
- [`cargo-generate`](https://github.com/cargo-generate/cargo-generate) and
  [`probe-rs`](https://github.com/probe-rs/probe-rs) (or `espflash` for ESP32).

```sh
cargo install cargo-generate probe-rs-tools
# ESP32 only:
cargo install espflash
```

## Step 1 — Generate a project

Pick the template that matches your chip family.

**ESP32:**

```sh
cargo generate --git https://github.com/tpt-solutions/tpt-e-node --path templates/esp32-blinky
```

**Generic RISC-V:**

```sh
cargo generate --git https://github.com/tpt-solutions/tpt-e-node --path templates/riscv-sensor
```

Answer the three prompts:

| Prompt | Example | Notes |
|--------|---------|-------|
| Project name | `my-esp32-device` | kebab-case |
| Chip | `esp32c3` / `gd32v` | exactly one |
| Device id | `esp32bl1` | **exactly 8 bytes** — broadcast at boot |

The generator writes a complete, buildable firmware into a new folder.

## Step 2 — Flash it

**ESP32 (C3 example, native USB):**

```sh
cd <your-project>
cargo build --release --features esp32c3 --target riscv32imc-unknown-none-elf
espflash flash -p <PORT> --monitor target/riscv32imc-unknown-none-elf/release/<your-project>
```

**GD32VF103 (Longan Nano):**

```sh
cd <your-project>
cargo build --release --features gd32v --target riscv32imac-unknown-none-elf
cargo flash --chip GD32VF103CBT6 --release --target riscv32imac-unknown-none-elf
```

The device boots, brings up the UART, and broadcasts its `DeviceManifest`
(capabilities + device id).

## Step 3 — See it in tpt-basestation

Run the mock BaseStation side on the host, pointed at the same serial port:

```sh
cargo run -p tpt-node-mock --bin mock_basestation
# or, for a WebSocket handshake against a running BaseStation:
TPT_SERIAL_PORT=<PORT> cargo run -p tpt-node-mock --bin mock_basestation
```

Open `tpt-basestation`. Within a moment your device's handshake arrives and the
matching **capability cards unlock automatically**:

- `esp32-blinky` registers `Serial` + `Ota` → the OTA / Serial cards activate.
- `riscv-sensor` registers `Telemetry` → the Telemetry card shows live samples.

That's it — no manual pairing, no config file.

## Developing on the host (no hardware)

You can build and exercise the whole node runtime on your laptop using
`tpt-node-mock`, which implements `LinkTransport` over WebSockets and registers
fake capabilities (including a host-backed LittleFS).

```sh
cargo run -p tpt-node-mock
```

Unit tests run against `tpt-e-link`'s mock transport:

```sh
cargo test -p tpt-node-core --features std
cargo test -p tpt-node-riscv --features std
```

## Next steps

- Read [`templates/esp32-blinky/src/main.rs`](../templates/esp32-blinky/src/main.rs)
  and [`templates/riscv-sensor/src/main.rs`](../templates/riscv-sensor/src/main.rs)
  to see how capabilities are registered.
- Add your own capability: implement `LittleFsDriver` / `TelemetryDriver` /
  `CommandHandler` and call `node.register_capability(...)`.
- See [`docs/manifest-mapping.md`](manifest-mapping.md) for how the wire
  manifest maps to `tpt-basestation`'s host-side view.
