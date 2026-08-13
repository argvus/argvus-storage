use crate::actions::{ActionKind, Actions};
use crate::config::Config;
use crate::device::Device;
use crate::util;

pub struct MenuEntry {
    pub kind: ActionKind,
    pub device_index: usize,
    pub label: String,
}

pub struct Menu<'a> {
    config: &'a Config,
}

impl<'a> Menu<'a> {
    pub fn new(config: &'a Config) -> Self {
        Menu { config }
    }

    pub async fn run(&self, devices: &[Device]) -> i32 {
        let mut entries: Vec<MenuEntry> = Vec::new();
        for (i, d) in devices.iter().enumerate() {
            let prefix = format!("{}  ·  ", d.name);
            let can_mount = !(d.mounted || d.encrypted && d.locked);
            if can_mount {
                entries.push(MenuEntry {
                    kind: ActionKind::Open,
                    device_index: i,
                    label: format!("{}Open", prefix),
                });
                entries.push(MenuEntry {
                    kind: ActionKind::Mount,
                    device_index: i,
                    label: format!("{}Mount", prefix),
                });
            }
            if d.mounted {
                entries.push(MenuEntry {
                    kind: ActionKind::Unmount,
                    device_index: i,
                    label: format!("{}Unmount", prefix),
                });
            }
            if d.encrypted && d.locked {
                entries.push(MenuEntry {
                    kind: ActionKind::Unlock,
                    device_index: i,
                    label: format!("{}Unlock", prefix),
                });
            }
            if d.encrypted && !d.locked && d.mounted {
                entries.push(MenuEntry {
                    kind: ActionKind::Lock,
                    device_index: i,
                    label: format!("{}Lock", prefix),
                });
            }
            if d.ejectable {
                entries.push(MenuEntry {
                    kind: ActionKind::Eject,
                    device_index: i,
                    label: format!("{}Eject", prefix),
                });
            }
            if d.can_power_off {
                entries.push(MenuEntry {
                    kind: ActionKind::PowerOff,
                    device_index: i,
                    label: format!("{}Power Off", prefix),
                });
            }
            if d.mounted {
                entries.push(MenuEntry {
                    kind: ActionKind::Copy,
                    device_index: i,
                    label: format!("{}Copy path", prefix),
                });
            }
        }

        if entries.is_empty() {
            util::notify("argvus-storage", "No removable storage devices", false);
            return 1;
        }

        let lines: Vec<String> = entries.iter().map(|e| e.label.clone()).collect();
        let mut cmd = self.config.menu.clone();
        if !self.config.menu_flags.is_empty() {
            cmd.push(' ');
            cmd.push_str(&self.config.menu_flags);
        }
        let Some((output, _error)) = util::run_capture(&cmd, &lines) else {
            // Cancel (dmenu returns non-zero when nothing is selected).
            return 1;
        };
        let selection = util::trim(&output);
        if selection.is_empty() {
            return 1;
        }

        for e in &entries {
            if e.label == selection {
                let actions = Actions::new(self.config);
                return actions.perform(e.kind, &devices[e.device_index]).await;
            }
        }
        1
    }

    pub async fn run_devices(&self, devices: &[Device]) -> i32 {
        if devices.is_empty() {
            util::notify("argvus-storage", "No removable storage devices", false);
            return 1;
        }

        let entries: Vec<String> = devices.iter().map(device_label).collect();
        let mut cmd = self.config.menu.clone();
        if !self.config.menu_flags.is_empty() {
            cmd.push(' ');
            cmd.push_str(&self.config.menu_flags);
        }
        let Some((output, _error)) = util::run_capture(&cmd, &entries) else {
            return 1;
        };
        let selection = util::trim(&output);
        if selection.is_empty() {
            return 1;
        }

        for (i, entry) in entries.iter().enumerate() {
            if entry == selection {
                let actions = Actions::new(self.config);
                return actions.perform(ActionKind::Open, &devices[i]).await;
            }
        }
        1
    }
}

fn device_label(d: &Device) -> String {
    let state = if d.encrypted && d.locked {
        "locked"
    } else if d.mounted {
        "mounted"
    } else {
        "available"
    };

    let mut details: Vec<String> = Vec::new();
    if !d.filesystem.is_empty() {
        details.push(d.filesystem.clone());
    }
    if d.capacity > 0 {
        details.push(util::human_bytes(d.capacity));
    }
    details.push(state.to_string());
    if !d.mount_point.is_empty() {
        details.push(d.mount_point.clone());
    }

    format!("{}  ·  {}", d.name, details.join(" · "))
}
