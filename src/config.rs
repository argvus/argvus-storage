const SYSTEM_CONFIG: &str = "/etc/argvus-storage/config.json";
const SYSTEM_THEME: &str = "/etc/argvus-storage/theme.css";
#[derive(Clone)]
pub struct Config {
    pub show_name: bool,
    pub show_capacity: bool,
    pub hide_when_empty: bool,
    pub show_hidden: bool,
    pub max_devices: i32,
    pub sort: String,
    pub separator: String,
    pub format: String,
    pub tooltip_format: String,
    pub open_command: String,
    pub file_manager_command: String,
    #[allow(dead_code)]
    pub copy_command: String,
    pub unlock_command: String,
    pub mode: String,
    pub menu: String,
    pub menu_flags: String,
    pub icons_mounted: String,
    pub icons_unmounted: String,
    pub icons_encrypted: String,
    pub icons_read_only: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            show_name: false,
            show_capacity: false,
            hide_when_empty: true,
            show_hidden: false,
            max_devices: 0,
            sort: "mount_time".to_string(),
            separator: "  ".to_string(),
            format: "{icon}".to_string(),
            tooltip_format: "{name}\n{state}\n{fs} · {capacity}\n{used} used · {free} free\n{mount}"
                .to_string(),
            open_command: "xdg-open".to_string(),
            file_manager_command: "xdg-open".to_string(),
            copy_command: "wl-copy".to_string(),
            unlock_command: "kitty -e".to_string(),
            mode: "rofi".to_string(),
            menu: "rofi".to_string(),
            menu_flags: "-dmenu -i -p Storage".to_string(),
            icons_mounted: "\u{f0a0}".to_string(),
            icons_unmounted: "\u{f0a0}".to_string(),
            icons_encrypted: "\u{f023}".to_string(),
            icons_read_only: "\u{f023}".to_string(),
        }
    }
}

impl Config {
    // Load config merging (in order): system defaults, user config
    // ($XDG_CONFIG_HOME/argvus-storage/config.json), explicit
    // --config path. Missing files are ignored; invalid JSON is skipped with a
    // warning.
    pub fn load(explicit_path: &str) -> Config {
        let mut cfg = Config::default();
        let mut paths: Vec<String> = Vec::new();
        paths.push(SYSTEM_CONFIG.to_string());
        if let Some(dir) = user_config_dir() {
            paths.push(dir + "/config.json");
        }
        if !explicit_path.is_empty() {
            paths.push(explicit_path.to_string());
        }
        for path in &paths {
            if let Some(json) = load_json(path) {
                apply_json(&mut cfg, &json);
            }
        }
        cfg
    }

    // Candidate theme.css paths, in the same precedence order as config.json
    // (system defaults, user override, then a theme.css next to an explicit
    // --config file). Callers load whichever of these exist, later paths
    // overriding earlier ones. theme.css always lives alongside the
    // corresponding config.json.
    pub fn theme_css_paths(explicit_config_path: &str) -> Vec<String> {
        let mut paths: Vec<String> = Vec::new();
        paths.push(SYSTEM_THEME.to_string());
        if let Some(dir) = user_config_dir() {
            paths.push(dir + "/theme.css");
        }
        if !explicit_config_path.is_empty()
            && let Some(dir) = std::path::Path::new(explicit_config_path).parent()
            && let Some(dir_str) = dir.to_str()
            && !dir_str.is_empty()
        {
            paths.push(format!("{}/theme.css", dir_str));
        }
        paths
    }
}

fn load_json(path: &str) -> Option<serde_json::Value> {
    let content = std::fs::read_to_string(path).ok()?;
    match serde_json::from_str(&strip_comments(&content)) {
        Ok(v) => Some(v),
        Err(e) => {
            eprintln!("argvus-storage: ignoring invalid config {}: {}", path, e);
            None
        }
    }
}

