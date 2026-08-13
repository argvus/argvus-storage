use std::collections::HashMap;

use zbus::Connection;
use zvariant::OwnedValue;
use zvariant::Value;

use crate::device::Device;

const SERVICE: &str = "org.freedesktop.UDisks2";
const MANAGER_PATH: &str = "/org/freedesktop/UDisks2";

const IFACE_BLOCK: &str = "org.freedesktop.UDisks2.Block";
const IFACE_FILESYSTEM: &str = "org.freedesktop.UDisks2.Filesystem";
const IFACE_ENCRYPTED: &str = "org.freedesktop.UDisks2.Encrypted";
const IFACE_PARTITION_TABLE: &str = "org.freedesktop.UDisks2.PartitionTable";
const IFACE_PARTITION: &str = "org.freedesktop.UDisks2.Partition";
const IFACE_LOOP: &str = "org.freedesktop.UDisks2.Loop";
const IFACE_DRIVE: &str = "org.freedesktop.UDisks2.Drive";

#[derive(Default)]
pub struct RawBlock {
    pub object_path: String,
    pub device: String,
    pub drive: String,
    pub id_label: String,
    pub id_type: String,
    pub hint_name: String,
    pub preferred_device: String,
    pub partition_type: String,
    pub size: u64,
    pub read_only: bool,
    pub hint_ignore: bool,
    pub mount_points: Vec<String>,
    pub has_filesystem: bool,
    pub has_encrypted: bool,
    pub is_partition_table: bool,
    pub is_loop: bool,
    pub is_cleartext: bool,
    pub cleartext: String,
    pub encryption_type: String,
}

#[derive(Default)]
pub struct RawDrive {
    pub object_path: String,
    pub vendor: String,
    pub model: String,
    pub connection_bus: String,
    pub removable: bool,
    pub ejectable: bool,
    pub media_removable: bool,
    pub optical: bool,
}

#[derive(Default)]
pub struct Enumerated {
    pub blocks: Vec<RawBlock>,
    pub drives: Vec<RawDrive>,
}

pub struct UdisksClient {
    connection: Option<Connection>,
    error: String,
}

impl UdisksClient {
    pub fn new() -> Self {
        UdisksClient {
            connection: None,
            error: String::new(),
        }
    }

    pub fn last_error(&self) -> &str {
        &self.error
    }

    pub fn connection(&self) -> Option<Connection> {
        self.connection.clone()
    }

    pub async fn connect(&mut self) -> bool {
        if self.connection.is_some() {
            return true;
        }
        match Connection::system().await {
            Ok(conn) => {
                self.connection = Some(conn);
                true
            }
            Err(e) => {
                self.error = e.to_string();
                false
            }
        }
    }

    pub async fn enumerate(&mut self) -> Enumerated {
        let mut out = Enumerated::default();
        let conn = match &self.connection {
            Some(c) => c.clone(),
            None => {
                self.error = "not connected".to_string();
                return out;
            }
        };
        let builder = match zbus::fdo::ObjectManagerProxy::builder(&conn).destination(SERVICE) {
            Ok(b) => b,
            Err(e) => {
                self.error = e.to_string();
                return out;
            }
        };
        let builder = match builder.path(MANAGER_PATH) {
            Ok(b) => b,
            Err(e) => {
                self.error = e.to_string();
                return out;
            }
        };
        let proxy = match builder.build().await {
            Ok(p) => p,
            Err(e) => {
                self.error = e.to_string();
                return out;
            }
        };
        let objects = match proxy.get_managed_objects().await {
            Ok(o) => o,
            Err(e) => {
                self.error = e.to_string();
                return out;
            }
        };

        for (obj_path, interfaces) in objects {
            let path = obj_path.to_string();
            let mut block: Option<RawBlock> = None;
            for (iface, props) in interfaces {
                let iface = iface.to_string();
                if iface == IFACE_DRIVE {
                    let mut d = RawDrive {
                        object_path: path.clone(),
                        ..RawDrive::default()
                    };
                    parse_drive(&props, &mut d);
                    out.drives.push(d);
                } else if iface == IFACE_BLOCK {
                    let b = block.get_or_insert_with(|| RawBlock {
                        object_path: path.clone(),
                        ..RawBlock::default()
                    });
                    parse_block(&props, b, BlockSource::Block);
                } else if iface == IFACE_FILESYSTEM {
                    let b = block.get_or_insert_with(|| RawBlock {
                        object_path: path.clone(),
                        ..RawBlock::default()
                    });
                    parse_block(&props, b, BlockSource::Filesystem);
                } else if iface == IFACE_ENCRYPTED {
                    let b = block.get_or_insert_with(|| RawBlock {
                        object_path: path.clone(),
                        ..RawBlock::default()
                    });
                    parse_block(&props, b, BlockSource::Encrypted);
                } else if iface == IFACE_PARTITION_TABLE {
                    block.get_or_insert_with(|| RawBlock {
                        object_path: path.clone(),
                        ..RawBlock::default()
                    })
                    .is_partition_table = true;
                } else if iface == IFACE_PARTITION {
                    if let Some(t) = prop_string(&props, "Type") {
                        block
                            .get_or_insert_with(|| RawBlock {
                                object_path: path.clone(),
                                ..RawBlock::default()
                            })
                            .partition_type = t;
                    }
                } else if iface == IFACE_LOOP {
                    block
                        .get_or_insert_with(|| RawBlock {
                            object_path: path.clone(),
                            ..RawBlock::default()
                        })
                        .is_loop = true;
                }
            }
            if let Some(b) = block {
                out.blocks.push(b);
            }
        }

        // Mark blocks that are the cleartext of an encrypted device so the
        // model can skip them (the LUKS container is the item shown in the bar).
        let cleartext_set: std::collections::HashSet<String> = out
            .blocks
            .iter()
            .filter(|b| !b.cleartext.is_empty() && b.cleartext != "/")
            .map(|b| b.cleartext.clone())
            .collect();
        for b in out.blocks.iter_mut() {
            if cleartext_set.contains(&b.object_path) {
                b.is_cleartext = true;
            }
        }
        out
    }

