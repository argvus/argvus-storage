// Native Wayland tray-style menu for removable devices.
//
// The transparent layer-shell surface provides popup semantics (positioning,
// Escape and click-outside dismissal). The visible widgets form a compact,
// hover-driven menu and an adjacent action submenu.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use gtk_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};

use crate::actions::{ActionKind, Actions};
use crate::config::Config;
use crate::device::Device;
use crate::snapshot;
use crate::util;

const MENU_WIDTH: i32 = 284;
const ACTION_MENU_WIDTH: i32 = 144;
const MENU_ROW_HEIGHT: i32 = 29;
const MENU_GAP_Y: i32 = 12;
const SCREEN_MARGIN: i32 = 4;

struct DeviceEntry {
    kind: ActionKind,
    device: Device,
}

enum UiUpdate {
    Devices(Vec<Device>),
    Close,
}

#[derive(Clone)]
struct MenuCtx {
    cfg: Config,
    handle: tokio::runtime::Handle,
    update_tx: mpsc::Sender<UiUpdate>,
    busy: Rc<Cell<bool>>,
    overlay: gtk::Window,
    fixed: gtk::Fixed,
    submenu: gtk::Box,
    submenu_x: i32,
    menu_y: i32,
    submenu_y: Rc<Cell<i32>>,
    active_row: Rc<RefCell<Option<gtk::EventBox>>>,
    hovered_action_row: Rc<RefCell<Option<gtk::EventBox>>>,
    active_index: Rc<Cell<i32>>,
    devices: Rc<RefCell<Vec<Device>>>,
}

struct PopupPosition {
    monitor: Option<gdk::Monitor>,
    x: i32,
    y: i32,
    width: i32,
    origin_x: i32,
    origin_y: i32,
}

