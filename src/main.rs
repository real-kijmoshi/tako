mod auth;
mod auto_update;
mod catalog;
mod config;
mod daemon;
mod dashboard;
mod edit;
mod failure;
mod server;
mod stream;
mod translate;
mod tui;
mod usage;
mod util;

const HELP: &str = "\
tako: one local endpoint for all your AI providers, with automatic fallback

usage: tako [command]     (no command opens the dashboard)

  claude       launch Claude Code through tako (starts tako if needed)
  web          open the web dashboard in your browser (starts tako if needed)

  start        start in the background
  stop         stop it
  restart      restart it
  status       show what's running, providers, combos, traffic
  logs [-f]    show the log (-f to follow)

  password     set/reset the web admin password (hidden terminal prompt)
  setup-vps [IP:port | --proxy]  configure password and VPS access
  config       edit the config (changes apply live)
  check        validate the config

  enable       start automatically at login
  disable      don't start at login

  update       update to the latest release
  auto-update  on / off / status: opt into daily verified updates
  uninstall    remove tako
  run          run in the foreground
  version      print the version
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let tty = unsafe { libc::isatty(0) == 1 && libc::isatty(1) == 1 };
    let cmd = args
        .first()
        .map(String::as_str)
        .unwrap_or(if tty { "tui" } else { "help" });
    let done = |r: Result<String, String>| r.map(|m| println!("{} {m}", util::green("✓")));
    let r = match cmd {
        "tui" | "ui" | "dash" => tui::run(),
        "claude" => daemon::claude(&args[1..]),
        "web" | "browser" | "open" => daemon::web(),
        "run" => server::run(),
        "start" => done(daemon::start()),
        "stop" => done(daemon::stop()),
        "restart" => done(daemon::restart()),
        "status" | "st" => daemon::status(),
        "logs" | "log" => daemon::logs(args.iter().any(|a| a == "-f" || a == "--follow")),
        "config" | "edit" => match args.get(1).map(String::as_str) {
            Some("path") => {
                println!("{}", config::path().display());
                Ok(())
            }
            _ => config::edit(),
        },
        "password" if args.iter().any(|a| a == "--help" || a == "-h") => {
            println!("usage: tako password\n\nSet or reset the web admin password using a hidden terminal prompt.\nUse at least 12 characters. Existing browser sessions are signed out; API keys keep working.");
            Ok(())
        }
        "password" if args.len() > 1 => Err("Passwords are entered in a hidden terminal prompt. Run `tako password` without arguments.".into()),
        "password" => auth::password(),
        "setup-vps" => setup_vps(&args[1..]),
        "check" => config::check(),
        "enable" => done(daemon::enable()),
        "disable" => done(daemon::disable()),
        "update" | "upgrade" => daemon::update(),
        "auto-update" => auto_update::command(&args[1..]),
        "uninstall" => daemon::uninstall(),
        "version" | "-V" | "--version" => {
            println!("tako {}", server::VERSION);
            Ok(())
        }
        "help" | "-h" | "--help" => {
            print!("{HELP}");
            Ok(())
        }
        other => Err(format!("unknown command '{other}', see `tako help`")),
    };
    if let Err(e) = r {
        eprintln!("{} {e}", util::red("error:"));
        std::process::exit(1);
    }
}

fn setup_vps(args: &[String]) -> Result<(), String> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("usage: tako setup-vps [IP:port | --proxy]\n\n  --proxy   bind to 127.0.0.1:20129 for an HTTPS reverse proxy (recommended)\n  IP:port   bind to a specific address\n  omitted   bind to 0.0.0.0:20129 for direct access\n\nPrompts for an admin password. Use Access & keys in the panel to create client keys.");
        return Ok(());
    }
    if args.len() > 1 {
        return Err("Use `tako setup-vps --help` for setup options".into());
    }
    let address = match args.first().map(String::as_str) {
        Some("--proxy") => "127.0.0.1:20129",
        Some(a) => a,
        None => "0.0.0.0:20129",
    };
    let parsed: std::net::SocketAddr = address
        .parse()
        .map_err(|_| "Use an IP:port address or --proxy. See `tako setup-vps --help`.")?;
    println!(
        "VPS setup · web panel + client API\n\n  Listen: {address}\n  Config: {}\n",
        config::path().display()
    );
    auth::password()?;
    edit::set_listen(address)?;
    let panel = if parsed.ip().is_loopback() {
        "Configure your HTTPS reverse proxy, then open https://YOUR_DOMAIN/ui".to_string()
    } else if parsed.ip().is_unspecified() {
        format!(
            "Open http://YOUR_VPS_IP:{}/ui (use HTTPS for internet access)",
            parsed.port()
        )
    } else {
        format!("Open http://{parsed}/ui (use HTTPS for internet access)")
    };
    println!("\nNext steps:\n  1. Run `tako restart` (or `tako run` for a foreground service).\n  2. {panel}.\n  3. Connect a provider, build a combo, then create a client key in Access & keys.");
    Ok(())
}
