use crate::config::Config;
use crate::device::Device;
use crate::i18n;
use crate::util;

pub struct RenderResult {
    pub json: String,
    pub text: String,
    pub tooltip: String,
    pub cls: String,
    pub alt: String,
}

pub struct Renderer<'a> {
    config: &'a Config,
}

impl<'a> Renderer<'a> {
    pub fn new(config: &'a Config) -> Self {
        Renderer { config }
    }

    fn icon_for(&self, d: &Device) -> &str {
        if d.encrypted && d.locked {
            return &self.config.icons_encrypted;
        }
        if d.mounted && d.read_only {
            return &self.config.icons_read_only;
        }
        if d.mounted {
            return &self.config.icons_mounted;
        }
        if d.read_only {
            return &self.config.icons_read_only;
        }
        &self.config.icons_unmounted
    }

    fn tooltip_for(&self, d: &Device) -> String {
        let mut tip = self.config.tooltip_format.clone();
        replace_all(&mut tip, "{icon}", self.icon_for(d));
        expand_tokens(&mut tip, d);

        // Drop empty lines (e.g. no mount point) so the tooltip stays tidy.
        let mut out = String::new();
        for line in tip.split('\n') {
            let line = util::trim(line);
            if line.is_empty() {
                continue;
            }
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(line);
        }
        out
    }

    fn classes_for(&self, visible: &[Device]) -> Vec<String> {
        let mut classes = vec!["storage".to_string()];
        if visible.is_empty() {
            classes.push("empty".to_string());
            return classes;
        }
        let mut any_mounted = false;
        let mut any_readonly = false;
        let mut any_encrypted = false;
        for d in visible {
            any_mounted = any_mounted || d.mounted;
            any_readonly = any_readonly || d.read_only;
            any_encrypted = any_encrypted || (d.encrypted && d.locked);
        }
        if any_mounted {
            classes.push("mounted".to_string());
        }
        if any_readonly {
            classes.push("readonly".to_string());
        }
        if any_encrypted {
            classes.push("encrypted".to_string());
        }
        if !any_mounted {
            classes.push("unmounted".to_string());
        }
        classes
    }

    pub fn render(&self, devices: &[Device]) -> RenderResult {
        let mut result = RenderResult {
            json: String::new(),
            text: String::new(),
            tooltip: String::new(),
            cls: "storage".to_string(),
            alt: String::new(),
        };

        let mut visible: Vec<Device> = devices.to_vec();
        let mut overflow = 0usize;
        if self.config.max_devices > 0 && visible.len() as i32 > self.config.max_devices {
            overflow = visible.len() - self.config.max_devices as usize;
            visible.truncate(self.config.max_devices as usize);
        }

        if visible.is_empty() {
            result.alt = "storage-empty".to_string();
            result.tooltip.clear();
            if self.config.hide_when_empty {
                result.text.clear();
            } else {
                result.text = self.config.icons_unmounted.clone();
            }
            result.cls = "storage empty".to_string();
        } else {
            result.text = self.config.icons_unmounted.clone();

            result
                .tooltip
                .push_str(i18n::tr("Removable devices", "Dispositivos removíveis"));
            for d in &visible {
                result.tooltip.push('\n');
                result.tooltip.push_str("• ");
                result
                    .tooltip
                    .push_str(&self.tooltip_for(d).replace('\n', " · "));
            }
            if overflow > 0 {
                result.tooltip.push('\n');
                result.tooltip.push('+');
                result.tooltip.push_str(&overflow.to_string());
                result.tooltip.push_str(i18n::tr(" more", " mais"));
            }
            let any_mounted = visible.iter().any(|d| d.mounted);
            result.alt = if any_mounted {
                "storage-mounted".to_string()
            } else {
                "storage-unmounted".to_string()
            };
            result.cls = if any_mounted {
                "storage mounted".to_string()
            } else {
                "storage unmounted".to_string()
            };
        }

        let classes = self.classes_for(&visible);
        let mut j = serde_json::Map::new();
        j.insert(
            "text".to_string(),
            serde_json::Value::String(result.text.clone()),
        );
        if !result.tooltip.is_empty() {
            j.insert(
                "tooltip".to_string(),
                serde_json::Value::String(result.tooltip.clone()),
            );
        }
        j.insert(
            "alt".to_string(),
            serde_json::Value::String(result.alt.clone()),
        );
        if classes.len() > 1 {
            j.insert(
                "class".to_string(),
                serde_json::Value::Array(
                    classes.into_iter().map(serde_json::Value::String).collect(),
                ),
            );
        } else {
            j.insert(
                "class".to_string(),
                serde_json::Value::String(classes[0].clone()),
            );
        }
        result.json = serde_json::Value::Object(j).to_string();
        result
    }
}

fn replace_all(haystack: &mut String, needle: &str, replacement: &str) {
    let mut pos = 0;
    while let Some(rel) = haystack[pos..].find(needle) {
        let idx = pos + rel;
        haystack.replace_range(idx..idx + needle.len(), replacement);
        pos = idx + replacement.len();
    }
}

fn expand_tokens(out: &mut String, d: &Device) {
    let used = util::human_bytes(d.used);
    let free = util::human_bytes(d.free);
    let capacity = util::human_bytes(d.capacity);

    let mut state = String::new();
    if d.encrypted && d.locked {
        state.push_str(i18n::tr("Encrypted · Locked", "Criptografado · Bloqueado"));
    } else if d.encrypted {
        state.push_str(i18n::tr("Encrypted", "Criptografado"));
    }
    if d.read_only {
        if !state.is_empty() {
            state.push_str(" · ");
        }
        state.push_str(i18n::tr("Read-only", "Somente leitura"));
    }

    let tokens: [(&str, &str); 10] = [
        ("{name}", &d.name),
        ("{capacity}", &capacity),
        ("{used}", &used),
        ("{free}", &free),
        ("{fs}", &d.filesystem),
        ("{block}", &d.block),
        ("{mount}", &d.mount_point),
        ("{vendor}", &d.vendor),
        ("{model}", &d.model),
        ("{state}", &state),
    ];
    for (key, value) in tokens {
        replace_all(out, key, value);
    }
}
