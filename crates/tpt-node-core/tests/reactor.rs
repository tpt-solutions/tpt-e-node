//! Unit tests for `tpt-node-core`.
//!
//! Tests run on the host (`std` feature) using `tpt-e-link`'s `MockTcpTransport`
//! and `pollster::block_on`, following the same pattern as `tpt-e-link`'s own
//! test suite.

#![cfg(feature = "std")]

use tpt_e_link::frame::{encode, FrameDecoder, MAX_FRAME_SIZE};
use tpt_e_link::transport::mock::MockTcpTransport;
use tpt_e_link::{
    AckPayload, Arch, CommandPayload, LinkMessage, LinkTransport, TelemetrySample, Version,
};

use tpt_node_core::{
    cmd, Capability, CapabilityRegistry, CommandHandler, LittleFsDriver, Node, NodeError,
    OtaHandler, RamFlashWriter, TelemetryDriver,
};

// ---------------------------------------------------------------------------
// Mock drivers
// ---------------------------------------------------------------------------

struct MockLittleFsDriver {
    last_cmd: core::cell::RefCell<Option<CommandPayload>>,
}

impl MockLittleFsDriver {
    fn new() -> Self {
        Self {
            last_cmd: core::cell::RefCell::new(None),
        }
    }
}

impl CommandHandler for MockLittleFsDriver {
    fn handles(&self, command_id: u32) -> bool {
        (cmd::LITTLEFS_BASE..cmd::LITTLEFS_BASE + 0x100).contains(&command_id)
    }

    fn handle(&mut self, cmd: &CommandPayload) -> AckPayload {
        *self.last_cmd.borrow_mut() = Some(cmd.clone());
        AckPayload {
            command_id: cmd.command_id,
            success: true,
            error_code: 0,
        }
    }
}

impl LittleFsDriver for MockLittleFsDriver {
    fn read_file(
        &mut self,
        _path: &str,
        _offset: u32,
        _buf: &mut [u8],
    ) -> Result<usize, NodeError> {
        Ok(0)
    }
    fn write_file(&mut self, _path: &str, _offset: u32, _data: &[u8]) -> Result<(), NodeError> {
        Ok(())
    }
    fn list_dir(
        &mut self,
        _path: &str,
    ) -> Result<heapless::Vec<heapless::String<64>, 32>, NodeError> {
        Ok(heapless::Vec::new())
    }
}

struct MockTelemetryDriver;

impl CommandHandler for MockTelemetryDriver {
    fn handles(&self, command_id: u32) -> bool {
        (cmd::TELEMETRY_BASE..cmd::TELEMETRY_BASE + 0x100).contains(&command_id)
    }

    fn handle(&mut self, cmd: &CommandPayload) -> AckPayload {
        AckPayload {
            command_id: cmd.command_id,
            success: true,
            error_code: 0,
        }
    }
}

impl TelemetryDriver for MockTelemetryDriver {
    fn read_samples(
        &mut self,
    ) -> heapless::Vec<TelemetrySample, { tpt_e_link::message::MAX_TELEMETRY_SAMPLES }> {
        let mut samples = heapless::Vec::new();
        let _ = samples.push(TelemetrySample {
            sensor_id: 1,
            value: 21.5,
        });
        samples
    }
}

// ---------------------------------------------------------------------------
// Frame I/O helpers (mirrors the loopback.rs pattern)
// ---------------------------------------------------------------------------

async fn read_frame(transport: &mut MockTcpTransport) -> LinkMessage {
    let mut header = [0u8; 5];
    transport
        .read_exact(&mut header)
        .await
        .expect("read header");
    let payload_len = u16::from_le_bytes([header[2], header[3]]) as usize;
    let mut rest = [0u8; MAX_FRAME_SIZE];
    let rest_len = payload_len + 4;
    transport
        .read_exact(&mut rest[..rest_len])
        .await
        .expect("read rest");

    let mut decoder = FrameDecoder::new();
    decoder.feed(&header).unwrap();
    decoder.feed(&rest[..rest_len]).unwrap();
    decoder
        .try_parse()
        .expect("decode")
        .expect("complete frame")
}

