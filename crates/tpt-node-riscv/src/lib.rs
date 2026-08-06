//! tpt-node-riscv — Generic RISC-V HAL for TPT Embedded Node.
//!
//! Implements [`tpt_e_link::LinkTransport`] for UART using `riscv-rt` +
//! `embedded-hal` traits. This crate targets generic (non-ESP32) RISC-V
//! silicon such as the GigaDevice GD32VF103 (`gd32v` feature) and SiFive
//! E310-class parts (`sifive` feature).
//!
//! Unlike [`tpt_node_esp32`], this crate builds **no** ESP32-specific HAL
//! primitives — `tpt-embedded-core` is ESP32-only, so nothing from it is
//! reused here (per the Phase 5 decision). The transport layer sits directly
//! on the `embedded-hal`/`embedded-hal-nb` serial traits, which every generic
//! RISC-V BSP implements, so a single transport implementation covers every
//! supported chip.
//!
//! # Wiring into `tpt-node-core`
//!
//! ```text
//! let serial = gd32vf103_pac::Peripherals::take().unwrap().USART0;
//! let tx = gpio.into_alternate_push_pull();
//! let rx = gpio.into_floating_input();
//! let (mut tx, mut rx) = Serial::new(serial, (tx, rx), 115_200.bps(), &clocks).split();
//! let transport = UartTransport::new(tx, rx);
//! let mut node = tpt_node_core::Node::new(
//!     transport, *b"gd32v001", Version::new(0, 1, 0), Arch::RiscV32,
//! );
//! node.register_capability(Capability::Ota).unwrap();
//! # pollster::block_on(async { node.run().await.unwrap(); });
//! ```
//!
//! `read_exact` maps every `nb` error to [`LinkError::Io`] (the wire layer
//! retries framing). The [`BlockingUartTransport`] counterpart completes each
//! future on its first poll, so the reactor can be driven with a trivial
//! busy-loop `block_on` for first bring-up (no executor required).

#![cfg_attr(not(feature = "std"), no_std)]
#![warn(missing_docs)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod transport;

#[cfg(any(feature = "gd32v", feature = "sifive"))]
pub use transport::{BlockingUartTransport, UartTransport};
