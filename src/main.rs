mod actions;
mod config;
mod device;
mod gui;
mod i18n;
mod menu;
mod monitor;
mod renderer;
mod udisks;
mod util;

use std::collections::HashMap;
use std::io::Write;

use actions::{ActionKind, Actions};
use config::Config;
use device::{Device, build_devices};
use menu::Menu;
use monitor::Monitor;
use renderer::Renderer;
use udisks::UdisksClient;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_usage(out: &mut dyn Write) {
    let usage = i18n::tr(
            "argvus-storage {VERSION} - removable storage module for waybar (UDisks2)\n\
             \n\
             Usage: argvus-storage [options] <command> [device]\n\
             \n\
             Commands:\n\
             \x20 watch        emit a JSON line for waybar whenever devices change (default)\n\
             \x20 once         emit a single JSON line and exit\n\
             \x20 list         print removable storage devices\n\
             \x20 devices      choose a device and open it in the file manager\n\
             \x20 menu         interactive context menu (gui or rofi mode)\n\
             \x20 open         open the device (mounts it first if needed)\n\
             \x20 mount        mount the device\n\
             \x20 unmount      unmount the device\n\
             \x20 eject        eject the media\n\
             \x20 poweroff     power off the drive\n\
             \x20 lock         lock an unlocked LUKS volume\n\
             \x20 unlock       unlock a locked LUKS volume (passphrase in terminal)\n\
             \x20 copy         copy the mount path to the clipboard\n\
             \n\
             [device] selects a device by block node (/dev/sdb1), label, name or\n\
             object path; defaults to the first device when omitted.\n\
             \n\
             Options:\n\
             \x20 --config <path>  use an explicit config file\n\
             \x20 --x <px>        pin the menu X to a position (gui mode)\n\
             \x20 --y <px>        pin the menu Y to a position (gui mode)\n\
             \x20 -h, --help       show this help\n\
             \x20 -v, --version    print the version",
            "argvus-storage {VERSION} - módulo de armazenamento removível para waybar (UDisks2)\n\
             \n\
             Uso: argvus-storage [opções] <comando> [dispositivo]\n\
             \n\
             Comandos:\n\
             \x20 watch        emite uma linha JSON para a waybar quando os dispositivos mudam (padrão)\n\
             \x20 once         emite uma única linha JSON e sai\n\
             \x20 list         lista os dispositivos de armazenamento removíveis\n\
             \x20 devices      escolhe um dispositivo e abre no gerenciador de arquivos\n\
             \x20 menu         menu de contexto interativo (modo gui ou rofi)\n\
             \x20 open         abre o dispositivo (montando antes, se necessário)\n\
             \x20 mount        monta o dispositivo\n\
             \x20 unmount      desmonta o dispositivo\n\
             \x20 eject        ejeta a mídia\n\
             \x20 poweroff     desliga a unidade\n\
             \x20 lock         bloqueia um volume LUKS desbloqueado\n\
             \x20 unlock       desbloqueia um volume LUKS bloqueado (senha no terminal)\n\
             \x20 copy         copia o ponto de montagem para a área de transferência\n\
             \n\
             [dispositivo] seleciona um dispositivo por nó de bloco (/dev/sdb1), rótulo,\n\
             nome ou caminho de objeto; usa o primeiro dispositivo quando omitido.\n\
             \n\
             Opções:\n\
             \x20 --config <caminho>  usa um arquivo de configuração explícito\n\
             \x20 --x <px>        fixa o X do menu em uma posição (modo gui)\n\
             \x20 --y <px>        fixa o Y do menu em uma posição (modo gui)\n\
             \x20 -h, --help       mostra esta ajuda\n\
             \x20 -v, --version    imprime a versão"
        )
    .replace("{VERSION}", VERSION);
    let _ = writeln!(out, "{}\n", usage);
}

pub(crate) async fn snapshot(cfg: &Config) -> Vec<Device> {
    let mut client = UdisksClient::new();
    if !client.connect().await {
        eprintln!("argvus-storage: {}", client.last_error());
        return Vec::new();
    }
    let raw = client.enumerate().await;
    let mut mt: HashMap<String, i64> = HashMap::new();
    let mut seq: HashMap<String, u64> = HashMap::new();
    let mut next = 0u64;
    build_devices(&raw, cfg, &mut mt, &mut seq, &mut next)
}

fn pick<'a>(devs: &'a [Device], token: &str) -> Option<&'a Device> {
    if !token.is_empty() {
        for d in devs {
            if d.block == token || d.name == token || d.object_path == token {
                return Some(d);
            }
        }
        for d in devs {
            let base = d.object_path.rsplit('/').next().unwrap_or("");
            if base == token {
                return Some(d);
            }
        }
    }
    devs.first()
}

