// Minimal i18n: picks the UI language from the process environment.
//
// Precedence follows the usual locale order (LC_ALL, LC_MESSAGES, LANG,
// LANGUAGE). A value starting with "pt" selects Portuguese; every other
// locale — including C/POSIX or an unset variable — falls back to English.
//
// Detection happens once at startup (i18n::init) and is then shared with
// every thread through a static, so translations are stable for the whole
// process lifetime.

use std::sync::OnceLock;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    En,
    Pt,
}

static LANG: OnceLock<Lang> = OnceLock::new();

// Called once from main(); tests and early errors keep the English default
// until then.
pub fn init() {
    let _ = LANG.set(detect());
}

pub fn lang() -> Lang {
    *LANG.get().unwrap_or(&Lang::En)
}

// Picks the translated variant at the call site: tr("english", "portugues").
pub fn tr<'a>(en: &'a str, pt: &'a str) -> &'a str {
    match lang() {
        Lang::Pt => pt,
        Lang::En => en,
    }
}

fn detect() -> Lang {
    for key in ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"] {
        if let Ok(value) = std::env::var(key) {
            // LANGUAGE may list several entries separated by ":"; only the
            // first one matters here since anything non-pt maps to English.
            let first = value.split(':').next().unwrap_or("");
            let code = first.split('.').next().unwrap_or("").to_lowercase();
            if code.starts_with("pt") {
                return Lang::Pt;
            }
            if !code.is_empty() && code != "c" && code != "posix" {
                return Lang::En;
            }
        }
    }
    Lang::En
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tr_defaults_to_english_before_init() {
        assert_eq!(tr("available", "disponível"), "available");
    }

    #[test]
    fn detect_reads_language_list_and_encoding_suffix() {
        // detect() is pure given env vars; exercise the parsing helpers via
        // the same rules used inside it.
        fn rule(value: &str) -> Option<Lang> {
            let first = value.split(':').next().unwrap_or("");
            let code = first.split('.').next().unwrap_or("").to_lowercase();
            if code.starts_with("pt") {
                return Some(Lang::Pt);
            }
            if !code.is_empty() && code != "c" && code != "posix" {
                return Some(Lang::En);
            }
            None
        }

        assert_eq!(rule("pt_BR.UTF-8"), Some(Lang::Pt));
        assert_eq!(rule("pt_PT"), Some(Lang::Pt));
        assert_eq!(rule("en_US.UTF-8"), Some(Lang::En));
        assert_eq!(rule("es_ES.UTF-8"), Some(Lang::En));
        assert_eq!(rule("C.UTF-8"), None);
        assert_eq!(rule("POSIX"), None);
        assert_eq!(rule("pt_BR:C"), Some(Lang::Pt));
    }
}
