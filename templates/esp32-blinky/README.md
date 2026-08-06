# esp32-blinky

A minimal [TPT](https://github.com/tpt-solutions) device firmware template.

It boots a [`tpt-node-core`](../../crates/tpt-node-core) reactor over a blocking
UART, advertises the `Serial` + `Ota` capabilities, and blinks an LED so you
have a visible "it's alive" signal. Once flashed, `tpt-basestation` receives the
handshake and unlocks the matching capability cards.

## Generate

```sh
cargo generate --git https://github.com/tpt-solutions/tpt-e-node --path templates/esp32-blinky
```

Answer the prompts for project name, chip, and 8-byte device id.

## Build & flash

```sh
# pick your chip feature
cargo build --release --features esp32c3 --target riscv32imc-unknown-none-elf
espflash flash -p /dev/ttyUSB0 --monitor target/riscv32imc-unknown-none-elf/release/esp32-blinky
```

Then, from the host:

```sh
cargo run -p tpt-node-mock --bin mock_basestation
```

and watch the device appear in `tpt-basestation`.
