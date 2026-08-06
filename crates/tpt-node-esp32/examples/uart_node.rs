//! Minimal classic-ESP32 node that runs the `tpt-node-core` reactor over a
//! blocking hardware UART.
//!
//! This is the bring-up / handshake binary: no executor, no interrupts. The
//! [`BlockingUartTransport`]'s futures complete on their first poll, so a tiny
//! busy-loop `block_on` is all that is needed to drive [`Node::run`].
//!
//! Build + flash (classic ESP32, e.g. an ESP32 DevKit with an FTDI bridge on
//! UART0 = GPIO1 TX / GPIO3 RX):
//!
//! ```text
//! . C:\Users\phill\export-esp.ps1
//! $env:RUSTFLAGS = "-C link-arg=-Tlinkall.x"
//! rustup run esp cargo build --manifest-path crates/tpt-node-esp32/Cargo.toml `
//!     --example uart_node --release --features esp32 `
//!     --target xtensa-esp32-none-elf -Z build-std=core,alloc
//! espflash flash -p COM3 --monitor target/xtensa-esp32-none-elf/release/examples/uart_node
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
use esp_hal::clock::CpuClock;
use esp_hal::entry;
use esp_hal::Config as EspConfig;
use esp_println::println;
use tpt_e_link::{Arch, Version};
use tpt_node_core::{Capability, Node};
use tpt_node_esp32::transport::uart::BlockingUartTransport;

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
    let peripherals = esp_hal::init(EspConfig::default().with_cpu_clock(CpuClock::Clock240MHz));

    let uart = esp_hal::uart::Uart::new_with_config(
        peripherals.UART0,
        esp_hal::uart::Config::default().baudrate(115_200),
        peripherals.GPIO3, // U0RXD
        peripherals.GPIO1, // U0TXD
    )
    .unwrap();
    let transport = BlockingUartTransport::new(uart);

    let mut node = Node::new(transport, *b"esp32001", Version::new(0, 1, 0), Arch::Esp32Xtensa);
    node.register_capability(Capability::Ota).unwrap();
    node.register_capability(Capability::Serial).unwrap();

    println!("[tpt-node] booting device {} v{}", "esp32001", Version::new(0, 1, 0));
    match block_on(node.run()) {
        Ok(()) => println!("[tpt-node] reactor exited cleanly (unexpected)"),
        Err(e) => println!("[tpt-node] reactor stopped: {:?}", e),
    }
    loop {}
}