async fn write_frame(transport: &mut MockTcpTransport, msg: &LinkMessage) {
    let mut buf = [0u8; MAX_FRAME_SIZE];
    let len = encode(msg, &mut buf).expect("encode");
    transport.write_all(&buf[..len]).await.expect("write_all");
}

// ---------------------------------------------------------------------------
// Capability enum tests
// ---------------------------------------------------------------------------

#[test]
fn capability_to_wire_mapping() {
    assert_eq!(
        Capability::LittleFs(Box::new(MockLittleFsDriver::new())).to_wire(),
        Some(tpt_e_link::Capability::LittleFs)
    );
    assert_eq!(Capability::Ota.to_wire(), Some(tpt_e_link::Capability::Ota));
    assert_eq!(
        Capability::Ble.to_wire(),
        Some(tpt_e_link::Capability::BleProvision)
    );
    assert_eq!(
        Capability::Serial.to_wire(),
        Some(tpt_e_link::Capability::Uart)
    );
    assert_eq!(Capability::Flash.to_wire(), None);
    assert_eq!(Capability::Debug.to_wire(), None);
    assert_eq!(Capability::Mesh.to_wire(), None);
    assert_eq!(Capability::Probe.to_wire(), None);
    assert_eq!(
        Capability::Telemetry(Box::new(MockTelemetryDriver)).to_wire(),
        None
    );
}

#[test]
fn capability_into_handler_extracts_drivers() {
    let cap = Capability::LittleFs(Box::new(MockLittleFsDriver::new()));
    assert!(cap.into_handler().is_some());

    let cap = Capability::Ota;
    assert!(cap.into_handler().is_none());
}

// ---------------------------------------------------------------------------
// CapabilityRegistry tests
// ---------------------------------------------------------------------------

#[test]
fn registry_builds_correct_manifest() {
    let mut reg = CapabilityRegistry::new();
    reg.register(Capability::Ota).unwrap();
    reg.register(Capability::Serial).unwrap();
    reg.register(Capability::Ble).unwrap();
    reg.register(Capability::LittleFs(Box::new(MockLittleFsDriver::new())))
        .unwrap();
    reg.register(Capability::Telemetry(Box::new(MockTelemetryDriver)))
        .unwrap();

    let manifest = reg.manifest(*b"testnode", Version::new(1, 2, 3), Arch::Esp32C3RiscV);

    assert_eq!(manifest.device_id, *b"testnode");
    assert_eq!(manifest.firmware_version, Version::new(1, 2, 3));
    assert_eq!(manifest.architecture, Arch::Esp32C3RiscV);
    assert_eq!(manifest.protocol_version, tpt_e_link::PROTOCOL_VERSION);

    // Only capabilities with a wire representation are advertised.
    assert_eq!(manifest.capabilities.len(), 4);
    assert!(manifest.capabilities.contains(&tpt_e_link::Capability::Ota));
    assert!(manifest
        .capabilities
        .contains(&tpt_e_link::Capability::Uart));
    assert!(manifest
        .capabilities
        .contains(&tpt_e_link::Capability::BleProvision));
    assert!(manifest
        .capabilities
        .contains(&tpt_e_link::Capability::LittleFs));
}

#[test]
fn registry_rejects_overflow() {
    let mut reg = CapabilityRegistry::new();
    for _ in 0..tpt_e_link::MAX_CAPABILITIES {
        reg.register(Capability::Flash).unwrap();
    }
    let result = reg.register(Capability::Ota);
    assert_eq!(result, Err(NodeError::RegistryFull));
}

