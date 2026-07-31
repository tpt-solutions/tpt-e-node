//! The async reactor / event loop.
//!
//! [`Node`] is the user-facing entry point: it owns a [`tpt_e_link::LinkTransport`],
//! a [`CapabilityRegistry`], an optional [`OtaHandler`], and a [`FrameDecoder`].
//! The [`Node::run`] method drives the main loop — broadcasting the manifest
//! on boot, then reading frames, decoding them, and dispatching commands.

use alloc::boxed::Box;

use tpt_e_link::frame::FrameDecoder;
use tpt_e_link::manifest::DeviceManifest;
use tpt_e_link::message::{AckPayload, CommandPayload, LinkMessage, TelemetryFrame};
use tpt_e_link::{Arch, LinkTransport, Version};

use crate::capability::{recv_frame, send_frame, Capability, CapabilityRegistry, CommandHandler};
use crate::error::NodeError;
use crate::ota::OtaHandler;

/// The main device node: combines transport, capability registry, and OTA
/// handler into a single async reactor.
///
/// On embedded targets this is spawned as an `embassy-executor` task; in tests
/// it is driven by `pollster::block_on`.
pub struct Node<T: LinkTransport> {
    transport: T,
    registry: CapabilityRegistry,
    ota: Option<OtaHandler>,
    decoder: FrameDecoder,
    device_id: [u8; 8],
    firmware_version: Version,
    architecture: Arch,
}

impl<T: LinkTransport> Node<T> {
    /// Creates a new node with the given transport and device metadata.
    ///
    /// No capabilities are registered yet; call [`register_capability`](Self::register_capability)
    /// before [`run`](Self::run).
    pub fn new(
        transport: T,
        device_id: [u8; 8],
        firmware_version: Version,
        architecture: Arch,
    ) -> Self {
        Self {
            transport,
            registry: CapabilityRegistry::new(),
            ota: None,
            decoder: FrameDecoder::new(),
            device_id,
            firmware_version,
            architecture,
        }
    }

    /// Registers a capability with the node.
    ///
    /// If [`Capability::Ota`] is registered, an internal [`OtaHandler`] is
    /// created automatically — the user does not supply a driver.
    ///
    /// Returns [`NodeError::RegistryFull`] if the maximum number of
    /// capabilities has been reached.
    pub fn register_capability(&mut self, cap: Capability) -> Result<(), NodeError> {
        if matches!(cap, Capability::Ota) && self.ota.is_none() {
            self.ota = Some(OtaHandler::new(Box::new(crate::ota::RamFlashWriter::new())));
        }
        self.registry.register(cap)
    }

    /// Builds the wire-format [`DeviceManifest`] from registered capabilities.
    pub fn manifest(&self) -> DeviceManifest {
        self.registry
            .manifest(self.device_id, self.firmware_version, self.architecture)
    }

    /// Returns `true` if the node has an OTA handler (i.e. `Capability::Ota`
    /// was registered).
    pub fn has_ota(&self) -> bool {
        self.ota.is_some()
    }

    /// Sends a telemetry frame to the host.
    pub async fn send_telemetry(&mut self, frame: &TelemetryFrame) -> Result<(), NodeError> {
        send_frame(&mut self.transport, &LinkMessage::Telemetry(frame.clone())).await
    }

    /// Sends a log line to the host.
    pub async fn send_log(&mut self, log: &tpt_e_link::LogLine) -> Result<(), NodeError> {
        send_frame(&mut self.transport, &LinkMessage::Log(log.clone())).await
    }

    /// Broadcasts the manifest, then enters the main event loop.
    ///
    /// The loop runs forever (or until the transport errors):
    /// 1. Sends a `Handshake` with the manifest on boot.
    /// 2. Reads frames, decodes them, and dispatches:
    ///    - `Command` → routed to the OTA handler or capability registry.
    ///    - `Heartbeat` → echoed back.
    ///    - Other message types from the host are ignored.
    pub async fn run(&mut self) -> Result<(), NodeError> {
        let manifest = self.manifest();
        send_frame(&mut self.transport, &LinkMessage::Handshake(manifest)).await?;

        loop {
            let msg = recv_frame(&mut self.transport, &mut self.decoder).await?;
            match msg {
                LinkMessage::Command(cmd) => {
                    let ack = self.dispatch_command(&cmd).await;
                    send_frame(&mut self.transport, &LinkMessage::Ack(ack)).await?;
                }
                LinkMessage::Heartbeat => {
                    send_frame(&mut self.transport, &LinkMessage::Heartbeat).await?;
                }
                LinkMessage::Handshake(_)
                | LinkMessage::Telemetry(_)
                | LinkMessage::Log(_)
                | LinkMessage::Ack(_) => {
                    // These message types are not expected from the host.
                }
            }
        }
    }

    /// Routes a command to the appropriate handler.
    ///
    /// The OTA handler is checked first (if active), then the capability
    /// registry. If no handler claims the command, a failure ack is returned.
    async fn dispatch_command(&mut self, cmd: &CommandPayload) -> AckPayload {
        if let Some(ota) = &mut self.ota {
            if ota.handles(cmd.command_id) {
                return ota.handle(cmd);
            }
        }

        if let Some(ack) = self.registry.dispatch(cmd) {
            return ack;
        }

        AckPayload {
            command_id: cmd.command_id,
            success: false,
            error_code: 0,
        }
    }
}