pub fn run(
    cfg: &Config,
    devices: &[Device],
    config_path: &str,
    fixed_pos: Option<(i32, i32)>,
) -> i32 {
    if let Err(e) = gtk::init() {
        eprintln!("argvus-storage: gtk init failed: {e}");
        util::notify("argvus-storage", "cannot open the storage menu", true);
        return 1;
    }

    install_css(config_path);

    let handle = tokio::runtime::Handle::current();
    let (update_tx, update_rx) = mpsc::channel::<UiUpdate>();
    let position = popup_position(devices.len(), fixed_pos);

    let overlay = gtk::Window::new(gtk::WindowType::Toplevel);
    overlay.set_title("Removable devices");
    overlay.set_role("argvus-storage");
    overlay.set_widget_name("storage-menu-overlay");
    overlay.set_decorated(false);
    overlay.set_resizable(false);
    overlay.set_app_paintable(true);
    if let Some(screen) = gdk::Screen::default()
        && let Some(visual) = screen.rgba_visual()
    {
        overlay.set_visual(Some(&visual));
    }

    overlay.init_layer_shell();
    overlay.set_namespace("argvus-storage-menu");
    overlay.set_layer(Layer::Overlay);
    overlay.set_anchor(Edge::Top, true);
    overlay.set_anchor(Edge::Bottom, true);
    overlay.set_anchor(Edge::Left, true);
    overlay.set_anchor(Edge::Right, true);
    overlay.set_exclusive_zone(-1);
    overlay.set_keyboard_mode(KeyboardMode::Exclusive);
    if let Some(monitor) = &position.monitor {
        overlay.set_monitor(monitor);
    }

    let click_catcher = gtk::EventBox::new();
    click_catcher.set_visible_window(true);
    click_catcher.set_widget_name("storage-click-catcher");
    click_catcher.add_events(gdk::EventMask::POINTER_MOTION_MASK);
    let fixed = gtk::Fixed::new();
    click_catcher.add(&fixed);
    overlay.add(&click_catcher);

    let main_menu = gtk::Box::new(gtk::Orientation::Vertical, 0);
    main_menu.set_size_request(MENU_WIDTH, -1);
    main_menu.style_context().add_class("storage-popup-menu");
    let menu_x = position.x;
    let menu_y = position.y;
    fixed.put(&main_menu, menu_x, menu_y);

    let submenu_x = if menu_x + MENU_WIDTH + ACTION_MENU_WIDTH <= position.width - SCREEN_MARGIN {
        menu_x + MENU_WIDTH
    } else {
        (menu_x - ACTION_MENU_WIDTH).max(SCREEN_MARGIN)
    };
    let submenu_y = Rc::new(Cell::new(menu_y));
    let submenu = gtk::Box::new(gtk::Orientation::Vertical, 0);
    submenu.set_size_request(ACTION_MENU_WIDTH, -1);
    submenu.style_context().add_class("storage-popup-menu");
    submenu.set_no_show_all(true);
    fixed.put(&submenu, submenu_x, menu_y);

    let ctx = MenuCtx {
        cfg: cfg.clone(),
        handle,
        update_tx,
        busy: Rc::new(Cell::new(false)),
        overlay: overlay.clone(),
        fixed: fixed.clone(),
        submenu: submenu.clone(),
        submenu_x,
        menu_y,
        submenu_y: submenu_y.clone(),
        active_row: Rc::new(RefCell::new(None)),
        hovered_action_row: Rc::new(RefCell::new(None)),
        active_index: Rc::new(Cell::new(-1)),
        devices: Rc::new(RefCell::new(devices.to_vec())),
    };

    rebuild_menu(&main_menu, devices, &ctx);

    let outside_main = main_menu.clone();
    let outside_submenu = submenu.clone();
    let outside_submenu_y = submenu_y.clone();
    click_catcher.connect_button_press_event(move |_, event| {
        let (x, y) = event.position();
        let main_allocation = outside_main.allocation();
        let inside_main = x >= f64::from(menu_x)
            && x < f64::from(menu_x + main_allocation.width())
            && y >= f64::from(menu_y)
            && y < f64::from(menu_y + main_allocation.height());
        let submenu_y = outside_submenu_y.get();
        let submenu_allocation = outside_submenu.allocation();
        let inside_submenu = outside_submenu.is_visible()
            && x >= f64::from(submenu_x)
            && x < f64::from(submenu_x + submenu_allocation.width())
            && y >= f64::from(submenu_y)
            && y < f64::from(submenu_y + submenu_allocation.height());
        if !inside_main && !inside_submenu {
            gtk::main_quit();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });

    overlay.connect_delete_event(|_, _| {
        gtk::main_quit();
        glib::Propagation::Proceed
    });
    overlay.connect_key_press_event(|_, event| {
        if event.keyval() == gdk::keys::constants::Escape {
            gtk::main_quit();
            return glib::Propagation::Stop;
        }
        glib::Propagation::Proceed
    });

    let poll_menu = main_menu.clone();
    let poll_ctx = ctx.clone();
    let origin_x = position.origin_x;
    let origin_y = position.origin_y;
    glib::timeout_add_local(Duration::from_millis(50), move || {
        while let Ok(update) = update_rx.try_recv() {
            poll_ctx.busy.set(false);
            match update {
                UiUpdate::Devices(devices) => rebuild_menu(&poll_menu, &devices, &poll_ctx),
                UiUpdate::Close => {
                    gtk::main_quit();
                    return glib::ControlFlow::Break;
                }
            }
        }
        if let Some((pointer_x, pointer_y)) = current_pointer_position() {
            update_hover(
                &poll_menu,
                pointer_x - origin_x,
                pointer_y - origin_y,
                &poll_ctx,
            );
        }
        glib::ControlFlow::Continue
    });

    overlay.show_all();
    overlay.present();
    gtk::main();
    0
}

fn rebuild_menu(main_menu: &gtk::Box, devices: &[Device], ctx: &MenuCtx) {
    clear_menu(main_menu);
    clear_menu(&ctx.submenu);
    ctx.submenu.hide();
    *ctx.devices.borrow_mut() = devices.to_vec();
    ctx.active_index.set(-1);
    *ctx.active_row.borrow_mut() = None;
    *ctx.hovered_action_row.borrow_mut() = None;

    if devices.is_empty() {
        let empty = menu_row("No removable devices", false);
        empty.set_sensitive(false);
        main_menu.pack_start(&empty, false, false, 0);
    } else {
        for device in devices {
            let row = menu_row(&device_title(device), true);
            main_menu.pack_start(&row, false, false, 0);
        }
    }

    main_menu.pack_start(
        &gtk::Separator::new(gtk::Orientation::Horizontal),
        false,
        false,
        2,
    );

    let refresh = menu_row("Refresh", false);
    connect_refresh(&refresh, ctx);
    main_menu.pack_start(&refresh, false, false, 0);

    let quit = menu_row("Quit", false);
    quit.connect_button_release_event(|_, _| {
        gtk::main_quit();
        glib::Propagation::Stop
    });
    main_menu.pack_start(&quit, false, false, 0);
    main_menu.show_all();
}

fn menu_row(text: &str, has_submenu: bool) -> gtk::EventBox {
    let event_box = gtk::EventBox::new();
    event_box.set_visible_window(true);
    event_box.add_events(gdk::EventMask::BUTTON_PRESS_MASK | gdk::EventMask::BUTTON_RELEASE_MASK);
    event_box.set_size_request(-1, MENU_ROW_HEIGHT);
    event_box.style_context().add_class("storage-menu-row");
    event_box.connect_button_press_event(|_, _| glib::Propagation::Stop);

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.set_margin_start(10);
    row.set_margin_end(8);

    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    label.set_hexpand(true);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    row.pack_start(&label, true, true, 0);

    if has_submenu {
        let arrow = gtk::Image::from_icon_name(Some("pan-end-symbolic"), gtk::IconSize::Menu);
        row.pack_end(&arrow, false, false, 0);
    }

    event_box.add(&row);
    event_box
}

fn show_device_submenu(row: &gtk::EventBox, device: &Device, index: usize, ctx: &MenuCtx) {
    set_active_row(ctx, Some(row));
    rebuild_action_menu(device, ctx);
    let submenu_y = ctx.menu_y + index as i32 * MENU_ROW_HEIGHT;
    ctx.submenu_y.set(submenu_y);
    ctx.fixed.move_(&ctx.submenu, ctx.submenu_x, submenu_y);
    for child in ctx.submenu.children() {
        child.show_all();
    }
    ctx.submenu.show();
}

fn set_active_row(ctx: &MenuCtx, row: Option<&gtk::EventBox>) {
    if ctx
        .active_row
        .borrow()
        .as_ref()
        .is_some_and(|current| Some(current) == row)
    {
        return;
    }
    if let Some(previous) = ctx.active_row.borrow_mut().take() {
        previous.style_context().remove_class("submenu-open");
        previous.queue_draw();
    }
    if let Some(row) = row {
        row.style_context().add_class("submenu-open");
        row.queue_draw();
        *ctx.active_row.borrow_mut() = Some(row.clone());
    }
}

fn set_hovered_action_row(ctx: &MenuCtx, row: Option<&gtk::EventBox>) {
    if ctx
        .hovered_action_row
        .borrow()
        .as_ref()
        .is_some_and(|current| Some(current) == row)
    {
        return;
    }
    if let Some(previous) = ctx.hovered_action_row.borrow_mut().take() {
        previous.style_context().remove_class("pointer-over");
        previous.queue_draw();
    }
    if let Some(row) = row {
        row.style_context().add_class("pointer-over");
        row.queue_draw();
        *ctx.hovered_action_row.borrow_mut() = Some(row.clone());
    }
}

fn update_hover(main_menu: &gtk::Box, x: i32, y: i32, ctx: &MenuCtx) {
    if ctx.submenu.is_visible() {
        for child in ctx.submenu.children() {
            let Ok(row) = child.downcast::<gtk::EventBox>() else {
                continue;
            };
            if widget_contains_pointer(&row, x, y, &ctx.overlay) {
                set_hovered_action_row(ctx, Some(&row));
                return;
            }
        }
    }
    set_hovered_action_row(ctx, None);

    let device_count = ctx.devices.borrow().len();
    let rows: Vec<gtk::EventBox> = main_menu
        .children()
        .into_iter()
        .filter_map(|child| child.downcast::<gtk::EventBox>().ok())
        .collect();

    for (index, row) in rows.iter().enumerate() {
        if !widget_contains_pointer(row, x, y, &ctx.overlay) {
            continue;
        }
        if index < device_count {
            if ctx.active_index.replace(index as i32) != index as i32 {
                let device = ctx.devices.borrow()[index].clone();
                show_device_submenu(row, &device, index, ctx);
            }
        } else {
            ctx.active_index.set(-1);
            set_active_row(ctx, Some(row));
            ctx.submenu.hide();
        }
        return;
    }

    ctx.active_index.set(-1);
    set_active_row(ctx, None);
    ctx.submenu.hide();
}

fn widget_contains_pointer(widget: &gtk::EventBox, x: i32, y: i32, overlay: &gtk::Window) -> bool {
    if !widget.is_visible() {
        return false;
    }
    let Some((widget_x, widget_y)) = widget.translate_coordinates(overlay, 0, 0) else {
        return false;
    };
    let allocation = widget.allocation();
    x >= widget_x
        && x < widget_x + allocation.width()
        && y >= widget_y
        && y < widget_y + allocation.height()
}

fn rebuild_action_menu(device: &Device, ctx: &MenuCtx) {
    clear_menu(&ctx.submenu);
    for entry in device_entries(device) {
        let action = menu_row(action_label(entry.kind), false);
        action.set_tooltip_text(Some(action_tooltip(entry.kind)));
        connect_action(&action, entry, ctx);
        ctx.submenu.pack_start(&action, false, false, 0);
    }
}

fn clear_menu(container: &gtk::Box) {
    for child in container.children() {
        container.remove(&child);
    }
}

fn connect_refresh(item: &gtk::EventBox, ctx: &MenuCtx) {
    let cfg = ctx.cfg.clone();
    let handle = ctx.handle.clone();
    let update_tx = ctx.update_tx.clone();
    let busy = ctx.busy.clone();
    let item_state = item.clone();
    let refresh = Rc::new(move || {
        if busy.replace(true) {
            return;
        }
        item_state.set_sensitive(false);
        let cfg = cfg.clone();
        let update_tx = update_tx.clone();
        handle.spawn(async move {
            let devices = snapshot(&cfg).await;
            let _ = update_tx.send(UiUpdate::Devices(devices));
        });
    });

    let click_refresh = refresh.clone();
    item.connect_button_release_event(move |_, _| {
        click_refresh();
        glib::Propagation::Stop
    });
}

fn connect_action(item: &gtk::EventBox, entry: DeviceEntry, ctx: &MenuCtx) {
    let cfg = ctx.cfg.clone();
    let handle = ctx.handle.clone();
    let update_tx = ctx.update_tx.clone();
    let busy = ctx.busy.clone();
    let overlay = ctx.overlay.clone();

    item.connect_button_release_event(move |_, _| {
        if busy.replace(true) {
            return glib::Propagation::Stop;
        }

        overlay.hide();
        let kind = entry.kind;
        let device = entry.device.clone();
        let cfg = cfg.clone();
        let update_tx = update_tx.clone();
        handle.spawn(async move {
            let _ = Actions::new(&cfg).perform(kind, &device).await;
            let _ = update_tx.send(UiUpdate::Close);
        });
        glib::Propagation::Stop
    });
}

fn popup_position(device_count: usize, fixed_pos: Option<(i32, i32)>) -> PopupPosition {
    let (pointer_x, pointer_y) = fixed_pos
        .unwrap_or_else(|| current_pointer_position().unwrap_or((0, 0)));
    let display = gdk::DisplayManager::get().default_display();
    let monitor = display
        .as_ref()
        .and_then(|display| display.monitor_at_point(pointer_x, pointer_y));

    let Some(monitor_ref) = monitor.as_ref() else {
        return PopupPosition {
            monitor,
            x: SCREEN_MARGIN,
            y: 32,
            width: 1920,
            origin_x: 0,
            origin_y: 0,
        };
    };

    let geometry = monitor_ref.geometry();
    let local_x = pointer_x - geometry.x();
    let local_y = pointer_y - geometry.y();
    let estimated_height = ((device_count as i32 + 3) * 32).max(128);
    let max_x = (geometry.width() - MENU_WIDTH - SCREEN_MARGIN).max(SCREEN_MARGIN);
    let max_y = (geometry.height() - estimated_height - SCREEN_MARGIN).max(SCREEN_MARGIN);

    PopupPosition {
        monitor,
        x: local_x.clamp(SCREEN_MARGIN, max_x),
        y: (local_y + MENU_GAP_Y).clamp(SCREEN_MARGIN, max_y),
        width: geometry.width(),
        origin_x: geometry.x(),
        origin_y: geometry.y(),
    }
}

fn current_pointer_position() -> Option<(i32, i32)> {
    let output = std::process::Command::new("hyprctl")
        .args(["cursorpos", "-j"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok()?;
    let x = value.get("x")?.as_i64()? as i32;
    let y = value.get("y")?.as_i64()? as i32;
    Some((x, y))
}

// Loads theme.css from disk instead of embedding CSS in the binary.
// Candidates are tried in the same precedence order as config.json (system
// defaults, then user override, then a theme.css next to an explicit
// --config file); every one that exists is applied, in that order, so a
// later file's rules win over an earlier file's for the same selector.
// Missing files are silently skipped — if none exist, the menu falls back
// to unstyled GTK widgets rather than failing to open.
fn install_css(config_path: &str) {
    let Some(screen) = gdk::Screen::default() else {
        return;
    };
    for path in Config::theme_css_paths(config_path) {
        if !std::path::Path::new(&path).is_file() {
            continue;
        }
        let provider = gtk::CssProvider::new();
        if let Err(e) = provider.load_from_path(&path) {
            eprintln!("argvus-storage: ignoring invalid theme {}: {}", path, e);
            continue;
        }
        gtk::StyleContext::add_provider_for_screen(
            &screen,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_USER,
        );
    }
}

// Mirror the action set offered by the rofi mode, filtered by device state.
fn device_entries(d: &Device) -> Vec<DeviceEntry> {
    let can_mount = !(d.mounted || (d.encrypted && d.locked));
    let mut out = Vec::new();
    if !d.encrypted || !d.locked {
        out.push(DeviceEntry {
            kind: ActionKind::Open,
            device: d.clone(),
        });
    }
    if can_mount {
        out.push(DeviceEntry {
            kind: ActionKind::Mount,
            device: d.clone(),
        });
    }
    if d.mounted {
        out.push(DeviceEntry {
            kind: ActionKind::Unmount,
            device: d.clone(),
        });
    }
    if d.encrypted && d.locked {
        out.push(DeviceEntry {
            kind: ActionKind::Unlock,
            device: d.clone(),
        });
    }
    if d.encrypted && !d.locked && d.mounted {
        out.push(DeviceEntry {
            kind: ActionKind::Lock,
            device: d.clone(),
        });
    }
    if d.ejectable {
        out.push(DeviceEntry {
            kind: ActionKind::Eject,
            device: d.clone(),
        });
    }
    if d.can_power_off {
        out.push(DeviceEntry {
            kind: ActionKind::PowerOff,
            device: d.clone(),
        });
    }
    if d.mounted {
        out.push(DeviceEntry {
            kind: ActionKind::Copy,
            device: d.clone(),
        });
    }
    out
}

fn action_label(kind: ActionKind) -> &'static str {
    match kind {
        ActionKind::Open => "Open",
        ActionKind::Mount => "Mount",
        ActionKind::Unmount => "Unmount",
        ActionKind::Eject => "Eject",
        ActionKind::PowerOff => "Power Off",
        ActionKind::Lock => "Lock",
        ActionKind::Unlock => "Unlock",
        ActionKind::Copy => "Copy path",
    }
}

fn action_tooltip(kind: ActionKind) -> &'static str {
    match kind {
        ActionKind::Open => "Open in the configured file manager",
        ActionKind::Mount => "Mount this device",
        ActionKind::Unmount => "Unmount this device",
        ActionKind::Eject => "Unmount and eject this device",
        ActionKind::PowerOff => "Safely power off this drive",
        ActionKind::Lock => "Lock this encrypted volume",
        ActionKind::Unlock => "Unlock this encrypted volume",
        ActionKind::Copy => "Copy the mount path",
    }
}

fn device_title(d: &Device) -> String {
    let state = if d.encrypted && d.locked {
        "locked"
    } else if d.mounted {
        "mounted"
    } else {
        "available"
    };
    if d.capacity > 0 {
        format!("{} · {} · {}", d.name, util::human_bytes(d.capacity), state)
    } else {
        format!("{} · {}", d.name, state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mounted_device_has_open_unmount_and_copy_actions() {
        let device = Device {
            mounted: true,
            ejectable: true,
            ..Device::default()
        };
        let kinds: Vec<ActionKind> = device_entries(&device)
            .into_iter()
            .map(|entry| entry.kind)
            .collect();

        assert!(kinds.contains(&ActionKind::Open));
        assert!(kinds.contains(&ActionKind::Unmount));
        assert!(kinds.contains(&ActionKind::Eject));
        assert!(kinds.contains(&ActionKind::Copy));
        assert!(!kinds.contains(&ActionKind::Mount));
    }

    #[test]
    fn locked_device_only_offers_unlock_and_safe_removal() {
        let device = Device {
            encrypted: true,
            locked: true,
            ejectable: true,
            can_power_off: true,
            ..Device::default()
        };
        let kinds: Vec<ActionKind> = device_entries(&device)
            .into_iter()
            .map(|entry| entry.kind)
            .collect();

        assert!(kinds.contains(&ActionKind::Unlock));
        assert!(kinds.contains(&ActionKind::Eject));
        assert!(kinds.contains(&ActionKind::PowerOff));
        assert!(!kinds.contains(&ActionKind::Open));
        assert!(!kinds.contains(&ActionKind::Mount));
    }

    #[test]
    fn device_title_matches_tray_menu_format() {
        let device = Device {
            name: "Ventoy".to_string(),
            capacity: 43_700_000_000,
            ..Device::default()
        };

        assert_eq!(device_title(&device), "Ventoy · 43.7 GB · available");
    }
}
