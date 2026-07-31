//! Capability registry and driver traits.
//!
//! The local [`Capability`] enum mirrors `tpt-basestation`'s `DeviceCapability`
//! vocabulary (`Serial`, `Ble`, `Ota`, `LittleFs`, `Flash`, `Debug`, `Probe`,
//! `Mesh`) plus `Telemetry`, but carries device-side driver trait objects where
//! appropriate. It maps to `tpt-e-link`'s wire [`Capability`] via
//! [`Capability::to_wire`].
//!
//! [`Capability`]: tpt_e_link::Capability

use alloc::boxed::Box;
use core::fmt;

use tpt_e_link::frame::{encode, FrameDecoder, MAGIC, MAX_FRAME_SIZE, MAX_PAYLOAD_SIZE};
use tpt_e_link::manifest::DeviceManifest;
use tpt_e_link::message::MAX_TELEMETRY_SAMPLES;
use tpt_e_link::message::{AckPayload, CommandPayload, LinkMessage, TelemetrySample};
use tpt_e_link::{Arch, Capability as WireCapability, LinkError, Version, MAX_CAPABILITIES};

use crate::error::NodeError;

/// Command-ID namespace for each capability. The high byte identifies the
/// capability; the low byte identifies the specific sub-command.
pub mod cmd {
    pub const LITTLEFS_BASE: u32 = 0x0100;
    pub const LITTLEFS_READ: u32 = 0x0100;
    pub const LITTLEFS_WRITE: u32 = 0x0101;
    pub const LITTLEFS_LIST: u32 = 0x0102;
    pub const LITTLEFS_FORMAT: u32 = 0x0103;

    pub const OTA_BASE: u32 = 0x0200;
    pub const OTA_START: u32 = 0x0200;
    pub const OTA_CHUNK: u32 = 0x0201;
    pub const OTA_FINISH: u32 = 0x0202;

    pub const TELEMETRY_BASE: u32 = 0x0300;
    pub const TELEMETRY_READ: u32 = 0x0300;

    pub const SERIAL_BASE: u32 = 0x0400;
    pub const BLE_BASE: u32 = 0x0500;
    pub const FLASH_BASE: u32 = 0x0600;
    pub const DEBUG_BASE: u32 = 0x0700;
    pub const MESH_BASE: u32 = 0x0800;
    pub const PROBE_BASE: u32 = 0x0900;
}

/// Trait for processing incoming [`CommandPayload`]s.
///
/// Implemented by every driver that carries state (e.g.
/// [`LittleFsDriver`], [`TelemetryDriver`]) and by the internal
/// [`crate::OtaHandler`].
pub trait CommandHandler {
    /// Returns `true` if this handler can service `command_id`.
    fn handles(&self, command_id: u32) -> bool;

    /// Execute the command and return an acknowledgement.
    fn handle(&mut self, cmd: &CommandPayload) -> AckPayload;
}

/// Driver trait for the `LittleFs` capability.
///
/// Implementors provide filesystem operations; the `CommandHandler` impl
/// dispatches `command_id`s in the [`cmd::LITTLEFS_BASE`] range to the
/// appropriate method.
pub trait LittleFsDriver: CommandHandler {
    /// Read up to `buf.len()` bytes from `path` starting at `offset`.
    /// Returns the number of bytes actually read.
    fn read_file(&mut self, path: &str, offset: u32, buf: &mut [u8]) -> Result<usize, NodeError>;

    /// Write `data` to `path` starting at `offset`.
    fn write_file(&mut self, path: &str, offset: u32, data: &[u8]) -> Result<(), NodeError>;

    /// List entries in the directory at `path`.
    fn list_dir(
        &mut self,
        path: &str,
    ) -> Result<heapless::Vec<heapless::String<64>, 32>, NodeError>;
}

/// Driver trait for the `Telemetry` capability.
pub trait TelemetryDriver: CommandHandler {
    /// Collect up to [`MAX_TELEMETRY_SAMPLES`] sensor readings.
    fn read_samples(&mut self) -> heapless::Vec<TelemetrySample, { MAX_TELEMETRY_SAMPLES }>;
}

