//! Host bring-up simulation for the RISC-V firmware.
//!
//! This stands in for the "flash it on real silicon" milestone: instead of a
//! GD32V/SiFive board, we wire the *real* embedded code path
//! (`BlockingUartTransport` + `Node::run`) to a mocked UART peripheral whose
//! TX/RX lines are in-memory ring buffers. The host side then:
//!
//! 1. Decodes the boot-time `Handshake` frame the device broadcasts and asserts
//!    the device id, architecture, and registered capabilities.
//! 2. Frames a `Command` (OTA start) the way a real BaseStation would, injects
//!    it into the device's RX line, and asserts the device replies with a
//!    well-formed `Ack` frame.
//!
//! Everything runs on the host with `--features std` — no embedded target, no
//! physical chip — yet it exercises exactly the bytes the firmware would put on
//! a real UART.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use embedded_hal_nb::serial::{ErrorKind, ErrorType, Read, Write};
use tpt_e_link::frame::{encode, FrameDecoder, MAGIC, MAX_FRAME_SIZE};
use tpt_e_link::message::LinkMessage;
use tpt_e_link::{Arch, Capability as WireCapability, Version};
use tpt_node_core::{Capability, Node};
use tpt_node_riscv::transport::uart::BlockingUartTransport;

/// In-memory byte pipe shared between the device's serial half and the host.
/// `Arc<Mutex<_>>` lets the device run on its own thread while the host feeds
/// and drains the same buffers.
type Pipe = Arc<Mutex<Vec<u8>>>;

/// Device-side transmit half: bytes the firmware writes land in `buf`.
struct DevTx {
    buf: Pipe,
}

/// Device-side receive half: bytes the host injects into `buf` are drained.
struct DevRx {
    buf: Pipe,
    eof: Arc<Mutex<bool>>,
}

impl ErrorType for DevTx {
    type Error = ErrorKind;
}
impl ErrorType for DevRx {
    type Error = ErrorKind;
}

impl Write<u8> for DevTx {
    fn write(&mut self, word: u8) -> nb::Result<(), Self::Error> {
        self.buf.lock().unwrap().push(word);
        Ok(())
    }
    fn flush(&mut self) -> nb::Result<(), Self::Error> {
        Ok(())
    }
}

impl Read<u8> for DevRx {
    fn read(&mut self) -> nb::Result<u8, Self::Error> {
        let mut buf = self.buf.lock().unwrap();
        if let Some(b) = buf.first().copied() {
            buf.remove(0);
            return Ok(b);
        }
        // Once the host signals end-of-stream, surface an error instead of
        // `WouldBlock` so the reactor's blocking `read_exact` fails and
        // `node.run()` returns, letting the bring-up thread join cleanly.
        if *self.eof.lock().unwrap() {
            return Err(nb::Error::Other(ErrorKind::Other));
        }
        Err(nb::Error::WouldBlock)
    }
}

/// The host-side peer: reads what the device transmitted (to decode frames)
/// and injects bytes for the device to receive.
struct HostPeer {
    rx_of_device: Pipe, // device reads from here
    tx_of_device: Pipe, // device writes here; host reads
}

impl HostPeer {
    /// Decodes every complete frame the device has transmitted so far and
    /// returns the most recent one (or `None` if no complete frame yet).
    fn next_message(&self) -> Option<LinkMessage> {
        let bytes = self.tx_of_device.lock().unwrap().clone();
        let mut decoder = FrameDecoder::new();
        let mut last = None;
        decoder.feed_and_drain(&bytes, |msg| {
            if let Ok(m) = msg {
                last = Some(m);
            }
        });
        last
    }

    /// Frames `msg` exactly as a real BaseStation would and injects it into the
    /// device's receive line.
    fn send(&self, msg: &LinkMessage) {
        let mut frame = [0u8; MAX_FRAME_SIZE];
        let n = encode(msg, &mut frame).expect("encode command frame");
        self.rx_of_device
            .lock()
            .unwrap()
            .extend_from_slice(&frame[..n]);
    }
}