#[test]
fn registry_dispatch_routes_to_correct_handler() {
    let mut reg = CapabilityRegistry::new();
    reg.register(Capability::LittleFs(Box::new(MockLittleFsDriver::new())))
        .unwrap();
    reg.register(Capability::Ota).unwrap();

    let cmd = CommandPayload::new(cmd::LITTLEFS_READ, &[]).unwrap();
    let ack = reg.dispatch(&cmd).expect("handler should be found");
    assert!(ack.success);
    assert_eq!(ack.command_id, cmd::LITTLEFS_READ);

    // No handler for telemetry command IDs.
    let cmd = CommandPayload::new(cmd::TELEMETRY_READ, &[]).unwrap();
    assert!(reg.dispatch(&cmd).is_none());
}

#[test]
fn registry_has_wire_detection() {
    let mut reg = CapabilityRegistry::new();
    reg.register(Capability::Ota).unwrap();
    reg.register(Capability::Flash).unwrap();
    reg.register(Capability::LittleFs(Box::new(MockLittleFsDriver::new())))
        .unwrap();

    assert!(reg.has_wire(tpt_e_link::Capability::Ota));
    assert!(reg.has_wire(tpt_e_link::Capability::LittleFs));
    assert!(!reg.has_wire(tpt_e_link::Capability::Uart));
}

// ---------------------------------------------------------------------------
// OtaHandler tests
// ---------------------------------------------------------------------------

fn make_start_args(size: u32, crc: u32) -> heapless::Vec<u8, 32> {
    let mut args: heapless::Vec<u8, 32> = heapless::Vec::new();
    args.extend_from_slice(&size.to_le_bytes()).unwrap();
    args.extend_from_slice(&crc.to_le_bytes()).unwrap();
    args
}

fn make_chunk_args(offset: u32, data: &[u8]) -> heapless::Vec<u8, 32> {
    let mut args: heapless::Vec<u8, 32> = heapless::Vec::new();
    args.extend_from_slice(&offset.to_le_bytes()).unwrap();
    args.extend_from_slice(data).unwrap();
    args
}

#[test]
fn ota_handler_full_flow() {
    let data = b"hello tpt-e-node OTA!";
    let expected_crc = {
        const CRC: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);
        CRC.checksum(data)
    };

    let writer = Box::new(RamFlashWriter::new());
    let mut handler = OtaHandler::new(writer);

    // Start
    let start_cmd = CommandPayload::new(
        cmd::OTA_START,
        &make_start_args(data.len() as u32, expected_crc),
    )
    .unwrap();
    let ack = handler.handle(&start_cmd);
    assert!(ack.success);
    assert!(handler.is_active());

    // Send data in two chunks
    let mid = data.len() / 2;
    let chunk1 = CommandPayload::new(cmd::OTA_CHUNK, &make_chunk_args(0, &data[..mid])).unwrap();
    let ack = handler.handle(&chunk1);
    assert!(ack.success);

    let chunk2 =
        CommandPayload::new(cmd::OTA_CHUNK, &make_chunk_args(mid as u32, &data[mid..])).unwrap();
    let ack = handler.handle(&chunk2);
    assert!(ack.success);

    // Finish — CRC should match
    let finish_cmd = CommandPayload::new(cmd::OTA_FINISH, &[]).unwrap();
    let ack = handler.handle(&finish_cmd);
    assert!(ack.success);
    assert!(!handler.is_active());
}

#[test]
fn ota_handler_rejects_bad_crc() {
    let data = b"hello tpt-e-node OTA!";
    let wrong_crc = 0xDEADBEEF;

    let writer = Box::new(RamFlashWriter::new());
    let mut handler = OtaHandler::new(writer);

    let start_cmd = CommandPayload::new(
        cmd::OTA_START,
        &make_start_args(data.len() as u32, wrong_crc),
    )
    .unwrap();
    let ack = handler.handle(&start_cmd);
    assert!(ack.success);

    let chunk_cmd = CommandPayload::new(cmd::OTA_CHUNK, &make_chunk_args(0, data)).unwrap();
    let ack = handler.handle(&chunk_cmd);
    assert!(ack.success);

    let finish_cmd = CommandPayload::new(cmd::OTA_FINISH, &[]).unwrap();
    let ack = handler.handle(&finish_cmd);
    assert!(!ack.success);
    assert_eq!(ack.error_code, 4); // CRC mismatch
    assert!(!handler.is_active());
}

