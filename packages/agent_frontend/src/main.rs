use clap::{Parser, Subcommand};
use playit_agent_service_common::{send_command, ServiceCommand, DEFAULT_SOCKET_PATH};
use std::error::Error;

#[derive(Parser)]
#[command(name = "playit-agent-frontend")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Start,
    Stop,
    Status,
    Reset,
    #[command(name = "list-tunnels")]
    ListTunnels,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let cmd = match cli.command {
        Commands::Start => ServiceCommand::Start,
        Commands::Stop => ServiceCommand::Stop,
        Commands::Status => ServiceCommand::Status,
        Commands::Reset => ServiceCommand::Reset,
        Commands::ListTunnels => ServiceCommand::ListTunnels,
    };

    #[cfg(unix)]
    {
        let resp = send_command(DEFAULT_SOCKET_PATH, &cmd).await?;
        println!("{}", resp.message);
    }

    #[cfg(not(unix))]
    {
        println!("frontend not supported on this platform");
    }

    Ok(())
}
