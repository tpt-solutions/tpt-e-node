//! Minimal GD32VF103 node that runs the `tpt-node-core` reactor over a
//! blocking UART.
//!
//! This is the bring-up / handshake binary: no executor, no interrupts. The
//! [`BlockingUartTransport`]'s futures complete on their first poll, so a tiny
//! busy-loop `block_on` is all that is needed to drive [`Node::run`].
//!
//! Build + flash (GD32VF103, e.g. a Longan Nano board — USART0 = PA9 TX /
//! PA10 RX):
//!
//! ```text
//! cargo build --manifest-path crates/tpt-node-riscv/Cargo.toml \
//!     --example gd32v_node --release --features gd32v \
//!     --target riscv32imac-unknown-none-elf
//! cargo flash --chip GD32VF103CBT6 --release \
//!     --example gd32v_node -p riscv32imac-unknown-none-elf
//! ```
//!
//! Then, from the host, run the serial-side mock BaseStation against the same
//! port:
//!
//! ```text
//! TPT_SERIAL_PORT=COM3 cargo run -p tpt-node-mock --bin mock_basestation
//! ```

#![no_std]
#![no_main]

use core::future::Future;
use core::pin::pin;
use core::ptr;
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use gd32vf103xx_hal::gpio::GpioExt;
use gd32vf103xx_hal::pac;
use gd32vf103xx_hal::prelude::*;
use gd32vf103xx_hal::serial::{Config, Serial};
use panic_halt as _;
use tpt_e_link::{Arch, Version};
use tpt_node_core::{Capability, Node};
use tpt_node_riscv::transport::uart::BlockingUartTransport;

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

#[riscv_rt::entry]
fn main() -> ! {
    let dp = pac::Peripherals::take().unwrap();
    let mut rcu = dp.RCU.configure().freeze();
    let gpioa = dp.GPIOA.split(&mut rcu);

    let tx = gpioa.pa9.into_alternate_push_pull();
    let rx = gpioa.pa10.into_floating_input();
    let (mut tx, mut rx) =
        Serial::new(dp.USART0, (tx, rx), Config::default().baudrate(115_200.bps()), &rcu)
            .split();

    let transport = BlockingUartTransport::new(tx, rx);
    let mut node = Node::new(transport, *b"gd32v001", Version::new(0, 1, 0), Arch::RiscV32Imac);
    node.register_capability(Capability::Ota).unwrap();
    node.register_capability(Capability::Serial).unwrap();

    block_on(node.run()).unwrap();
    loop {}
}
