use std::collections::HashMap;

use crate::config::Config;
use crate::device::{build_devices, Device};
use crate::udisks::UdisksClient;
use crate::util;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    Mount,
    Unmount,
    Eject,
    PowerOff,
    Lock,
    Unlock,
    Open,
    Copy,
}

pub struct Actions<'a> {
    config: &'a Config,
}

impl<'a> Actions<'a> {
    pub fn new(config: &'a Config) -> Self {
        Actions { config }
    }

    async fn mount(&self, d: &Device) -> Result<(), String> {
        if d.encrypted && d.locked {
            return Err("device is locked; unlock it first".to_string());
        }
        let mut client = UdisksClient::new();
        if !client.connect().await {
            return Err(client.last_error().to_string());
        }
        client.mount(d).await
    }

    async fn unmount(&self, d: &Device) -> Result<(), String> {
        if !d.mounted {
            return Err("device is not mounted".to_string());
        }
        let mut client = UdisksClient::new();
        if !client.connect().await {
            return Err(client.last_error().to_string());
        }
        client.unmount(d).await
    }

    async fn eject(&self, d: &Device) -> Result<(), String> {
        if !d.ejectable {
            return Err("device is not ejectable".to_string());
        }
        let mut client = UdisksClient::new();
        if !client.connect().await {
            return Err(client.last_error().to_string());
        }
        if d.mounted {
            client.unmount(d).await?;
        }
        client.eject(d).await
    }

    async fn power_off(&self, d: &Device) -> Result<(), String> {
        if !d.can_power_off {
            return Err("device cannot be powered off".to_string());
        }
        let mut client = UdisksClient::new();
        if !client.connect().await {
            return Err(client.last_error().to_string());
        }
        client.power_off(d).await
    }

    async fn lock(&self, d: &Device) -> Result<(), String> {
        if !d.encrypted || d.locked {
            return Err("device is not an unlocked encrypted volume".to_string());
        }
        let mut client = UdisksClient::new();
        if !client.connect().await {
            return Err(client.last_error().to_string());
        }
        client.lock(d).await
    }

    async fn unlock(&self, d: &Device) -> Result<(), String> {
        if !d.encrypted || !d.locked {
            return Err("device is not a locked encrypted volume".to_string());
        }
        // The passphrase goes through an interactive terminal, then udisksctl
        // does the actual unlock.
        let cmd = format!(
            "{} udisksctl unlock --block-device {}",
            self.config.unlock_command,
            util::shell_quote(&d.block)
        );
        if util::fork_exec(&cmd) < 0 {
            return Err("failed to launch unlock terminal".to_string());
        }
        Ok(())
    }

    async fn open(&self, d: &Device) -> Result<(), String> {
        let target = self.resolve_mount(d).await?;
        let cmd = format!(
            "{} {}",
            self.config.file_manager_command,
            util::shell_quote(&target)
        );
        if util::fork_exec(&cmd) < 0 {
            return Err(format!(
                "failed to launch {}",
                self.config.file_manager_command
            ));
        }
        Ok(())
    }

    async fn copy(&self, d: &Device) -> Result<(), String> {
        let target = self.resolve_mount(d).await?;
        let mut clipboard = arboard::Clipboard::new()
            .map_err(|e| format!("clipboard unavailable: {}", e))?;
        clipboard
            .set_text(target)
            .map_err(|e| format!("failed to set clipboard: {}", e))
    }

    // Resolve the mount point, mounting first if needed.
    async fn resolve_mount(&self, d: &Device) -> Result<String, String> {
        if !d.mount_point.is_empty() {
            return Ok(d.mount_point.clone());
        }
        self.mount(d).await?;
        if let Some(fresh) = find_fresh(d, self.config).await
            && !fresh.mount_point.is_empty()
        {
            return Ok(fresh.mount_point);
        }
        Err("device mounted but mount point could not be resolved".to_string())
    }

    pub async fn perform(&self, kind: ActionKind, d: &Device) -> i32 {
        let err;
        let result: Result<(), String> = match kind {
            ActionKind::Mount => {
                if d.mounted {
                    return 2;
                }
                self.mount(d).await
            }
            ActionKind::Unmount => {
                if !d.mounted {
                    return 2;
                }
                self.unmount(d).await
            }
            ActionKind::Eject => {
                if !d.ejectable {
                    return 2;
                }
                self.eject(d).await
            }
            ActionKind::PowerOff => {
                if !d.can_power_off {
                    return 2;
                }
                self.power_off(d).await
            }
            ActionKind::Lock => {
                if !d.encrypted || d.locked {
                    return 2;
                }
                self.lock(d).await
            }
            ActionKind::Unlock => {
                if !d.encrypted || !d.locked {
                    return 2;
                }
                self.unlock(d).await
            }
            ActionKind::Open => self.open(d).await,
            ActionKind::Copy => self.copy(d).await,
        };
        if let Err(e) = result {
            err = e;
            util::notify("argvus-storage", &err, true);
            eprintln!("argvus-storage: {}", err);
            return 1;
        }
        0
    }
}

// Re-enumerate UDisks2 after a mount and return the fresh copy of `d` so the
// caller can read its actual mount point.
async fn find_fresh(d: &Device, cfg: &Config) -> Option<Device> {
    let mut client = UdisksClient::new();
    if !client.connect().await {
        return None;
    }
    let raw = client.enumerate().await;
    let mut mt = HashMap::new();
    let mut seq = HashMap::new();
    let mut next = 0u64;
    let devs = build_devices(&raw, cfg, &mut mt, &mut seq, &mut next);
    devs.into_iter().find(|x| x.object_path == d.object_path)
}