#[test]
fn ota_handler_rejects_chunk_without_start() {
    let writer = Box::new(RamFlashWriter::new());
    let mut handler = OtaHandler::new(writer);

    let chunk_cmd = CommandPayload::new(cmd::OTA_CHUNK, &make_chunk_args(0, b"data")).unwrap();
    let ack = handler.handle(&chunk_cmd);
    assert!(!ack.success);
    assert_eq!(ack.error_code, 2); // not in Receiving state
}

#[test]
fn ota_handler_rejects_oversized_image() {
    let writer = Box::new(RamFlashWriter::new());
    let mut handler = OtaHandler::new(writer);

    // Declare a large image
    let start_cmd = CommandPayload::new(cmd::OTA_START, &make_start_args(99999, 0)).unwrap();
    let ack = handler.handle(&start_cmd);
    assert!(ack.success); // start always succeeds; failure happens on write

    // Write at an offset that would exceed the 1024-byte buffer
    let big_data = [0u8; 28];
    let chunk_cmd = CommandPayload::new(cmd::OTA_CHUNK, &make_chunk_args(1000, &big_data)).unwrap();
    let ack = handler.handle(&chunk_cmd);
    assert!(!ack.success);
    assert_eq!(ack.error_code, 3); // write failed
}

// ---------------------------------------------------------------------------
// Node integration tests
// ---------------------------------------------------------------------------

#[test]
fn command_32_bytes_through_transport() {
    let (mut device, mut host) = MockTcpTransport::pair().unwrap();

    let mut args: heapless::Vec<u8, 32> = heapless::Vec::new();
    args.extend_from_slice(&42u32.to_le_bytes()).unwrap();
    args.extend_from_slice(&[0xAA; 28]).unwrap();

    let cmd = CommandPayload::new(cmd::OTA_CHUNK, &args).unwrap();
    let msg = LinkMessage::Command(cmd.clone());

    pollster::block_on(write_frame(&mut host, &msg));
    let received = pollster::block_on(read_frame(&mut device));

    match received {
        LinkMessage::Command(received_cmd) => {
            assert_eq!(received_cmd.command_id, cmd.command_id);
            assert_eq!(received_cmd.args.len(), 32, "args length should be 32");
            assert_eq!(&received_cmd.args[..], &cmd.args[..], "args should match");
        }
        _ => panic!("expected Command"),
    }
}

#[test]
fn command_payload_32_byte_args_round_trip() {
    let mut args: heapless::Vec<u8, 32> = heapless::Vec::new();
    args.extend_from_slice(&0u32.to_le_bytes()).unwrap();
    args.extend_from_slice(&[1u8; 28]).unwrap();
    assert_eq!(args.len(), 32);

    let cmd = CommandPayload::new(cmd::OTA_CHUNK, &args).unwrap();
    assert_eq!(cmd.args.len(), 32);

    let msg = LinkMessage::Command(cmd.clone());
    let mut buf = [0u8; MAX_FRAME_SIZE];
    let len = encode(&msg, &mut buf).unwrap();

    let mut decoder = FrameDecoder::new();
    decoder.feed(&buf[..len]).unwrap();
    let decoded = decoder.try_parse().unwrap().unwrap();

    match decoded {
        LinkMessage::Command(decoded_cmd) => {
            assert_eq!(decoded_cmd.command_id, cmd.command_id);
            assert_eq!(decoded_cmd.args.len(), 32);
            assert_eq!(&decoded_cmd.args[..], &cmd.args[..]);
        }
        _ => panic!("expected Command"),
    }
}

