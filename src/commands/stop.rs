use anyhow::{Context, Result};
use log::info;
use std::process::Command;

pub fn execute() -> Result<()> {
    info!("Stopping VM...");

    let status = Command::new("sudo")
        .args(["pkill", "-f", "cloud-hypervisor"])
        .status()
        .context("Failed to run pkill")?;

    if status.success() {
        info!("VM stopped");
    } else {
        info!("No running VM found");
    }

    // Also kill virtiofsd
    let _ = Command::new("sudo")
        .args(["pkill", "-f", "virtiofsd"])
        .status();

    Ok(())
}
