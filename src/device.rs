use std::cmp::Ordering;
use std::collections::HashMap;

use crate::config::Config;
use crate::udisks::{Enumerated, RawBlock, RawDrive};
use crate::util;

#[derive(Debug, Clone, Default)]
pub struct Device {
    pub object_path: String,
    pub drive_path: String,
    pub block: String,
    pub name: String,
    pub vendor: String,
    pub model: String,
    pub filesystem: String,
    pub mount_point: String,
    pub cleartext: String,
    pub capacity: u64,
    pub used: u64,
    pub free: u64,
    pub mounted: bool,
    pub read_only: bool,
    pub encrypted: bool,
    pub locked: bool,
    pub removable: bool,
    pub ejectable: bool,
    pub can_power_off: bool,
    pub is_optical: bool,
    pub hidden: bool,
    pub insertion_seq: u64,
    pub mount_time: i64,
}

fn basename(path: &str) -> &str {
    match path.rfind('/') {
        Some(slash) => &path[slash + 1..],
        None => path,
    }
}

// A partition that lives on the EFI system partition path is hidden by default.
fn is_efi_system(b: &RawBlock, mount_point: &str) -> bool {
    if !mount_point.is_empty() && mount_point.to_lowercase().contains("/boot/efi") {
        return true;
    }
    if b.id_type.eq_ignore_ascii_case("vfat") && b.partition_type.eq_ignore_ascii_case("0xef") {
        return true;
    }
    false
}

fn is_removable_drive(drive: Option<&RawDrive>) -> bool {
    match drive {
        Some(d) => d.removable || d.media_removable || d.ejectable || d.optical,
        None => false,
    }
}

pub fn build_devices(
    raw: &Enumerated,
    cfg: &Config,
    mount_time: &mut HashMap<String, i64>,
    insertion_seq: &mut HashMap<String, u64>,
    next_insertion: &mut u64,
) -> Vec<Device> {
    let mut drives: HashMap<String, &RawDrive> = HashMap::new();
    for d in &raw.drives {
        drives.insert(d.object_path.clone(), d);
    }

    let mut out: Vec<Device> = Vec::with_capacity(raw.blocks.len());

    for b in &raw.blocks {
        if b.is_partition_table || b.is_cleartext || b.is_loop || b.device.is_empty() {
            continue;
        }
        if !b.has_filesystem && !b.has_encrypted {
            continue;
        }
        let drive = if b.drive.is_empty() {
            None
        } else {
            drives.get(&b.drive).copied()
        };
        if !is_removable_drive(drive) {
            continue;
        }

        let mut d = Device {
            object_path: b.object_path.clone(),
            drive_path: b.drive.clone(),
            block: b.device.clone(),
            encrypted: b.has_encrypted,
            locked: b.has_encrypted && (b.cleartext.is_empty() || b.cleartext == "/"),
            cleartext: String::new(),
            ..Device::default()
        };
        if !d.locked {
            d.cleartext = b.cleartext.clone();
        }

        if let Some(mp) = b.mount_points.iter().find(|mp| !mp.is_empty()) {
            d.mount_point = mp.clone();
        }
        d.mounted = !d.mount_point.is_empty();

        if let Some(drv) = drive {
            d.vendor = util::trim(&drv.vendor).to_string();
            d.model = util::trim(&drv.model).to_string();
            d.removable = drv.removable || drv.media_removable;
            d.ejectable = drv.ejectable;
            d.is_optical = drv.optical;
        }
        d.can_power_off = !d.drive_path.is_empty() && (d.removable || d.ejectable);

        d.filesystem = b.id_type.clone();
        if d.encrypted && d.filesystem.is_empty() {
            d.filesystem = "crypto_LUKS".to_string();
        }

        d.read_only = b.read_only;
        if d.mounted {
            if let Some((total, used, free, ro)) = util::statvfs_usage(&d.mount_point) {
                d.capacity = total;
                d.used = used;
                d.free = free;
                d.read_only = d.read_only || ro;
            }
        } else {
            d.capacity = b.size;
        }

        d.hidden = b.hint_ignore || is_efi_system(b, &d.mount_point);
        if d.hidden && !cfg.show_hidden {
            continue;
        }

        if !b.id_label.is_empty() {
            d.name = b.id_label.clone();
        } else if !b.hint_name.is_empty() {
            d.name = b.hint_name.clone();
        } else if !d.vendor.is_empty() || !d.model.is_empty() {
            if !d.vendor.is_empty() && !d.model.is_empty() {
                d.name = format!("{} {}", d.vendor, d.model);
            } else if !d.vendor.is_empty() {
                d.name = d.vendor.clone();
            } else {
                d.name = d.model.clone();
            }
        } else {
            d.name = basename(&d.block).to_string();
        }

        let seq = insertion_seq.entry(d.object_path.clone()).or_insert_with(|| {
            let n = *next_insertion;
            *next_insertion += 1;
            n
        });
        d.insertion_seq = *seq;

        if d.mounted {
            let now = now_epoch();
            let mt = mount_time.entry(d.object_path.clone()).or_insert(0);
            if *mt <= 0 {
                *mt = now;
            }
            d.mount_time = *mt;
        } else {
            mount_time.insert(d.object_path.clone(), 0);
            d.mount_time = 0;
        }

        out.push(d);
    }

    let mode = cfg.sort.as_str();
    out.sort_by(|a, b| {
        match mode {
            "name" => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            "size" => {
                if a.capacity != b.capacity {
                    return b.capacity.cmp(&a.capacity);
                }
                a.insertion_seq.cmp(&b.insertion_seq)
            }
            "insertion" => a.insertion_seq.cmp(&b.insertion_seq),
            _ => {
                if a.mounted != b.mounted {
                    return if a.mounted {
                        Ordering::Less
                    } else {
                        Ordering::Greater
                    };
                }
                if a.mounted && b.mounted && a.mount_time != b.mount_time {
                    return b.mount_time.cmp(&a.mount_time);
                }
                a.insertion_seq.cmp(&b.insertion_seq)
            }
        }
    });

    out
}

fn now_epoch() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