async fn run_watch(cfg: &Config, once: bool) -> i32 {
    let renderer = Renderer::new(cfg);
    let emit = |devs: &[Device]| {
        let result = renderer.render(devs);
        let mut stdout = std::io::stdout().lock();
        if stdout.write_all(result.json.as_bytes()).is_err() || stdout.write_all(b"\n").is_err() {
            // waybar restarted the stream; leave quietly.
            std::process::exit(0);
        }
        let _ = stdout.flush();
    };

    if once {
        emit(&snapshot(cfg).await);
        return 0;
    }

    let mut monitor = Monitor::new(cfg);
    match monitor.run(emit).await {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("argvus-storage: {}", e);
            1
        }
    }
}

async fn run_action(cfg: &Config, cmd: &str, token: &str) -> i32 {
    let kind = match cmd {
        "open" => ActionKind::Open,
        "mount" => ActionKind::Mount,
        "unmount" => ActionKind::Unmount,
        "eject" => ActionKind::Eject,
        "poweroff" => ActionKind::PowerOff,
        "lock" => ActionKind::Lock,
        "unlock" => ActionKind::Unlock,
        "copy" => ActionKind::Copy,
        _ => return -1,
    };
    let devs = snapshot(cfg).await;
    let Some(d) = pick(&devs, token) else {
        eprintln!(
            "argvus-storage: {}",
            i18n::tr(
                "no removable storage devices",
                "nenhum dispositivo de armazenamento removível"
            )
        );
        return 1;
    };
    let actions = Actions::new(cfg);
    actions.perform(kind, d).await
}

fn run_list(devs: &[Device]) -> i32 {
    for d in devs {
        println!(
            "{}\t{}\t{}\t{}\t{}",
            d.block,
            d.name,
            d.filesystem,
            if d.mounted { &d.mount_point } else { "-" },
            util::human_bytes(d.capacity)
        );
    }
    0
}

#[tokio::main]
async fn main() {
    i18n::init();

    // Never die on a closed stdout (waybar restarting the stream).
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut config_path = String::new();
    let mut positional: Vec<String> = Vec::new();
    let mut fixed_x: Option<i32> = None;
    let mut fixed_y: Option<i32> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--config" {
            i += 1;
            if i >= args.len() {
                eprintln!(
                    "argvus-storage: {}",
                    i18n::tr("--config requires a path", "--config exige um caminho")
                );
                std::process::exit(2);
            }
            config_path = args[i].clone();
        } else if arg == "--x" {
            i += 1;
            if i >= args.len() {
                eprintln!(
                    "argvus-storage: {}",
                    i18n::tr("--x requires a value", "--x exige um valor")
                );
                std::process::exit(2);
            }
            fixed_x = args[i].parse().ok();
        } else if arg == "--y" {
            i += 1;
            if i >= args.len() {
                eprintln!(
                    "argvus-storage: {}",
                    i18n::tr("--y requires a value", "--y exige um valor")
                );
                std::process::exit(2);
            }
            fixed_y = args[i].parse().ok();
        } else if arg == "-h" || arg == "--help" {
            print_usage(&mut std::io::stdout());
            return;
        } else if arg == "-v" || arg == "--version" {
            println!("argvus-storage {}", VERSION);
            return;
        } else {
            positional.push(arg.clone());
        }
        i += 1;
    }

    let cmd = positional
        .first()
        .cloned()
        .unwrap_or_else(|| "watch".to_string());
    let token = positional.get(1).cloned().unwrap_or_default();

    let cfg = Config::load(&config_path);

    let code = match cmd.as_str() {
        "watch" => run_watch(&cfg, false).await,
        "once" => run_watch(&cfg, true).await,
        "list" => run_list(&snapshot(&cfg).await),
        "menu" => {
            let devs = snapshot(&cfg).await;
            if cfg.mode == "gui" {
                match fixed_x.zip(fixed_y) {
                    Some(pos) => gui::run(&cfg, &devs, &config_path, Some(pos)),
                    None => {
                        // GUI menu only opens anchored to a waybar click; the
                        // SUPER+SHIFT+S keybinding is rofi-only.
                        0
                    }
                }
            } else {
                Menu::new(&cfg).run(&devs).await
            }
        }
        "devices" => {
            let devs = snapshot(&cfg).await;
            Menu::new(&cfg).run_devices(&devs).await
        }
        _ => run_action(&cfg, &cmd, &token).await,
    };
    if code == -1 {
        eprintln!(
            "argvus-storage: {}",
            i18n::tr(
                &format!("unknown command '{}'", cmd),
                &format!("comando desconhecido '{}'", cmd)
            )
        );
        print_usage(&mut std::io::stderr());
        std::process::exit(2);
    }
    std::process::exit(code);
}
