# TPT Embedded Node (tpt-e-node)

**Device Firmware SDK for the TPT Ecosystem**

[![Status](https://img.shields.io/badge/status-Phase%201%20Development-yellow)](https://github.com/tpt/tpt-e-node)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-blue)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange)](https://www.rust-lang.org)

---

## Overview

**tpt-e-node** is the device-side runtime and Hardware Abstraction Layer (HAL) SDK for the [TPT (Telemetry Protocol Transport)](https://github.com/tpt) ecosystem. While [`tpt-e-link`](https://github.com/tpt/tpt-e-link) defines the protocol language, **tpt-e-node provides the muscle and brain for the physical hardware**.

It is structured as a **Rust workspace** that allows:
- **Hobbyists** to flash a pre-built template in **5 minutes**
- **Engineers** to build custom, production-grade firmware in a **memory-safe, `no_std` environment** for **ESP32** and **RISC-V** architectures

---

## Key Features

| Feature | Description |
|---------|-------------|
| 🚀 **5-Minute Setup** | Clone a template, flash it, and see your device instantly recognized in [tpt-basestation](https://github.com/tpt/tpt-basestation) |
| 🔄 **Architecture Agnostic** | Identical core runtime logic runs on ESP32 (Xtensa/RISC-V) and generic RISC-V chips |
| 🛡️ **Memory Safe** | Leverages Rust's `no_std` ecosystem to eliminate buffer overflows and memory leaks |
| 🧩 **Composable Capabilities** | Register hardware features (filesystem, sensors, OTA) for automatic BaseStation discovery |
| 🧪 **Mock-First Development** | Test device logic entirely on your laptop via `tpt-node-mock` before deploying to hardware |

---

## Architecture

```
tpt-e-node/
├── Cargo.toml                 # Workspace root
├── crates/
│   ├── tpt-node-core/         # Architecture-agnostic runtime & state machines
│   ├── tpt-node-esp32/        # ESP32 HAL (UART, BLE, Flash) using `esp-hal`
│   └── tpt-node-riscv/        # Generic RISC-V HAL using `riscv-rt` + `embedded-hal`
└── templates/
    ├── esp32-blinky/          # Quickstart template for ESP32 hobbyists
    └── riscv-sensor/          # Quickstart template for RISC-V hobbyists
```

### Core Crates

| Crate | Purpose |
|-------|---------|
| **`tpt-node-core`** | The "brains" — async reactor/event loop, capability registry, internal handlers (OTA, LittleFS) |
| **`tpt-node-esp32`** | The "muscle" for ESP32 — implements `LinkTransport` for UART/BLE, handles partitions & flash |
| **`tpt-node-riscv`** | The "muscle" for generic RISC-V — implements `LinkTransport` via `embedded-hal` traits |

---

## How It Works

```mermaid
sequenceDiagram
    participant Device as tpt-e-node Device
    participant Transport as tpt-e-link Transport
    participant BaseStation as tpt-basestation

    Device->>Transport: Initialize hardware (ESP32/RISC-V HAL)
    Device->>Transport: Construct DeviceManifest (capabilities)
    Device->>BaseStation: Broadcast manifest
    BaseStation->>Device: Dynamic UI rendering (filesystem, telemetry, OTA)
    Device->>BaseStation: Runtime telemetry & command handling
```

### Capability Registration (User Code)

```rust
// Example: Registering capabilities in your firmware
node.register_capability(Capability::LittleFS(my_fs_driver));
node.register_capability(Capability::Telemetry(my_sensor_reader));
node.register_capability(Capability::OTA); // Automatic chunked download + CRC + flash
```

---

## Quickstart (5 Minutes)

```bash
# 1. Install toolchain
cargo install cargo-generate probe-rs-tools

# 2. Generate project from template
cargo generate --git https://github.com/tpt/tpt-e-node --template esp32-blinky

# 3. Flash to device
cd my-device && cargo run --release
```

> **Result:** Your device appears in tpt-basestation instantly with a live telemetry dashboard.

---

## Mock-First Development

Develop and test **entirely on your laptop** — no hardware required:

```bash
# Run simulated device with fake filesystem & sensors
cargo run --bin tpt-node-mock
```

This starts a WebSocket-based mock transport that connects to tpt-basestation, letting you:
- Test OTA update flows
- Validate telemetry streaming
- Debug command handling
- Iterate on UI/UX before flashing real silicon

---

## Roadmap

| Phase | Focus | Status |
|-------|-------|--------|
| **Phase 1** | Cargo workspace setup, `LinkTransport` trait, `tpt-node-core` event loop (`embassy-executor`) | 🟡 In Progress |
| **Phase 2** | `tpt-node-esp32` implementation, ESP32-C3 handshake with mock BaseStation | ⏳ Planned |
| **Phase 3** | Internal OTA & LittleFS handlers in `tpt-node-core` | ⏳ Planned |
| **Phase 4** | `cargo-generate` templates, "5-Minute Quickstart" docs | ⏳ Planned |

---

## Ecosystem

| Component | Description |
|-----------|-------------|
| **[tpt-e-link](https://github.com/tpt/tpt-e-link)** | Protocol definition & transport traits |
| **[tpt-basestation](https://github.com/tpt/tpt-basestation)** | Web-based device management & visualization UI |
| **tpt-e-node** | This repo — device firmware SDK |

---

## License

Licensed under either of:
- **MIT License** ([LICENSE-MIT](LICENSE-MIT))
- **Apache License 2.0** ([LICENSE-APACHE](LICENSE-APACHE))

at your option.

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines. We welcome:
- Hardware support for new MCUs
- Capability implementations (sensors, actuators, storage)
- Documentation improvements
- Bug reports & feature requests

---

**Part of the [TPT](https://github.com/tpt) ecosystem** — Making embedded IoT development delightful.