/// The local capability enum, aligned with `tpt-basestation`'s
/// `DeviceCapability` vocabulary but carrying device-side driver instances.
///
/// Variants that carry a `Box<dyn Driver>` own a concrete driver; the
/// remaining variants are pure markers whose handling is either internal
/// (e.g. [`Capability::Ota`]) or not yet wired up.
pub enum Capability {
    /// LittleFS filesystem backed by a driver implementation.
    LittleFs(Box<dyn LittleFsDriver + Send>),
    /// Over-the-air update support. Handled internally by `tpt-node-core`
    /// once registered — the user does not supply a driver.
    Ota,
    /// BLE provisioning (advertising, GATT service for Wi-Fi credentials).
    Ble,
    /// Serial / UART transport.
    Serial,
    /// Raw flash read/write.
    Flash,
    /// Debug / probe interface.
    Debug,
    /// Mesh networking (ESP-NOW, 802.15.4, etc.).
    Mesh,
    /// Hardware probe / sniffer.
    Probe,
    /// Telemetry / sensor data backed by a driver implementation.
    Telemetry(Box<dyn TelemetryDriver + Send>),
}

impl fmt::Debug for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Capability::LittleFs(_) => write!(f, "Capability::LittleFs(<driver>)"),
            Capability::Ota => write!(f, "Capability::Ota"),
            Capability::Ble => write!(f, "Capability::Ble"),
            Capability::Serial => write!(f, "Capability::Serial"),
            Capability::Flash => write!(f, "Capability::Flash"),
            Capability::Debug => write!(f, "Capability::Debug"),
            Capability::Mesh => write!(f, "Capability::Mesh"),
            Capability::Probe => write!(f, "Capability::Probe"),
            Capability::Telemetry(_) => write!(f, "Capability::Telemetry(<driver>)"),
        }
    }
}

impl Capability {
    /// Maps this local capability to its wire-format representation in
    /// `tpt-e-link`'s [`Capability`] enum, if one exists.
    ///
    /// Some local capabilities (e.g. `Flash`, `Debug`, `Mesh`, `Probe`,
    /// `Telemetry`) have no direct wire representation — they are
    /// host-side concepts or are communicated via other message types.
    pub fn to_wire(&self) -> Option<WireCapability> {
        match self {
            Capability::LittleFs(_) => Some(WireCapability::LittleFs),
            Capability::Ota => Some(WireCapability::Ota),
            Capability::Ble => Some(WireCapability::BleProvision),
            Capability::Serial => Some(WireCapability::Uart),
            Capability::Flash => None,
            Capability::Debug => None,
            Capability::Mesh => None,
            Capability::Probe => None,
            Capability::Telemetry(_) => None,
        }
    }

    /// Returns `true` if this capability carries a command handler.
    pub fn has_handler(&self) -> bool {
        !matches!(self, Capability::Ota)
    }

    /// Extracts the command handler from this capability, if present.
    ///
    /// `Capability::Ota` is handled internally by [`crate::OtaHandler`] and
    /// does not return a handler here.
    pub fn into_handler(self) -> Option<Box<dyn CommandHandler + Send>> {
        match self {
            Capability::LittleFs(d) => Some(d),
            Capability::Telemetry(d) => Some(d),
            Capability::Ota
            | Capability::Ble
            | Capability::Serial
            | Capability::Flash
            | Capability::Debug
            | Capability::Mesh
            | Capability::Probe => None,
        }
    }
}

/// A single registered capability entry in the registry.
struct RegisteredCap {
    /// Wire-format capability (if any) to advertise in the manifest.
    wire: Option<WireCapability>,
    /// Command handler for this capability (if any).
    handler: Option<Box<dyn CommandHandler + Send>>,
}

/// Stores registered capabilities, builds the wire-format [`DeviceManifest`],
/// and dispatches incoming commands to the appropriate handler.
pub struct CapabilityRegistry {
    entries: heapless::Vec<RegisteredCap, MAX_CAPABILITIES>,
}

impl CapabilityRegistry {
    /// Creates an empty registry.
    pub const fn new() -> Self {
        Self {
            entries: heapless::Vec::new(),
        }
    }

