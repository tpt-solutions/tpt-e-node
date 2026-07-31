//! End-to-end test: a real `tpt-node-mock` device talking to BaseStation's
//! WebSocket listener over the live `tpt-e-link` wire protocol.
//!
//! This is the Phase 3 "mock-first" integration — no hardware, no mocks
//! beyond the host-backed LittleFS driver.

use std::time::Duration;

use tpt_basestation_core::{start_ws_listener, WsRegistry};
use tpt_basestation_hal::{DeviceDiscovery, DiscoveryConfidence, ManifestStore};
use tpt_e_link::message::CommandPayload;
use tpt_e_link::{Arch, Version};
use tpt_node_core::{Capability, Node};
use tpt_node_mock::{HostLittleFsDriver, WsTransport};

#[tokio::test]
async fn mock_device_registers_and_serves_commands_over_ws() {
    let registry = WsRegistry::new();
    let handle = start_ws_listener("127.0.0.1:0".parse().unwrap(), registry.clone())
        .await
        .expect("start ws listener");
    let url = format!("ws://{}/ws", handle.addr);

    let fs_dir = tempfile::tempdir().expect("tempdir");

    let transport = WsTransport::connect(&url).expect("ws connect");
    let closer = transport
        .try_clone_stream()
        .expect("clone socket")
        .expect("plain ws:// connection should have a cloneable TCP socket");
    let mut node = Node::new(
        transport,
        *b"e2emock1",
        Version::new(0, 1, 0),
        Arch::Esp32C3RiscV,
    );
    node.register_capability(Capability::Ota).expect("register ota");
    node.register_capability(Capability::LittleFs(Box::new(HostLittleFsDriver::new(
        fs_dir.path().to_path_buf(),
    ))))
    .expect("register littlefs");
    node.register_capability(Capability::Serial).expect("register serial");

    // `WsTransport`'s read/write are `async fn`s that internally make
    // *blocking* tungstenite calls with no real `.await` point (same as
    // production `tpt-node-mock`, which drives them via `pollster::block_on`
    // on a dedicated thread — see `src/main.rs`). Spawning `node.run()` as an
    // ordinary `tokio::spawn` task would let it hog a runtime worker thread
    // on its blocking read, starving this test's own polling below. Route it
    // through `spawn_blocking` + `pollster::block_on` instead, matching how
    // the real binary runs it.
    let node_task =
        tokio::task::spawn_blocking(move || pollster::block_on(node.run()));

    // The reactor broadcasts its manifest immediately on boot — wait for the
    // listener to decode it and register the device.
    let device = wait_for(
        || registry.snapshot().into_iter().find(|d| d.device_id == "e2emock1"),
        "device manifest never registered",
    )
    .await;
    assert_eq!(device.firmware_version, "0.1.0");
    assert_eq!(device.architecture, "esp32_c3_riscv");
    for expected in ["ota", "little_fs", "serial"] {
        assert!(
            device.capabilities.iter().any(|c| c == expected),
            "missing capability {expected}: {:?}",
            device.capabilities
        );
    }

    // The hal discovery pipeline should surface the device as a normal
    // handshake-confirmed network device.
    let store = ManifestStore::load();
    let projected = DeviceDiscovery::new(store).discover_network(&registry.snapshot());
    let entry = projected
        .iter()
        .find(|d| d.port == url)
        .expect("device should appear in discovery");
    assert_eq!(entry.confidence, DiscoveryConfidence::Handshake);
    assert!(entry.capabilities.has(&tpt_basestation_hal::capability::DeviceCapability::LittleFs));

    // Drive a real command over the link: the mock LittleFS write command
    // (0x0101, args = [path_len u8][path][offset u32 LE][data]) should land in
    // the host-backed directory.
    let mut args = Vec::new();
    args.push(b"e2e.txt".len() as u8);
    args.extend_from_slice(b"e2e.txt");
    args.extend_from_slice(&0u32.to_le_bytes());
    args.extend_from_slice(b"hello ws");
    let cmd = CommandPayload::new(0x0101, &args).expect("build command");

    registry
        .send_command("e2emock1", &cmd)
        .expect("send_command should succeed");

    let written = wait_for(
        || {
            let path = fs_dir.path().join("e2e.txt");
            std::fs::read_to_string(&path)
                .ok()
                .filter(|contents| contents == "hello ws")
        },
        "LittleFS write command never reached the host-backed driver",
    )
    .await;
    assert_eq!(written, "hello ws");

    // Dropping the link deregisters the device. `node_task` is a
    // `spawn_blocking` task parked in a blocking socket read, so `abort()`
    // alone would not interrupt it — force the underlying TCP socket closed
    // instead, which makes that blocking read return an error and the
    // reactor loop exit.
    let _ = closer.shutdown(std::net::Shutdown::Both);
    node_task.abort();
    wait_for(
        || (!registry.contains("e2emock1")).then_some(()),
        "device should be removed from the registry after disconnect",
    )
    .await;
    handle.stop();
}

/// Poll `f` every 50ms until it returns `Some` (timeout 5s), then unwrap.
async fn wait_for<T>(mut f: impl FnMut() -> Option<T>, timeout_msg: &str) -> T {
    for _ in 0..100 {
        if let Some(value) = f() {
            return value;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timeout: {timeout_msg}");
}
