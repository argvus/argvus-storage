# Contributing

Thank you for contributing to Argvus Storage.

## Guidelines

- Keep the Waybar output stable and machine-readable.
- Avoid polling when UDisks2 events can provide the signal.
- Preserve the precedence order: system defaults, user overrides, explicit config path.
- Keep GTK menu styling in CSS files, not hard-coded in Rust, unless behavior requires code.
- Do not move system defaults back into `argvus`; this package owns its config and themes.

## Pull requests

Include:

- what changed;
- how it affects Waybar or the menu;
- how it was tested;
- any new configuration keys or theme selectors.

Run before submitting:

```sh
cargo test --locked
cargo clippy --locked --all-targets --all-features -- -D warnings
```