    /// Registers a capability.
    ///
    /// Returns [`NodeError::RegistryFull`] if the maximum number of
    /// capabilities (`MAX_CAPABILITIES`) has been reached.
    pub fn register(&mut self, cap: Capability) -> Result<(), NodeError> {
        let (wire, handler) = (cap.to_wire(), cap.into_handler());
        self.entries
            .push(RegisteredCap { wire, handler })
            .map_err(|_| NodeError::RegistryFull)?;
        Ok(())
    }

    /// Returns the number of registered capabilities.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if no capabilities are registered.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns `true` if a capability with the given wire representation is
    /// registered.
    pub fn has_wire(&self, cap: WireCapability) -> bool {
        self.entries.iter().any(|e| e.wire == Some(cap))
    }

    /// Builds a [`DeviceManifest`] from the registered capabilities.
    ///
    /// Only capabilities with a wire representation are included.
    pub fn manifest(
        &self,
        device_id: [u8; 8],
        firmware_version: Version,
        architecture: Arch,
    ) -> DeviceManifest {
        let mut manifest = DeviceManifest::new(device_id, firmware_version, architecture);
        for entry in &self.entries {
            if let Some(wire) = &entry.wire {
                let _ = manifest.push_capability(*wire);
            }
        }
        manifest
    }

    /// Dispatches a command to the first registered handler that claims it.
    ///
    /// Returns `Some(AckPayload)` if a handler processed the command, or
    /// `None` if no handler was found.
    pub fn dispatch(&mut self, cmd: &CommandPayload) -> Option<AckPayload> {
        for entry in &mut self.entries {
            if let Some(handler) = &mut entry.handler {
                if handler.handles(cmd.command_id) {
                    return Some(handler.handle(cmd));
                }
            }
        }
        None
    }
}

impl Default for CapabilityRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Frame I/O helpers — shared by the reactor and tests.
// ---------------------------------------------------------------------------

/// Encodes `msg` as a complete wire frame and writes it to `transport`.
pub(crate) async fn send_frame<T: tpt_e_link::LinkTransport>(
    transport: &mut T,
    msg: &LinkMessage,
) -> Result<(), NodeError> {
    let mut buf = [0u8; MAX_FRAME_SIZE];
    let len = encode(msg, &mut buf).map_err(|_| NodeError::CodecError)?;
    transport.write_all(&buf[..len]).await?;
    Ok(())
}

/// Reads exactly one complete frame from `transport` and decodes it.
///
/// This implementation reads the fixed-size header first to learn the payload
/// length, then reads the remainder. It is suitable for chunk-oriented
/// transports (TCP, WebSocket) where each `read_exact` completes atomically.
/// Byte-oriented transports (UART, BLE) should instead use
/// [`FrameDecoder::feed_and_drain`] directly.
pub(crate) async fn recv_frame<T: tpt_e_link::LinkTransport>(
    transport: &mut T,
    decoder: &mut FrameDecoder,
) -> Result<LinkMessage, NodeError> {
    let mut header = [0u8; 5];
    transport.read_exact(&mut header).await?;

    if header[0] != MAGIC[0] || header[1] != MAGIC[1] {
        return Err(NodeError::Transport(LinkError::InvalidFrame));
    }

    let payload_len = u16::from_le_bytes([header[2], header[3]]) as usize;
    if payload_len > MAX_PAYLOAD_SIZE {
        return Err(NodeError::Transport(LinkError::InvalidFrame));
    }

    let rest_len = payload_len + 4;
    let mut rest = [0u8; MAX_FRAME_SIZE];
    transport.read_exact(&mut rest[..rest_len]).await?;

    decoder
        .feed(&header)
        .map_err(|_| NodeError::Transport(LinkError::BufferTooSmall))?;
    decoder
        .feed(&rest[..rest_len])
        .map_err(|_| NodeError::Transport(LinkError::BufferTooSmall))?;

    match decoder.try_parse() {
        Ok(Some(msg)) => Ok(msg),
        Ok(None) => Err(NodeError::Transport(LinkError::InvalidFrame)),
        Err(e) => Err(NodeError::Transport(e)),
    }
}