fn apply_json(cfg: &mut Config, j: &serde_json::Value) {
    let obj = match j.as_object() {
        Some(o) => o,
        None => return,
    };
    macro_rules! get_bool {
        ($name:literal) => {
            obj.get($name).and_then(|v| v.as_bool())
        };
    }
    macro_rules! get_str {
        ($name:literal) => {
            obj.get($name).and_then(|v| v.as_str()).map(String::from)
        };
    }
    if let Some(v) = get_bool!("show_name") {
        cfg.show_name = v;
    }
    if let Some(v) = get_bool!("show_capacity") {
        cfg.show_capacity = v;
    }
    if let Some(v) = get_bool!("hide_when_empty") {
        cfg.hide_when_empty = v;
    }
    if let Some(v) = get_bool!("show_hidden") {
        cfg.show_hidden = v;
    }
    if let Some(v) = obj.get("max_devices").and_then(|v| v.as_i64()) {
        cfg.max_devices = v as i32;
    }
    if let Some(v) = get_str!("sort") {
        cfg.sort = v;
    }
    if let Some(v) = get_str!("separator") {
        cfg.separator = v;
    }
    if let Some(v) = get_str!("format") {
        cfg.format = v;
    }
    if let Some(v) = get_str!("tooltip_format") {
        cfg.tooltip_format = v;
    }
    if let Some(v) = get_str!("open_command") {
        cfg.open_command = v;
        cfg.file_manager_command = cfg.open_command.clone();
    }
    if let Some(v) = get_str!("file_manager_command") {
        cfg.file_manager_command = v;
    }
    if let Some(v) = get_str!("copy_command") {
        cfg.copy_command = v;
    }
    if let Some(v) = get_str!("unlock_command") {
        cfg.unlock_command = v;
    }
    if let Some(v) = get_str!("mode") {
        cfg.mode = v;
    }
    if let Some(v) = get_str!("menu") {
        cfg.menu = v;
    }
    if let Some(v) = get_str!("menu_flags") {
        cfg.menu_flags = v;
    }
    if let Some(icons) = obj.get("icons").and_then(|v| v.as_object()) {
        let icon_get = |key: &str| {
            icons
                .get(key)
                .and_then(|v| v.as_str())
                .map(String::from)
        };
        if let Some(v) = icon_get("mounted") {
            cfg.icons_mounted = v;
        }
        if let Some(v) = icon_get("unmounted") {
            cfg.icons_unmounted = v;
        }
        if let Some(v) = icon_get("encrypted") {
            cfg.icons_encrypted = v;
        }
        if let Some(v) = icon_get("read_only") {
            cfg.icons_read_only = v;
        }
    }
}

fn user_config_dir() -> Option<String> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME")
        && !xdg.is_empty()
    {
        return Some(format!("{}/argvus-storage", xdg.to_string_lossy()));
    }
    if let Some(home) = std::env::var_os("HOME")
        && !home.is_empty()
    {
        return Some(format!("{}/.config/argvus-storage", home.to_string_lossy()));
    }
    None
}

// JSON with // and /* */ comments allowed (the shipped config.json documents
// its keys with comments). Comments outside string literals are stripped.
fn strip_comments(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut in_string = false;
    let mut escaped = false;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if in_string {
            if c < 0x80 {
                out.push(c as char);
                if escaped {
                    escaped = false;
                } else if c == b'\\' {
                    escaped = true;
                } else if c == b'"' {
                    in_string = false;
                }
                i += 1;
            } else {
                let ch_len = utf8_len(c);
                let end = (i + ch_len).min(bytes.len());
                out.push_str(&input[i..end]);
                i = end;
            }
            continue;
        }
        match c {
            b'"' => {
                in_string = true;
                out.push('"');
                i += 1;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            }
            0x80..=0xFF => {
                let ch_len = utf8_len(c);
                let end = (i + ch_len).min(bytes.len());
                out.push_str(&input[i..end]);
                i = end;
            }
            _ => {
                out.push(c as char);
                i += 1;
            }
        }
    }
    out
}

fn utf8_len(first: u8) -> usize {
    if first >= 0xF0 {
        4
    } else if first >= 0xE0 {
        3
    } else if first >= 0xC0 {
        2
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_line_comments() {
        let s = "{\n  // a comment\n  \"key\": \"value\" /* block */\n}";
        let out = strip_comments(s);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["key"], "value");
    }

    #[test]
    fn comments_inside_strings_preserved() {
        let s = "{\"url\": \"https:////example\", \"path\": \"/a/*b*/c\"}";
        let out = strip_comments(s);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["url"], "https:////example");
        assert_eq!(v["path"], "/a/*b*/c");
    }

    #[test]
    fn utf8_preserved() {
        let s = "{\"glyph\": \"\u{f053}\"}";
        let out = strip_comments(s);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["glyph"], "\u{f053}");
    }

    #[test]
    fn defaults_apply() {
        let cfg = Config::default();
        assert_eq!(cfg.sort, "mount_time");
        assert_eq!(cfg.format, "{icon}");
        assert_eq!(cfg.file_manager_command, "xdg-open");
        assert_eq!(cfg.icons_mounted, "\u{f0a0}");
    }

    #[test]
    fn mode_parsed_from_json() {
        let mut cfg = Config::default();
        let v: serde_json::Value = serde_json::from_str("{\"mode\": \"gui\"}").unwrap();
        apply_json(&mut cfg, &v);
        assert_eq!(cfg.mode, "gui");
        assert_eq!(Config::default().mode, "rofi");
    }
}
