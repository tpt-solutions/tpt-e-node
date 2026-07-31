//! `tpt-node-mock` — run a real TPT node on a laptop over WebSocket.

use std::path::PathBuf;
use std::time::Duration;

use tpt_e_link::{Arch, Version};
use tpt_node_core::{Capability, Node};
use tpt_node_mock::{HostLittleFsDriver, MockTelemetryDriver, WsTransport};

fn main() {
    let url = std::env::var("TPT_WS_URL").unwrap_or_else(|_| "ws://127.0.0.1:8375/ws".to_string());
    let fs_root = std::env::var("TPT_MOCK_FS").unwrap_or_else(|_| "mock-fs".to_string());
    let device_id = std::env::var("TPT_MOCK_DEVICE_ID").unwrap_or_else(|_| "tptmock1".to_string());

    if device_id.len() != 8 {
        eprintln!(
            "TPT_MOCK_DEVICE_ID must be exactly 8 bytes, got {:?}",
            device_id
        );
        std::process::exit(2);
    }
    let root = PathBuf::from(&fs_root);
    if let Err(e) = std::fs::create_dir_all(&root) {
        eprintln!("failed to create mock FS root {fs_root:?}: {e}");
        std::process::exit(2);
    }

    println!("[tpt-node-mock] device_id={device_id} fs_root={fs_root} connecting to {url}");

    loop {
        match pollster::block_on(run_once(&url, &root, &device_id)) {
            Ok(()) => eprintln!("[tpt-node-mock] disconnected cleanly"),
            Err(e) => eprintln!("[tpt-node-mock] session ended: {e}"),
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

async fn run_once(
    url: &str,
    root: &PathBuf,
    device_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let transport = WsTransport::connect(url)?;
    println!("[tpt-node-mock] connected");

    let mut id = [0u8; 8];
    id.copy_from_slice(device_id.as_bytes());

    let mut node = Node::new(transport, id, Version::new(0, 1, 0), Arch::Esp32C3RiscV);
    node.register_capability(Capability::Ota)?;
    node.register_capability(Capability::LittleFs(Box::new(
        HostLittleFsDriver::new(root.clone()),
    )))?;
    node.register_capability(Capability::Telemetry(Box::new(MockTelemetryDriver)))?;
    node.register_capability(Capability::Serial)?;
    println!(
        "[tpt-node-mock] registered capabilities: ota, little_fs ({}), telemetry, serial",
        root.display()
    );

    node.run().await?;
    Ok(())
}
