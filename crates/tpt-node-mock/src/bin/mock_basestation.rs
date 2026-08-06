//! `mock-basestation` — a host-side stand-in for `tpt-basestation` that talks
//! to a physical device over a serial port using the `tpt-e-link` wire format.
//!
//! It opens the given serial port, decodes every frame the device sends with a
//! [`FrameDecoder`](tpt_e_link::frame::FrameDecoder), pretty-prints the
//! [`DeviceManifest`](tpt_e_link::DeviceManifest) from the boot-time
//! `Handshake`, echoes `Heartbeat`s, and fires one `Command` (the serial
//! capability's base ID) to exercise the command-dispatch round trip.
//!
//! ```sh
//! cargo run -p tpt-node-mock --bin mock_basestation
//! TPT_SERIAL_PORT=COM3 cargo run -p tpt-node-mock --bin mock_basestation
//! ```

use std::io::{Read, Write};
use std::time::Duration;

use tpt_e_link::frame::{encode, FrameDecoder, MAX_FRAME_SIZE};
use tpt_e_link::message::{CommandPayload, LinkMessage};

const SERIAL_BASE_CMD: u32 = 0x0400;

fn display_manifest(manifest: &tpt_e_link::DeviceManifest) {
    let id = String::from_utf8_lossy(&manifest.device_id);
    println!("  device_id        = {id}");
    println!("  firmware_version = {}", manifest.firmware_version);
    println!("  architecture     = {:?}", manifest.architecture);
    println!("  protocol_version = {}", manifest.protocol_version);
    println!(
        "  capabilities     = [{}]",
        manifest
            .capabilities
            .iter()
            .map(|c| format!("{c:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
}

fn send_frame(port: &mut impl Write, msg: &LinkMessage) -> std::io::Result<()> {
    let mut buf = [0u8; MAX_FRAME_SIZE];
    let n = encode(msg, &mut buf).map_err(std::io::Error::other)?;
    port.write_all(&buf[..n])?;
    port.flush()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let port_name = std::env::var("TPT_SERIAL_PORT").unwrap_or_else(|_| "COM3".to_string());
    let baud = std::env::var("TPT_SERIAL_BAUD")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(115_200);

    println!("[mock-basestation] opening {port_name} @ {baud} baud");
    let mut port = serialport::new(port_name.clone(), baud)
        .timeout(Duration::from_millis(500))
        .open()
        .map_err(|e| format!("failed to open {port_name}: {e}"))?;

    let mut decoder = FrameDecoder::new();
    let mut rx = [0u8; 256];
    let mut sent_serial_cmd = false;

    loop {
        let n = match port.read(&mut rx) {
            Ok(n) if n > 0 => n,
            Ok(_) => {
                // Serial port timeout; give the device a moment.
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(e) => return Err(e.into()),
        };

        decoder.feed_and_drain(&rx[..n], |result| match result {
            Ok(LinkMessage::Handshake(manifest)) => {
                println!("[mock-basestation] got Handshake from device:");
                display_manifest(&manifest);
                let _ = send_frame(&mut port, &LinkMessage::Heartbeat);
            }
            Ok(LinkMessage::Heartbeat) => {
                println!("[mock-basestation] got Heartbeat (echoing)");
                let _ = send_frame(&mut port, &LinkMessage::Heartbeat);
            }
            Ok(LinkMessage::Ack(ack)) => {
                println!(
                    "[mock-basestation] got Ack command_id=0x{:04X} success={} error_code={}",
                    ack.command_id, ack.success, ack.error_code
                );
            }
            Ok(LinkMessage::Telemetry(frame)) => {
                println!("[mock-basestation] got Telemetry t={}ms {:?}", frame.timestamp_ms, frame.samples);
            }
            Ok(LinkMessage::Log(log)) => {
                println!("[mock-basestation] got Log {:?} t={}ms {}", log.level, log.timestamp_ms, log.message);
            }
            Ok(LinkMessage::Command(cmd)) => {
                println!("[mock-basestation] got Command 0x{:04X} {:?}", cmd.command_id, cmd.args);
            }
            Err(e) => println!("[mock-basestation] frame error: {e:?}"),
        });

        if !sent_serial_cmd {
            sent_serial_cmd = true;
            let cmd = CommandPayload::new(SERIAL_BASE_CMD, &[])?;
            match send_frame(&mut port, &LinkMessage::Command(cmd)) {
                Ok(()) => println!("[mock-basestation] sent serial probe command 0x{SERIAL_BASE_CMD:04X}"),
                Err(e) => println!("[mock-basestation] failed to send command: {e}"),
            }
        }
    }
}
