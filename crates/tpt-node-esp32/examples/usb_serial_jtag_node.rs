//! Minimal ESP32-C3 node that runs the `tpt-node-core` reactor over the
//! chip's native USB-Serial-JTAG port — no external USB-UART bridge needed,
//! so this is the bring-up binary for DevKitM/DevKitC-style C3 boards that
//! only break out the native USB port.
//!
//! Like [`uart_node`](super::uart_node): no executor, no interrupts. The
//! [`UsbSerialJtagTransport`]'s futures complete on their first poll, so a
//! tiny busy-loop `block_on` is all that is needed to drive [`Node::run`].
//!
//! Build + flash:
//!
//! ```text
//! . C:\Users\phill\export-esp.ps1
//! cargo build --manifest-path crates/tpt-node-esp32/Cargo.toml `
//!     --example usb_serial_jtag_node --release --features esp32c3 `
//!     --target riscv32imc-unknown-none-elf
//! espflash flash -p COM5 --monitor target/riscv32imc-unknown-none-elf/release/examples/usb_serial_jtag_node
//! ```
//!
//! Then, from the host, run the serial-side mock BaseStation against the
//! same port:
//!
//! ```text
//! TPT_SERIAL_PORT=COM5 cargo run -p tpt-node-mock --bin mock_basestation
//! ```

#![no_std]
#![no_main]

extern crate alloc;

use core::future::Future;
use core::pin::pin;
use core::ptr;
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use esp_alloc::heap_allocator;
use esp_backtrace as _;
use esp_hal::entry;
use esp_hal::usb_serial_jtag::UsbSerialJtag;
use esp_hal::Config as EspConfig;
use tpt_e_link::{Arch, Version};
use tpt_node_core::{Capability, Node};
use tpt_node_esp32::transport::usb_serial_jtag::UsbSerialJtagTransport;

fn noop_waker() -> Waker {
    fn noop(_: *const ()) {}
    fn clone(_: *const ()) -> RawWaker {
        RawWaker::new(ptr::null(), &VTABLE)
    }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
    // SAFETY: the vtable is a valid, static no-op vtable and the waker is
    // never actually woken (every future polled here completes on first poll).
    unsafe { Waker::from_raw(RawWaker::new(ptr::null(), &VTABLE)) }
}

fn block_on<F: Future>(fut: F) -> F::Output {
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);
    let mut fut = pin!(fut);
    loop {
        if let Poll::Ready(value) = fut.as_mut().poll(&mut cx) {
            return value;
        }
    }
}

#[entry]
fn main() -> ! {
    heap_allocator!(72 * 1024);
    let peripherals = esp_hal::init(EspConfig::default());

    let usb = UsbSerialJtag::new(peripherals.USB_DEVICE);
    let transport = UsbSerialJtagTransport::new(usb);

    let mut node = Node::new(transport, *b"esp32c3\0", Version::new(0, 1, 0), Arch::Esp32C3RiscV);
    node.register_capability(Capability::Ota).unwrap();
    node.register_capability(Capability::Serial).unwrap();

    match block_on(node.run()) {
        Ok(()) => loop {},
        Err(_) => loop {},
    }
}