    async fn call_method(
        &self,
        object_path: &str,
        interface: &str,
        method: &str,
        options: &HashMap<String, Value<'static>>,
    ) -> Result<(), String> {
        let conn = self
            .connection
            .as_ref()
            .ok_or_else(|| "not connected to system bus".to_string())?;
        let body = (options.clone(),);
        conn.call_method(Some(SERVICE), object_path, Some(interface), method, &body)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub async fn mount(&self, device: &Device) -> Result<(), String> {
        let target = if !device.cleartext.is_empty() && device.cleartext != "/" {
            &device.cleartext
        } else {
            &device.object_path
        };
        self.call_method(target, IFACE_FILESYSTEM, "Mount", &no_user_interaction())
            .await
    }

    pub async fn unmount(&self, device: &Device) -> Result<(), String> {
        let target = if !device.cleartext.is_empty() && device.cleartext != "/" {
            &device.cleartext
        } else {
            &device.object_path
        };
        self.call_method(target, IFACE_FILESYSTEM, "Unmount", &no_user_interaction())
            .await
    }

    pub async fn eject(&self, device: &Device) -> Result<(), String> {
        if device.drive_path.is_empty() {
            return Err("device has no drive to eject".to_string());
        }
        self.call_method(&device.drive_path, IFACE_DRIVE, "Eject", &no_user_interaction())
            .await
    }

    pub async fn power_off(&self, device: &Device) -> Result<(), String> {
        if device.drive_path.is_empty() {
            return Err("device has no drive to power off".to_string());
        }
        self.call_method(&device.drive_path, IFACE_DRIVE, "PowerOff", &no_user_interaction())
            .await
    }

    pub async fn lock(&self, device: &Device) -> Result<(), String> {
        self.call_method(&device.object_path, IFACE_ENCRYPTED, "Lock", &empty_options())
            .await
    }
}

fn no_user_interaction() -> HashMap<String, Value<'static>> {
    let mut m = HashMap::new();
    m.insert("auth.no_user_interaction".to_string(), Value::Bool(true));
    m
}

fn empty_options() -> HashMap<String, Value<'static>> {
    HashMap::new()
}

fn prop_string(props: &HashMap<String, OwnedValue>, name: &str) -> Option<String> {
    let v = props.get(name)?;
    if let Ok(s) = <&str>::try_from(v) {
        return Some(s.to_string());
    }
    if let Ok(s) = <&zvariant::Str>::try_from(v) {
        return Some(s.to_string());
    }
    if let Ok(o) = <&zvariant::ObjectPath>::try_from(v) {
        return Some(o.to_string());
    }
    None
}

fn prop_bool(props: &HashMap<String, OwnedValue>, name: &str) -> Option<bool> {
    let v = props.get(name)?;
    <bool>::try_from(v).ok()
}

