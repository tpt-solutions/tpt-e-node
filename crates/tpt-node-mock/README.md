# tpt-node-mock

Mock-first TPT device for [TPT Embedded Node](https://github.com/tpt-solutions/tpt-e-node).

A `std` binary + library that implements [`tpt_e_link::LinkTransport`] over a
local WebSocket connection, then runs the real [`tpt_node_core::Node`] reactor
on top of it with fake, host-backed capabilities:

- **LittleFS** — a simulated filesystem backed by a real directory on the
  host's hard drive (`HostLittleFsDriver`).
- **OTA** — the internal `tpt-node-core` handler with an in-RAM flash writer.
- **Telemetry** — a simulated sensor driver.

This lets an engineer develop and test device logic, OTA flows, and
capability-gated BaseStation UI entirely on a laptop, before compiling for a
physical microcontroller.

## Running

```sh
cargo run -p tpt-node-mock
```

The mock connects to `ws://127.0.0.1:8375/ws` by default, so BaseStation should
be running its WebSocket listener (`start_ws_listener`).

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
