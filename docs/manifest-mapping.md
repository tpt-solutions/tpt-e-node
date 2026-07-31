# DeviceManifest field mapping: `tpt-e-link` ↔ `tpt-basestation::hal`

Two distinct `DeviceManifest` types exist in the ecosystem, by design (see
`todo.md` Phase 2):

- **`tpt_e_link::manifest::DeviceManifest`** (`tpt-e-link/src/manifest.rs`) —
  the `no_std` wire format. Minimal, `postcard`-serialized, device-authoritative:
  it's what a physical or mock node actually broadcasts on boot.
- **`tpt_basestation_hal::manifest::DeviceManifest`** (`tpt-basestation/hal/src/manifest.rs`) —
  a host-side, TOML-based *static* manifest describing how to *recognize* a
  device from USB enumeration (VID/PID, boot fingerprint regex, handshake
  probe) before any `tpt-e-link` traffic has been exchanged. It has no
  relationship to a specific booted device instance.

There is no direct struct-to-struct conversion between these two — they answer
different questions ("what did this device just tell me about itself?" vs.
"how do I recognize a device family from a USB descriptor?"). Instead, a wire
`DeviceManifest` (or, for network transports, the `WsDevice` view over it) is
projected into a `DiscoveredDevice`, the common result type both discovery
paths (USB/serial and network) produce. See
`tpt-basestation/hal/src/discovery/mod.rs::DeviceDiscovery::discover_network`.

## Field mapping

| `tpt_e_link::DeviceManifest` field | Wire type | Projected into `DiscoveredDevice` as | Notes |
|---|---|---|---|
| `device_id` | `[u8; 8]` | `description` (ASCII/hex string, via `WsDevice::device_id`) | Also the `WsRegistry` key and the discovery `port` disambiguator. |
| `firmware_version` | `tpt_e_link::Version` | *(not currently surfaced — `WsDevice::firmware_version` is display-only)* | No equivalent field on `DiscoveredDevice`; available on `WsDevice` if the UI needs it later. |
| `architecture` | `tpt_e_link::manifest::Arch` | *(not currently surfaced)* | Same as above — carried on `WsDevice::architecture`, not yet projected further. |
| `protocol_version` | `u8` | *(not projected — checked against `PROTOCOL_VERSION` at the transport layer)* | A mismatch is a connection-level concern, not a discovery-result field. |
| `capabilities` | `Vec<tpt_e_link::manifest::Capability, 16>` | `capabilities: CapabilitySet` (`Vec<DeviceCapability>`) | See capability mapping below. `Serial` is also unconditionally added for every network device regardless of what it reports (the WebSocket connection itself proves serial-equivalent connectivity). |
| *(n/a — no wire equivalent)* | — | `port: String` | Set to the device's WebSocket URL (`WsDevice::url`) for network transport, standing in for a USB port path. |
| *(n/a)* | — | `confidence: DiscoveryConfidence` | Always `Handshake` for network devices — the manifest came from the device itself, not inferred from VID/PID. |
| *(n/a)* | — | `transport: DeviceTransport` | Always `Network` for WebSocket-connected devices. |
| *(n/a)* | — | `manifest: Option<hal::manifest::DeviceManifest>` | Always `None` for network devices — there is no static TOML manifest lookup involved; the device is self-describing. |

## Capability mapping

The wire `Capability` enum (`tpt-e-link`) and the host-side `DeviceCapability`
enum (`tpt-basestation::hal::capability`) are separate types with an
intentionally *narrower* wire vocabulary — not every wire capability has a
host-visible UI concept, and not every host capability can be self-reported
over the wire.

`tpt_node_core::Capability` (device-side, `tpt-e-node/crates/tpt-node-core/src/capability.rs`)
sits between the two: it's what firmware registers, and `Capability::to_wire()`
(`tpt-node-core/src/capability.rs:137`) downgrades it to the wire enum where
possible. On the host, `capability_names()` (`tpt-basestation/core/src/ws.rs:279`)
turns wire capabilities into strings, and `ws_capabilities()`
(`tpt-basestation/hal/src/discovery/mod.rs:18`) turns those strings into
`DeviceCapability` flags.

| `tpt_node_core::Capability` (device) | `tpt_e_link::Capability` (wire) | serialized name (`WsDevice.capabilities`) | `hal::DeviceCapability` (host UI) |
|---|---|---|---|
| `Serial` | `Uart` | `serial` | `Serial` — but see note\* |
| `Ble` | `BleProvision` | `ble` | `Ble` |
| `Ota` | `Ota` | `ota` | `Ota` |
| `LittleFs(driver)` | `LittleFs` | `little_fs` | `LittleFs` |
| `Flash` | *(no wire repr — `to_wire()` returns `None`)* | — | `Flash` *(host-only; no device path reports it today)* |
| `Debug` | *(no wire repr)* | — | `Debug` *(host-only)* |
| `Probe` | *(no wire repr)* | — | `Probe` *(host-only)* |
| `Mesh` | *(no wire repr)* | — | `Mesh` *(host-only)* |
| `Telemetry(driver)` | *(no wire repr)* | — | *(no host equivalent yet)* |
| *(no device equivalent)* | `Wifi`, `I2c`, `Spi`, `Adc`, `Pwm`, `Gpio` | `wifi`, `i2c`, `spi`, `adc`, `pwm`, `gpio` | *(not matched by `ws_capabilities()` — silently dropped)* |

\* `ws_capabilities()` unconditionally pushes `DeviceCapability::Serial` for
every network device — the WebSocket connection itself proves serial-
equivalent connectivity — and does **not** match the `"serial"` string
against anything in its `match`. So a device's `Capability::Serial`
registration reaching the wire as `"serial"` is redundant with, not the
source of, the host's `Serial` flag; omitting `Capability::Serial`
device-side has no visible effect on discovery today.

The six wire-only variants (`Wifi`/`I2c`/`Spi`/`Adc`/`Pwm`/`Gpio`) and the
four host-only variants (`Flash`/`Debug`/`Probe`/`Mesh`) exist for future
`tpt-node-esp32`/`tpt-node-riscv` HAL work (Phase 4/5) and BaseStation UI
concepts respectively — there is no current path connecting them. Extend
`tpt_node_core::Capability`, `ws_capabilities()`, and (if a new UI card is
needed) `hal::DeviceCapability` together when a HAL crate starts reporting
one of these.

## Where this is enforced

- Wire format: `tpt-e-link/src/manifest.rs` (`Capability`, `Arch`, `DeviceManifest`).
- Device-side registration → wire projection: `tpt-e-node/crates/tpt-node-core/src/capability.rs` (`Capability::to_wire`).
- Host-side capability string → `DeviceCapability`: `tpt-basestation/hal/src/discovery/mod.rs` (`ws_capabilities`).
- Host-side wire manifest → `DiscoveredDevice`: `tpt-basestation/hal/src/discovery/mod.rs` (`DeviceDiscovery::discover_network`).
- Static (USB/serial) `hal::manifest::DeviceManifest`: `tpt-basestation/hal/src/manifest.rs` — unrelated to the wire format; do not conflate the two.
