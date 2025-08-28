#[cfg(windows)]
mod win_service {
    use std::{
        ffi::OsString,
        sync::{mpsc, Arc},
        sync::atomic::{AtomicBool, Ordering},
        thread,
        time::Duration,
    };

    use playit_agent_core::{
        agent_control::{platform::get_platform, version::register_version},
        network::{origin_lookup::OriginLookup, tcp::tcp_settings::TcpSettings, udp::udp_settings::UdpSettings},
        playit_agent::{PlayitAgent, PlayitAgentSettings},
        PROTOCOL_VERSION,
    };
    use playit_api_client::{api::{AgentAccountStatus, AgentVersion, PlayitAgentVersion}, PlayitApi};
    use serde::Deserialize;
    use tokio::runtime::Runtime;
    use tray_icon::menu::{Menu, MenuItem};
    use tray_icon::{Icon, TrayIconBuilder};
    use windows_service::{
        define_windows_service,
        service::{
            ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
            ServiceType,
        },
        service_control_handler::{self, ServiceControlHandlerResult},
        service_dispatcher, Result,
    };

    const SERVICE_NAME: &str = "playit-service";
    const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;

    define_windows_service!(ffi_service_main, service_main);

    pub fn run() -> Result<()> {
        service_dispatcher::start(SERVICE_NAME, ffi_service_main)
    }

    fn service_main(_arguments: Vec<OsString>) {
        if let Err(e) = run_service() {
            tracing::error!(?e, "service error");
        }
    }