#[test]
fn riscv_firmware_boots_handshakes_and_answers_commands() {
    let rx_of_device: Pipe = Arc::new(Mutex::new(Vec::new()));
    let tx_of_device: Pipe = Arc::new(Mutex::new(Vec::new()));
    let eof: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));

    let device = BlockingUartTransport::new(
        DevTx { buf: tx_of_device.clone() },
        DevRx {
            buf: rx_of_device.clone(),
            eof: eof.clone(),
        },
    );

    let mut node = Node::new(device, *b"rvsen001", Version::new(0, 1, 0), Arch::RiscV32);
    node.register_capability(Capability::Ota).unwrap();
    node.register_capability(Capability::Telemetry(Box::new(SyntheticSensor {
        reading: 0,
    })))
    .unwrap();

    // Drive the real reactor on its own thread, just like a bring-up binary
    // would drive it with block_on on the metal. The `let _ =` silences the
    // unused-Result warning: the reactor is expected to exit via a transport
    // error once the host signals EOF below.
    let device_thread = thread::spawn(move || {
        let _ = pollster::block_on(node.run());
    });

    let peer = HostPeer {
        rx_of_device: rx_of_device.clone(),
        tx_of_device: tx_of_device.clone(),
    };

    // 1. The device must broadcast a Handshake carrying the manifest.
    let handshake = wait_for(|| peer.next_message(), "device never transmitted a frame");
    let LinkMessage::Handshake(manifest) = handshake else {
        panic!("first frame was not a Handshake: {handshake:?}");
    };
    assert_eq!(manifest.device_id, *b"rvsen001");
    assert_eq!(manifest.architecture, Arch::RiscV32);
    // `Ota` is advertised on the wire; `Telemetry` carries a driver but has no
    // wire capability (per `Capability::to_wire`), so it is intentionally
    // absent from the broadcast manifest.
    assert!(manifest.capabilities.contains(&WireCapability::Ota));

    // 2. Inject an OTA-start command and expect an Ack back.
    let mut args = Vec::new();
    args.extend_from_slice(&256u32.to_le_bytes()); // expected_size
    args.extend_from_slice(&0x1234_5678u32.to_le_bytes()); // expected_crc
    let cmd = tpt_e_link::message::CommandPayload::new(0x0200, &args).expect("build ota-start");
    peer.send(&LinkMessage::Command(cmd));

    let ack = wait_for(
        || match peer.next_message() {
            Some(LinkMessage::Ack(a)) if a.command_id == 0x0200 => Some(a),
            _ => None,
        },
        "device never acked the OTA-start command",
    );
    assert!(ack.success, "OTA-start should be accepted by the internal handler");

    // Signal end-of-stream on the device's RX line so the reactor's blocking
    // read errors out and `node.run()` returns, letting the bring-up thread
    // join without hanging on the otherwise-infinite event loop.
    *eof.lock().unwrap() = true;
    device_thread.join().unwrap();
}

/// Minimal `TelemetryDriver` so the bring-up registers a real capability.
struct SyntheticSensor {
    reading: u16,
}

impl tpt_node_core::CommandHandler for SyntheticSensor {
    fn handles(&self, _: u32) -> bool {
        false
    }
    fn handle(&mut self, cmd: &tpt_e_link::message::CommandPayload) -> tpt_e_link::message::AckPayload {
        tpt_e_link::message::AckPayload {
            command_id: cmd.command_id,
            success: false,
            error_code: 0,
        }
    }
}

impl tpt_node_core::TelemetryDriver for SyntheticSensor {
    fn read_samples(
        &mut self,
    ) -> heapless::Vec<tpt_e_link::message::TelemetrySample, { tpt_e_link::message::MAX_TELEMETRY_SAMPLES }>
    {
        let mut samples = heapless::Vec::new();
        self.reading = self.reading.wrapping_add(1);
        let _ = samples.push(tpt_e_link::message::TelemetrySample {
            sensor_id: 1,
            value: self.reading as f32,
        });
        samples
    }
}

/// Poll `f` every 20ms until it returns `Some` (timeout 5s).
fn wait_for<T>(mut f: impl FnMut() -> Option<T>, msg: &str) -> T {
    for _ in 0..250 {
        if let Some(v) = f() {
            return v;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("timeout: {msg}");
}

// Keep the magic constant referenced so a stray magic byte in the stream can't
// be mistaken for an unrelated import; also documents the frame start marker.
const _: [u8; 2] = MAGIC;