#[test]
fn ota_through_frame_round_trip() {
    let mut handler = OtaHandler::new(Box::new(RamFlashWriter::new()));

    let firmware = b"tpt-e-node test firmware image data";
    let crc = {
        const CRC: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);
        CRC.checksum(firmware)
    };

    let mut buf = [0u8; MAX_FRAME_SIZE];
    let mut decoder = FrameDecoder::new();

    // OTA_START
    let start_cmd =
        CommandPayload::new(cmd::OTA_START, &make_start_args(firmware.len() as u32, crc)).unwrap();
    let len = encode(&LinkMessage::Command(start_cmd.clone()), &mut buf).unwrap();
    decoder.feed(&buf[..len]).unwrap();
    match decoder.try_parse().unwrap().unwrap() {
        LinkMessage::Command(decoded) => {
            assert_eq!(decoded.command_id, start_cmd.command_id);
            assert_eq!(decoded.args, start_cmd.args);
            let ack = handler.handle(&decoded);
            assert!(ack.success, "OTA start should succeed");
        }
        _ => panic!("expected Command"),
    }

    // OTA_CHUNK (first 28 bytes)
    let chunk_cmd =
        CommandPayload::new(cmd::OTA_CHUNK, &make_chunk_args(0, &firmware[..28])).unwrap();
    let len = encode(&LinkMessage::Command(chunk_cmd.clone()), &mut buf).unwrap();
    decoder.feed(&buf[..len]).unwrap();
    match decoder.try_parse().unwrap().unwrap() {
        LinkMessage::Command(decoded) => {
            let ack = handler.handle(&decoded);
            assert!(ack.success, "OTA chunk 1 should succeed");
        }
        _ => panic!("expected Command"),
    }

    // OTA_CHUNK (remaining 7 bytes)
    let chunk_cmd =
        CommandPayload::new(cmd::OTA_CHUNK, &make_chunk_args(28, &firmware[28..])).unwrap();
    let len = encode(&LinkMessage::Command(chunk_cmd.clone()), &mut buf).unwrap();
    decoder.feed(&buf[..len]).unwrap();
    match decoder.try_parse().unwrap().unwrap() {
        LinkMessage::Command(decoded) => {
            let ack = handler.handle(&decoded);
            assert!(ack.success, "OTA chunk 2 should succeed");
        }
        _ => panic!("expected Command"),
    }

    // OTA_FINISH
    let finish_cmd = CommandPayload::new(cmd::OTA_FINISH, &[]).unwrap();
    let len = encode(&LinkMessage::Command(finish_cmd.clone()), &mut buf).unwrap();
    decoder.feed(&buf[..len]).unwrap();
    match decoder.try_parse().unwrap().unwrap() {
        LinkMessage::Command(decoded) => {
            let ack = handler.handle(&decoded);
            assert!(ack.success, "OTA finish should succeed with correct CRC");
        }
        _ => panic!("expected Command"),
    }
}

#[test]
fn node_manifest_reflects_registered_capabilities() {
    let (device, _host) = MockTcpTransport::pair().unwrap();
    let mut node = Node::new(
        device,
        *b"testnode",
        Version::new(0, 1, 0),
        Arch::Esp32C3RiscV,
    );

    node.register_capability(Capability::Ota).unwrap();
    node.register_capability(Capability::Serial).unwrap();
    node.register_capability(Capability::LittleFs(Box::new(MockLittleFsDriver::new())))
        .unwrap();

    assert!(node.has_ota());

    let manifest = node.manifest();
    assert_eq!(manifest.device_id, *b"testnode");
    assert_eq!(manifest.capabilities.len(), 3);
    assert!(manifest.capabilities.contains(&tpt_e_link::Capability::Ota));
    assert!(manifest
        .capabilities
        .contains(&tpt_e_link::Capability::Uart));
    assert!(manifest
        .capabilities
        .contains(&tpt_e_link::Capability::LittleFs));
}