    fn run_service() -> Result<()> {
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let (tray_close_tx, tray_close_rx) = mpsc::channel();

        let status_handle = service_control_handler::register(
            SERVICE_NAME,
            {
                let shutdown_tx = shutdown_tx.clone();
                move |control_event| match control_event {
                    ServiceControl::Stop => {
                        let _ = shutdown_tx.send(());
                        ServiceControlHandlerResult::NoError
                    }
                    _ => ServiceControlHandlerResult::NotImplemented,
                }
            },
        )?;

        status_handle.set_service_status(ServiceStatus {
            service_type: SERVICE_TYPE,
            current_state: ServiceState::Running,
            controls_accepted: ServiceControlAccept::STOP,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        })?;

        #[derive(Deserialize)]
        struct Config { secret_key: String }

        fn load_secret() -> Option<String> {
            if let Ok(secret) = std::env::var("PLAYIT_AGENT_SECRET") {
                return Some(secret);
            }

            let path = if let Some(mut dir) = dirs::config_local_dir() {
                dir.push("playit_gg");
                let _ = std::fs::create_dir_all(&dir);
                dir.push("playit.toml");
                dir
            } else {
                std::path::PathBuf::from("playit.toml")
            };

            let content = std::fs::read_to_string(&path).ok()?;
            if let Ok(cfg) = toml::from_str::<Config>(&content) {
                Some(cfg.secret_key)
            } else {
                Some(content.trim().to_string())
            }
        }

        let secret = match load_secret() {
            Some(s) => s,
            None => {
                tracing::error!("missing secret key");
                return Ok(());
            }
        };

        let login_url = {
            let rt = Runtime::new().expect("runtime");
            rt.block_on(async {
                let api = PlayitApi::create("https://api.playit.gg".to_string(), Some(secret.clone()));
                if let Ok(data) = api.agents_rundata().await {
                    if matches!(data.account_status, AgentAccountStatus::Guest) {
                        if let Ok(session) = api.login_guest().await {
                            return Some(format!("https://playit.gg/login/guest-account/{}", session.session_key));
                        }
                    }
                }
                None
            })
        };

        // build tray icon on separate thread
        let tray_shutdown = shutdown_tx.clone();
        let tray_login_url = login_url.clone();
        let tray_handle = thread::spawn(move || {
            // simple empty icon; on real build include icon file
            let icon = Icon::from_rgba(vec![0; 16 * 16 * 4], 16, 16).ok();
            let mut tray_builder = TrayIconBuilder::new().with_tooltip("Playit Agent");
            if let Some(icon) = icon {
                tray_builder = tray_builder.with_icon(icon);
            }
            let menu = Menu::new();
            let quit_item = MenuItem::with_id("quit", "Quit", true, None);
            let login_item = MenuItem::with_id("login", "Login", true, None);
            if tray_login_url.is_some() {
                let _ = menu.append(&login_item);
            }
            let _ = menu.append(&quit_item);
            let quit_id = quit_item.id().clone();
            let login_id = login_item.id().clone();
            tray_builder = tray_builder.with_menu(Box::new(menu));
            let _tray_icon = tray_builder.build().expect("tray icon");

            let menu_channel = tray_icon::menu::MenuEvent::receiver();
            loop {
                if let Ok(event) = menu_channel.recv_timeout(Duration::from_millis(100)) {
                    if event.id() == &quit_id {
                        let _ = tray_shutdown.send(());
                    } else if event.id() == &login_id {
                        if let Some(url) = &tray_login_url {
                            let _ = webbrowser::open(url);
                        }
                    }
                }
                if tray_close_rx.try_recv().is_ok() {
                    break;
                }
            }
        });

        // start the playit agent using agent-core
        let (agent_keep_tx, agent_keep_rx) = mpsc::channel::<Arc<AtomicBool>>();

        let agent_secret = secret.clone();
        let agent_thread = thread::spawn(move || {
            let rt = Runtime::new().expect("runtime");
            rt.block_on(async move {
                let secret = agent_secret;

                register_version(PlayitAgentVersion {
                    version: AgentVersion {
                        platform: get_platform(),
                        version: env!("CARGO_PKG_VERSION").to_string(),
                        has_expired: false,
                    },
                    official: true,
                    details_website: None,
                    proto_version: PROTOCOL_VERSION,
                });

                let api = PlayitApi::create("https://api.playit.gg".to_string(), Some(secret.clone()));
                let lookup = Arc::new(OriginLookup::default());
                if let Ok(data) = api.agents_rundata().await {
                    lookup.update_from_run_data(&data).await;
                }

                let settings = PlayitAgentSettings {
                    udp_settings: UdpSettings::default(),
                    tcp_settings: TcpSettings::default(),
                    api_url: "https://api.playit.gg".to_string(),
                    secret_key: secret,
                };

                match PlayitAgent::new(settings, lookup.clone()).await {
                    Ok(agent) => {
                        let keep = agent.keep_running();
                        let _ = agent_keep_tx.send(keep.clone());

                        let api_clone = api.clone();
                        let lookup_clone = lookup.clone();
                        let keep_clone = keep.clone();
                        tokio::spawn(async move {
                            while keep_clone.load(Ordering::SeqCst) {
                                match api_clone.agents_rundata().await {
                                    Ok(data) => lookup_clone.update_from_run_data(&data).await,
                                    Err(e) => tracing::error!(?e, "failed to refresh rundata"),
                                }
                                tokio::time::sleep(Duration::from_secs(10)).await;
                            }
                        });

                        agent.run().await;
                    }
                    Err(e) => tracing::error!(?e, "failed to start agent"),
                }
            });
        });

        let keep_running = agent_keep_rx.recv().ok();

        let _ = shutdown_rx.recv();
        let _ = tray_close_tx.send(());
        let _ = tray_handle.join();

        if let Some(keep) = keep_running {
            keep.store(false, Ordering::SeqCst);
        }
        let _ = agent_thread.join();

        status_handle.set_service_status(ServiceStatus {
            service_type: SERVICE_TYPE,
            current_state: ServiceState::Stopped,
            controls_accepted: ServiceControlAccept::empty(),
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        })?;

        Ok(())
    }
}

#[cfg(windows)]
fn main() -> windows_service::Result<()> {
    win_service::run()
}

#[cfg(not(windows))]
fn main() {
    println!("This binary is only supported on Windows");
}
