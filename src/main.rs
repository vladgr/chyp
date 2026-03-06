mod commands;
mod settings;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use log::info;
use std::os::unix::process::CommandExt;
use std::process::Command;

use settings::Settings;

/// Check if running as root, if not re-execute with sudo
fn ensure_root() -> Result<()> {
    let uid = unsafe { libc::getuid() };
    if uid != 0 {
        info!("Elevating privileges with sudo...");

        let args: Vec<String> = std::env::args().collect();
        let exe = std::env::current_exe().context("Failed to get current executable")?;

        // Preserve original user's HOME directory and UID/GID
        let home = std::env::var("HOME").unwrap_or_default();
        let sudo_uid = uid.to_string();
        let sudo_gid = unsafe { libc::getgid() }.to_string();

        let err = Command::new("sudo")
            .arg(format!("HOME={}", home))
            .arg(format!("SUDO_UID={}", sudo_uid))
            .arg(format!("SUDO_GID={}", sudo_gid))
            .arg(exe)
            .args(&args[1..])
            .exec();

        // exec() only returns on error
        anyhow::bail!("Failed to execute sudo: {}", err);
    }
    Ok(())
}

/// Chown path to original user (if running via sudo)
pub fn chown_to_user(path: &std::path::Path) -> Result<()> {
    if let (Ok(uid_str), Ok(gid_str)) = (std::env::var("SUDO_UID"), std::env::var("SUDO_GID")) {
        if let (Ok(uid), Ok(gid)) = (uid_str.parse::<u32>(), gid_str.parse::<u32>()) {
            let path_cstr = std::ffi::CString::new(path.to_str().unwrap_or_default())?;
            unsafe {
                libc::chown(path_cstr.as_ptr(), uid, gid);
            }
        }
    }
    Ok(())
}

#[derive(Parser)]
#[command(name = "chyp")]
#[command(about = "Cloud Hypervisor CLI tool for VM management")]
#[command(version)]
#[command(rename_all = "snake_case")]
struct Cli {
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

    /// Shared folder path
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

    /// Kill running VM
    Kill,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp(None)
        .init();

    let cli = Cli::parse();

    let settings = Settings::with_overrides(
        cli.vm_name,
        cli.image_url,
        cli.cpus,
        cli.memory_size,
        cli.disk_size,
        cli.shared_folder,
    );

    info!("chyp - Cloud Hypervisor CLI");

    match cli.command {
        Commands::Install => {
            ensure_root()?;
            commands::install::execute(&settings)?;
        }
        Commands::SetupNetwork => {
            ensure_root()?;
            commands::network::execute()?;
        }
        Commands::Run => {
            ensure_root()?;
            commands::run::execute(&settings)?;
        }
        Commands::Kill => {
            ensure_root()?;
            commands::kill::execute()?;
        }
    }

    Ok(())
}