fn prop_u64(props: &HashMap<String, OwnedValue>, name: &str) -> Option<u64> {
    let v = props.get(name)?;
    <u64>::try_from(v).ok()
}

// `ay` byte arrays (Device, ...), trailing NULs stripped.
fn prop_bytes(props: &HashMap<String, OwnedValue>, name: &str) -> Option<Vec<u8>> {
    let v = props.get(name)?.try_clone().ok()?;
    let mut bytes: Vec<u8> = Vec::try_from(v).ok()?;
    while bytes.last() == Some(&0) {
        bytes.pop();
    }
    Some(bytes)
}

// `aay` (MountPoints), trailing NULs stripped.
fn prop_mount_points(props: &HashMap<String, OwnedValue>, name: &str) -> Option<Vec<String>> {
    let v = props.get(name)?.try_clone().ok()?;
    let arrays: Vec<Vec<u8>> = Vec::try_from(v).ok()?;
    let mut out = Vec::new();
    for mut bytes in arrays {
        while bytes.last() == Some(&0) {
            bytes.pop();
        }
        if !bytes.is_empty() {
            out.push(String::from_utf8_lossy(&bytes).into_owned());
        }
    }
    Some(out)
}

#[derive(Clone, Copy, PartialEq)]
enum BlockSource {
    Block,
    Filesystem,
    Encrypted,
}

fn parse_block(props: &HashMap<String, OwnedValue>, b: &mut RawBlock, source: BlockSource) {
    match source {
        BlockSource::Block => {
            if let Some(bytes) = prop_bytes(props, "Device") {
                b.device = String::from_utf8_lossy(&bytes).into_owned();
            }
            if let Some(v) = prop_string(props, "Drive") {
                b.drive = v;
            }
            if let Some(v) = prop_string(props, "IdLabel") {
                b.id_label = v;
            }
            if let Some(v) = prop_string(props, "IdType") {
                b.id_type = v;
            }
            if let Some(v) = prop_string(props, "HintName") {
                b.hint_name = v;
            }
            if let Some(v) = prop_string(props, "PreferredDevice") {
                b.preferred_device = v;
            }
            if let Some(v) = prop_string(props, "PartitionType") {
                b.partition_type = v;
            }
            if let Some(v) = prop_u64(props, "Size") {
                b.size = v;
            }
            if let Some(v) = prop_bool(props, "ReadOnly") {
                b.read_only = v;
            }
            if let Some(v) = prop_bool(props, "HintIgnore") {
                b.hint_ignore = v;
            }
            if let Some(v) = prop_string(props, "HintEncryptionType") {
                b.encryption_type = v;
            }
        }
        BlockSource::Filesystem => {
            if let Some(v) = prop_mount_points(props, "MountPoints") {
                b.mount_points = v;
            }
            b.has_filesystem = props.contains_key("MountPoints");
        }
        BlockSource::Encrypted => {
            if let Some(v) = prop_string(props, "CleartextDevice") {
                b.cleartext = v;
            }
            b.has_encrypted = true;
        }
    }
}

fn parse_drive(props: &HashMap<String, OwnedValue>, d: &mut RawDrive) {
    if let Some(v) = prop_string(props, "Vendor") {
        d.vendor = v;
    }
    if let Some(v) = prop_string(props, "Model") {
        d.model = v;
    }
    if let Some(v) = prop_string(props, "ConnectionBus") {
        d.connection_bus = v;
    }
    if let Some(v) = prop_bool(props, "Removable") {
        d.removable = v;
    }
    if let Some(v) = prop_bool(props, "Ejectable") {
        d.ejectable = v;
    }
    if let Some(v) = prop_bool(props, "MediaRemovable") {
        d.media_removable = v;
    }
    if let Some(v) = prop_bool(props, "Optical") {
        d.optical = v;
    }
}

// True when the message is a UDisks2 change signal we care about.
pub fn is_relevant_signal(msg: &zbus::Message) -> bool {
    let header = msg.header();
    let sender = header.sender().map(|s| s.to_string()).unwrap_or_default();
    if sender != SERVICE {
        return false;
    }
    let iface = header.interface().map(|s| s.to_string()).unwrap_or_default();
    let member = header.member().map(|s| s.to_string()).unwrap_or_default();
    matches!(
        (iface.as_str(), member.as_str()),
        ("org.freedesktop.DBus.Properties", "PropertiesChanged")
            | ("org.freedesktop.DBus.ObjectManager", "InterfacesAdded")
            | ("org.freedesktop.DBus.ObjectManager", "InterfacesRemoved")
    )
}
