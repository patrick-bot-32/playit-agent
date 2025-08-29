use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub enum ServiceCommand {
    Start,
    Stop,
    Status,
    Reset,
    ListTunnels,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ServiceResponse {
    pub message: String,
}

pub const DEFAULT_SOCKET_PATH: &str = "/tmp/playit-agent.sock";

#[cfg(unix)]
pub async fn send_command(
    path: &str,
    cmd: &ServiceCommand,
) -> Result<ServiceResponse, Box<dyn std::error::Error>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixStream;

    let mut stream = UnixStream::connect(path).await?;
    let data = serde_json::to_vec(cmd)?;
    stream.write_all(&data).await?;
    stream.shutdown().await?;

    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).await?;
    let resp: ServiceResponse = serde_json::from_slice(&buf)?;
    Ok(resp)
}

#[cfg(not(unix))]
pub async fn send_command(
    _path: &str,
    _cmd: &ServiceCommand,
) -> Result<ServiceResponse, Box<dyn std::error::Error>> {
    Err("unsupported platform".into())
}
