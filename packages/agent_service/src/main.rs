use playit_agent_core::PROTOCOL_VERSION;
use playit_agent_service_common::{ServiceCommand, ServiceResponse, DEFAULT_SOCKET_PATH};
use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    // reference to agent core
    let _version = PROTOCOL_VERSION;

    #[cfg(unix)]
    {
        use tokio::net::UnixListener;
        if let Ok(_) = std::fs::remove_file(DEFAULT_SOCKET_PATH) {}
        let listener = UnixListener::bind(DEFAULT_SOCKET_PATH)?;
        loop {
            let (stream, _) = listener.accept().await?;
            tokio::spawn(async move {
                if let Err(err) = handle_client(stream).await {
                    eprintln!("{err}");
                }
            });
        }
    }
    #[cfg(not(unix))]
    {
        eprintln!("service not implemented for this platform");
    }

    Ok(())
}

#[cfg(unix)]
async fn handle_client(mut stream: tokio::net::UnixStream) -> Result<(), Box<dyn Error>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;
    let cmd: ServiceCommand = serde_json::from_slice(&buf)?;
    let resp = match cmd {
        ServiceCommand::Start => ServiceResponse {
            message: "started".into(),
        },
        ServiceCommand::Stop => ServiceResponse {
            message: "stopped".into(),
        },
        ServiceCommand::Status => ServiceResponse {
            message: "status".into(),
        },
        ServiceCommand::Reset => ServiceResponse {
            message: "reset".into(),
        },
        ServiceCommand::ListTunnels => ServiceResponse {
            message: "tunnels".into(),
        },
    };
    let resp_bytes = serde_json::to_vec(&resp)?;
    stream.write_all(&resp_bytes).await?;
    Ok(())
}
