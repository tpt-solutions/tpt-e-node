# tpt-node-core

Architecture-agnostic runtime and state machines for [TPT Embedded Node](https://github.com/tpt-solutions/tpt-e-node) devices.

This crate provides the "brains" of a TPT-capable device: a capability
registry, command dispatch, an internal OTA handler, and an async reactor loop
that drives a [`tpt_e_link::LinkTransport`].

It is `#![no_std]` + `alloc`, designed to run on bare-metal microcontrollers
(ESP32, RISC-V) under `embassy-executor`, while remaining testable on the host
via the `std` feature and `tpt-e-link`'s mock transport.

## Features

- `std` — enables `tpt-e-link/std` (mock transport) for host-side testing.

## Quick start (host-side test)

```no_run
# #[cfg(feature = "std")] {
use tpt_e_link::transport::mock::MockTcpTransport;
use tpt_e_link::{Arch, Version};
use tpt_node_core::{cmd, Capability, CommandHandler, LittleFsDriver, Node};

struct NopDriver;
impl CommandHandler for NopDriver {
    fn handles(&self, _id: u32) -> bool { false }
    fn handle(&mut self, _cmd: &tpt_e_link::CommandPayload) -> tpt_e_link::AckPayload {
        tpt_e_link::AckPayload { command_id: _cmd.command_id, success: true, error_code: 0 }
    }
}
impl LittleFsDriver for NopDriver {
    fn read_file(&mut self, _p: &str, _o: u32, _b: &mut [u8]) -> Result<usize, tpt_node_core::NodeError> { Ok(0) }
    fn write_file(&mut self, _p: &str, _o: u32, _d: &[u8]) -> Result<(), tpt_node_core::NodeError> { Ok(()) }
    fn list_dir(&mut self, _p: &str) -> Result<heapless::Vec<heapless::String<64>, 32>, tpt_node_core::NodeError> { Ok(heapless::Vec::new()) }
}
# pollster::block_on(async {
let (device, _host) = MockTcpTransport::pair().unwrap();
let mut node = Node::new(device, *b"tptnode0", Version::new(0, 1, 0), Arch::Esp32C3RiscV);
node.register_capability(Capability::Ota).unwrap();
node.register_capability(Capability::LittleFs(Box::new(NopDriver))).unwrap();
node.run().await.unwrap();
# });
# }
```

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE) at your option.
