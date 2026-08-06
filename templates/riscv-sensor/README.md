# riscv-sensor

A minimal [TPT](https://github.com/tpt-solutions) RISC-V device firmware
template that advertises a `Telemetry` capability.

It boots a [`tpt-node-core`](../../crates/tpt-node-core) reactor over a blocking
UART, registers a `Telemetry` driver (here a synthetic incrementing sensor), and
streams samples to `tpt-basestation`, which unlocks the Telemetry card.

## Generate

```sh
cargo generate --git https://github.com/tpt-solutions/tpt-e-node --path templates/riscv-sensor
```

Answer the prompts for project name, chip family, and 8-byte device id.

## Build & flash (GD32VF103 / Longan Nano)

```sh
cargo build --release --features gd32v --target riscv32imac-unknown-none-elf
cargo flash --chip GD32VF103CBT6 --release --target riscv32imac-unknown-none-elf
```

SiFive E310 boards follow the same `tpt-node-riscv` wiring — see the `sifive`
branch in `src/main.rs` and `crates/tpt-node-riscv/examples/gd32v_node.rs`.

Then, from the host:

```sh
cargo run -p tpt-node-mock --bin mock_basestation
```

and watch live telemetry appear in `tpt-basestation`.
