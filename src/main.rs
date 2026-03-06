mod commands;
mod settings;

use anyhow::Result;
use clap::{Parser, Subcommand};
use log::info;

use settings::Settings;

/// Run chown on the .chyp directory to ensure user ownership
pub fn chown_chyp_dir(settings: &Settings) -> Result<()> {
    let user = &settings.user;
    let chyp_dir = &settings.project_folder;

    let status = std::process::Command::new("sudo")
        .args(["chown", "-R", &format!("{}:{}", user, user), chyp_dir])
        .status()?;

    if !status.success() {
        log::warn!("Failed to chown {}", chyp_dir);
    }
    Ok(())
}

#[derive(Parser)]
#[command(name = "chyp")]
#[command(about = "Cloud Hypervisor CLI tool for VM management")]
#[command(version)]
#[command(rename_all = "snake_case")]
struct Cli {
    /// Username for file ownership
    #[arg(long)]
    user: Option<String>,

    /// VM name
    #[arg(long)]
    vm_name: Option<String>,

    /// Cloud image URL
    #[arg(long)]
    image_url: Option<String>,

    /// Number of CPUs
    #[arg(long)]
    cpus: Option<u32>,

    /// Memory size in GB
    #[arg(long)]
    memory_size: Option<u32>,

    /// Disk size in GB
    #[arg(long)]
    disk_size: Option<u32>,

    /// Project folder path (stores VM images and configs)
    #[arg(long)]
    project_folder: Option<String>,

    /// Shared folder path (shared with VM)
    #[arg(long)]
    shared_folder: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
#[command(rename_all = "kebab_case")]
enum Commands {
    /// Install Cloud Hypervisor and virtiofsd
    Install,

    /// Setup network bridge with internet access
    SetupNetwork,

    /// Run virtual machine
    Run,

    /// Stop running VM
    Stop,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();

    let cli = Cli::parse();

    let settings = Settings::with_overrides(
        cli.user,
        cli.vm_name,
        cli.image_url,
        cli.cpus,
        cli.memory_size,
        cli.disk_size,
        cli.project_folder,
        cli.shared_folder,
    );

    info!("chyp - Cloud Hypervisor CLI");

    match cli.command {
        Commands::Install => {
            commands::install::execute(&settings)?;
        }
        Commands::SetupNetwork => {
            commands::network::execute()?;
        }
        Commands::Run => {
            commands::run::execute(&settings)?;
        }
        Commands::Stop => {
            commands::stop::execute()?;
        }
    }

    Ok(())
}