#[test]
fn node_handshake_over_mock_transport() {
    let (device, mut host) = MockTcpTransport::pair().unwrap();
    let mut node = Node::new(
        device,
        *b"handshak",
        Version::new(0, 1, 0),
        Arch::Esp32C3RiscV,
    );
    node.register_capability(Capability::Ota).unwrap();
    node.register_capability(Capability::Serial).unwrap();

    std::thread::scope(|s| {
        let handle = s.spawn(move || pollster::block_on(node.run()));

        // Receive the handshake.
        let msg = pollster::block_on(read_frame(&mut host));
        match msg {
            LinkMessage::Handshake(m) => {
                assert_eq!(m.device_id, *b"handshak");
                assert_eq!(m.architecture, Arch::Esp32C3RiscV);
                assert!(m.capabilities.contains(&tpt_e_link::Capability::Ota));
                assert!(m.capabilities.contains(&tpt_e_link::Capability::Uart));
            }
            other => panic!("expected Handshake, got {other:?}"),
        }

        // Send a heartbeat; expect one back.
        pollster::block_on(write_frame(&mut host, &LinkMessage::Heartbeat));
        let msg = pollster::block_on(read_frame(&mut host));
        assert_eq!(msg, LinkMessage::Heartbeat);

        // Drop host so the device reactor exits with a transport error.
        drop(host);
        let result = handle.join().expect("thread panicked");
        assert!(result.is_err(), "expected transport error after host drop");
    });
}

#[test]
fn node_command_dispatch_over_mock_transport() {
    let (device, mut host) = MockTcpTransport::pair().unwrap();
    let mut node = Node::new(
        device,
        *b"cmd-test",
        Version::new(0, 1, 0),
        Arch::Esp32C3RiscV,
    );
    node.register_capability(Capability::LittleFs(Box::new(MockLittleFsDriver::new())))
        .unwrap();

    std::thread::scope(|s| {
        let handle = s.spawn(move || pollster::block_on(node.run()));

        // Skip the handshake.
        let _ = pollster::block_on(read_frame(&mut host));

        // Send a LittleFS command.
        let cmd = CommandPayload::new(cmd::LITTLEFS_READ, &[]).unwrap();
        pollster::block_on(write_frame(&mut host, &LinkMessage::Command(cmd)));

        // Expect an ack.
        let msg = pollster::block_on(read_frame(&mut host));
        match msg {
            LinkMessage::Ack(ack) => {
                assert!(ack.success);
                assert_eq!(ack.command_id, cmd::LITTLEFS_READ);
            }
            other => panic!("expected Ack, got {other:?}"),
        }

        // Send a telemetry command — no handler registered, expect failure ack.
        let cmd = CommandPayload::new(cmd::TELEMETRY_READ, &[]).unwrap();
        pollster::block_on(write_frame(&mut host, &LinkMessage::Command(cmd)));

        let msg = pollster::block_on(read_frame(&mut host));
        match msg {
            LinkMessage::Ack(ack) => {
                assert!(!ack.success);
                assert_eq!(ack.command_id, cmd::TELEMETRY_READ);
            }
            other => panic!("expected Ack, got {other:?}"),
        }

        drop(host);
        let _ = handle.join();
    });
}

