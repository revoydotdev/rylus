use clap::CommandFactory;
use clap_complete::generate;
#[cfg(unix)]
use signal_hook::iterator::Signals;
#[cfg(unix)]
use signal_hook::{consts::TERM_SIGNALS, low_level::signal_name};
use tracing::{error, info, warn};

use rylus_core::config::{get_config, Config};
use rylus_core::Web2UiMessage;

mod log;
mod mdns;
mod rylus;
mod session;
mod tls;
mod web;

fn main() {
    let (sender, receiver) = std::sync::mpsc::sync_channel::<String>(100);

    log::setup_logging(sender);

    let conf = get_config();

    if let Some(shell) = conf.completions {
        generate(
            shell,
            &mut Config::command(),
            "rylus",
            &mut std::io::stdout(),
        );
        return;
    }

    if conf.print_index_html {
        print!("{}", web::INDEX_HTML);
        return;
    }
    if conf.print_access_html {
        print!("{}", web::ACCESS_HTML);
        return;
    }
    if conf.print_style_css {
        print!("{}", web::STYLE_CSS);
        return;
    }
    if conf.print_lib_js {
        print!("{}", web::LIB_JS);
        return;
    }

    #[cfg(target_os = "linux")]
    {
        // make sure XInitThreads is called before any threading is done
        #[cfg(feature = "x11")]
        rylus_capture::x11::x11_init();

        pipewire::init();
    }

    #[cfg(feature = "gui")]
    if !conf.no_gui {
        run_with_gui(conf, receiver);
        return;
    }

    // No-GUI mode: start server immediately
    drop(receiver);
    run_headless(conf);
}

fn run_headless(conf: Config) {
    let mut rylus_server = rylus::Rylus::new();
    let mut mdns_publisher = if !conf.no_mdns {
        Some(mdns::MdnsPublisher::new())
    } else {
        None
    };
    if let Some(ref mut publisher) = mdns_publisher {
        publisher.register(conf.web_port, None).ok();
    }
    if !rylus_server.start(
        &conf,
        Box::new(|msg| match msg {
            Web2UiMessage::UInputInaccessible => {
                warn!(std::include_str!("strings/uinput_error.txt"))
            }
        }),
    ) {
        error!("Failed to start Rylus server.");
        std::process::exit(1);
    }
    #[cfg(unix)]
    {
        let mut signals = Signals::new(TERM_SIGNALS).expect("Failed to register signal handlers");
        #[allow(clippy::never_loop)]
        for sig in signals.forever() {
            info!(
                "Shutting down after receiving signal {signame} ({sig})...",
                signame = signal_name(sig).unwrap_or("UNKNOWN SIGNAL")
            );
            std::thread::spawn(move || {
                #[allow(clippy::never_loop)]
                for sig in signals.forever() {
                    warn!(
                        "Received second signal {signame} ({sig}) while shutting down \
                        gracefully, proceeding with forceful shutdown...",
                        signame = signal_name(sig).unwrap_or("UNKNOWN SIGNAL")
                    );
                    std::process::exit(1);
                }
            });
            rylus_server.stop();
            break;
        }
    }
    #[cfg(not(unix))]
    {
        loop {
            std::thread::park();
        }
    }
}

#[cfg(feature = "gui")]
fn run_with_gui(conf: Config, receiver: std::sync::mpsc::Receiver<String>) {
    rylus_gui::run(&conf, receiver, Box::new(rylus::Rylus::new()));
}
