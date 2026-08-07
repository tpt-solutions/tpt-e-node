# tpt-node-riscv

Generic RISC-V HAL for [TPT Embedded Node](https://github.com/tpt-solutions/tpt-e-node).

Implements [`tpt_e_link::LinkTransport`] for UART using `riscv-rt` +
`embedded-hal` traits. This crate targets generic (non-ESP32) RISC-V silicon
such as the GigaDevice GD32VF103 (`gd32v` feature) and SiFive E310-class parts
(`sifive` feature).

Unlike `tpt-node-esp32`, this crate builds no ESP32-specific HAL primitives —
the transport layer sits directly on the `embedded-hal`/`embedded-hal-nb`
serial traits, which every generic RISC-V BSP implements, so a single
transport implementation covers every supported chip.

## Chip selection

Enable exactly one chip feature to pull in the matching `riscv-rt` /
`embedded-hal` BSP:

- `gd32v`
- `sifive`

## Features

- `std` — enables `tpt-e-link/std` and `tpt-node-core/std` for host-side testing.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