#[test]
fn node_ota_single_chunk_through_reactor() {
    let (device, mut host) = MockTcpTransport::pair().unwrap();
    let mut node = Node::new(
        device,
        *b"ota-sngl",
        Version::new(0, 1, 0),
        Arch::Esp32Xtensa,
    );
    node.register_capability(Capability::Ota).unwrap();

    let firmware = b"short firmware image";
    let crc = {
        const CRC: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);
        CRC.checksum(firmware)
    };

    std::thread::scope(|s| {
        let handle = s.spawn(move || pollster::block_on(node.run()));

        // Skip handshake.
        let _ = pollster::block_on(read_frame(&mut host));

        // OTA start
        let start_cmd =
            CommandPayload::new(cmd::OTA_START, &make_start_args(firmware.len() as u32, crc))
                .unwrap();
        pollster::block_on(write_frame(&mut host, &LinkMessage::Command(start_cmd)));
        let msg = pollster::block_on(read_frame(&mut host));
        match msg {
            LinkMessage::Ack(ack) => assert!(ack.success),
            other => panic!("expected Ack, got {other:?}"),
        }

        // Single chunk with all firmware data
        let chunk_cmd = CommandPayload::new(cmd::OTA_CHUNK, &make_chunk_args(0, firmware)).unwrap();
        pollster::block_on(write_frame(&mut host, &LinkMessage::Command(chunk_cmd)));
        let msg = pollster::block_on(read_frame(&mut host));
        match msg {
            LinkMessage::Ack(ack) => assert!(ack.success),
            other => panic!("expected Ack, got {other:?}"),
        }

        // OTA finish
        let finish_cmd = CommandPayload::new(cmd::OTA_FINISH, &[]).unwrap();
        pollster::block_on(write_frame(&mut host, &LinkMessage::Command(finish_cmd)));
        let msg = pollster::block_on(read_frame(&mut host));
        match msg {
            LinkMessage::Ack(ack) => {
                assert!(ack.success, "OTA should succeed with correct CRC")
            }
            other => panic!("expected Ack, got {other:?}"),
        }

        drop(host);
        let _ = handle.join();
    });
}

#[test]
fn node_ota_flow_over_mock_transport() {
    let (device, mut host) = MockTcpTransport::pair().unwrap();
    let mut node = Node::new(
        device,
        *b"ota-test",
        Version::new(0, 1, 0),
        Arch::Esp32Xtensa,
    );
    node.register_capability(Capability::Ota).unwrap();

    let firmware = b"tpt-e-node test firmware image data";
    let crc = {
        const CRC: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);
        CRC.checksum(firmware)
    };

    std::thread::scope(|s| {
        let handle = s.spawn(move || pollster::block_on(node.run()));

        // Skip handshake.
        let _ = pollster::block_on(read_frame(&mut host));

        // OTA start
        let mut start_args: heapless::Vec<u8, 32> = heapless::Vec::new();
        start_args
            .extend_from_slice(&(firmware.len() as u32).to_le_bytes())
            .unwrap();
        start_args.extend_from_slice(&crc.to_le_bytes()).unwrap();
        let cmd = CommandPayload::new(cmd::OTA_START, &start_args).unwrap();
        pollster::block_on(write_frame(&mut host, &LinkMessage::Command(cmd)));

        let msg = pollster::block_on(read_frame(&mut host));
        match msg {
            LinkMessage::Ack(ack) => assert!(ack.success),
            other => panic!("expected Ack, got {other:?}"),
        }

        // OTA chunks (max 28 bytes per chunk due to 4-byte offset + 32-byte args)
        let mut offset = 0u32;
        let chunk_size = 28usize;
        while (offset as usize) < firmware.len() {
            let end = (offset as usize + chunk_size).min(firmware.len());
            let mut args: heapless::Vec<u8, 32> = heapless::Vec::new();
            args.extend_from_slice(&offset.to_le_bytes()).unwrap();
            args.extend_from_slice(&firmware[offset as usize..end])
                .unwrap();
            let cmd = CommandPayload::new(cmd::OTA_CHUNK, &args).unwrap();
            pollster::block_on(write_frame(&mut host, &LinkMessage::Command(cmd)));

            let msg = pollster::block_on(read_frame(&mut host));
            match msg {
                LinkMessage::Ack(ack) => assert!(ack.success),
                other => panic!("expected Ack, got {other:?}"),
            }
            offset = end as u32;
        }

        // OTA finish
        let cmd = CommandPayload::new(cmd::OTA_FINISH, &[]).unwrap();
        pollster::block_on(write_frame(&mut host, &LinkMessage::Command(cmd)));

        let msg = pollster::block_on(read_frame(&mut host));
        match msg {
            LinkMessage::Ack(ack) => assert!(ack.success, "OTA should succeed with correct CRC"),
            other => panic!("expected Ack, got {other:?}"),
        }

        drop(host);
        let _ = handle.join();
    });
}
