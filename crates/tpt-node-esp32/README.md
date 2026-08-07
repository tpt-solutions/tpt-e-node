# tpt-node-esp32

ESP32 HAL for [TPT Embedded Node](https://github.com/tpt-solutions/tpt-e-node).

Provides real-hardware [`tpt_e_link::LinkTransport`] implementations and
flash-backed drivers for the `tpt-node-core` reactor on the ESP32 family:

- `UartTransport` — an async UART transport driven by `esp-hal`'s
  interrupt-backed `Uart<'_, Async, _>`.
- `BleTransport` — a transport that carries `tpt-e-link` frames over a GATT
  byte pipe.
- `OtaRegionFlashWriter` — wires the internal OTA handler from `tpt-node-core`
  to real ESP32 flash via `embedded-storage`, with incremental CRC32.
- `RegionStorage` — a sub-region view over a `NorFlash` device for LittleFS.
- `EspLittleFsDriver` (feature `fs`) — a real
  [`tpt_node_core::LittleFsDriver`] backed by `littlefs2`.

The `tpt-e-typestate-hal` safe DMA/ISR layer is wired in as a dependency so
device firmware can use typestate-checked DMA for ring-buffer handoff and
hardware crypto.

## Chip selection

At most one chip feature may be enabled. Each pulls in the `esp-hal` stack and
the `tpt-e-typestate-hal` safe DMA/ISR layer for that chip:

- `esp32`
- `esp32s3`
- `esp32c3`
- `esp32c6`

Additional features:

- `std` — enables `tpt-e-link/std` (mock transport) for host-side testing.
- `fs` — enables the `littlefs2`-backed `EspLittleFsDriver`.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
