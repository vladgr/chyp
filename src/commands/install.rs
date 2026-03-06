use anyhow::{Context, Result};
use log::{info, warn};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use crate::settings::Settings;

const CH_VERSION: &str = "v43.0";
const CH_RELEASE_URL: &str = "https://github.com/cloud-hypervisor/cloud-hypervisor/releases/download";

pub fn execute(settings: &Settings) -> Result<()> {
    info!("Installing Cloud Hypervisor and dependencies...");

    // Create directory structure
    create_directories(settings)?;

    // Install system dependencies
    install_dependencies()?;

    // Install Cloud Hypervisor
    install_cloud_hypervisor()?;

    // Install hypervisor firmware
    install_firmware()?;

    // Install virtiofsd
    install_virtiofsd()?;

    // Verify installation
    verify_installation()?;

    // Ensure user ownership of .chyp directory
    crate::chown_chyp_dir()?;

    info!("Installation completed successfully!");
    Ok(())
}

fn install_dependencies() -> Result<()> {
    info!("Installing system dependencies...");

    let status = Command::new("sudo")
        .args(["apt-get", "install", "-y", "qemu-utils", "cloud-image-utils", "genisoimage", "libguestfs-tools"])
        .status()
        .context("Failed to install dependencies")?;

    if !status.success() {
        warn!("Some dependencies may not have installed correctly");
    }

    Ok(())
}

fn create_directories(settings: &Settings) -> Result<()> {
    info!("Creating directory structure at {:?}", settings.base_dir());

    let dirs = [
        settings.base_dir(),
        settings.images_dir(),
        settings.vms_dir(),
        settings.shared_dir(),
    ];

    for dir in &dirs {
        fs::create_dir_all(dir)
            .with_context(|| format!("Failed to create directory: {:?}", dir))?;
        info!("Created directory: {:?}", dir);
    }

    Ok(())
}

fn install_cloud_hypervisor() -> Result<()> {
    info!("Downloading Cloud Hypervisor {}...", CH_VERSION);

    let download_url = format!(
        "{}/{}/cloud-hypervisor-static",
        CH_RELEASE_URL, CH_VERSION
    );

    let tmp_path = "/tmp/cloud-hypervisor";
    let install_path = "/usr/local/bin/cloud-hypervisor";

    // Download using curl
    let status = Command::new("curl")
        .args(["-L", "-o", tmp_path, &download_url])
        .status()
        .context("Failed to execute curl")?;

    if !status.success() {
        anyhow::bail!("Failed to download Cloud Hypervisor");
    }

    // Make executable
    let mut perms = fs::metadata(tmp_path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(tmp_path, perms)?;

    // Move to install location (requires sudo)
    let status = Command::new("sudo")
        .args(["mv", tmp_path, install_path])
        .status()
        .context("Failed to install cloud-hypervisor")?;

    if !status.success() {
        anyhow::bail!("Failed to move cloud-hypervisor to {}", install_path);
    }

    info!("Installed Cloud Hypervisor to {}", install_path);
    Ok(())
}

fn install_firmware() -> Result<()> {
    let firmware_dir = "/usr/share/cloud-hypervisor";
    let firmware_path = format!("{}/hypervisor-fw", firmware_dir);

    if std::path::Path::new(&firmware_path).exists() {
        info!("Firmware already installed at {}", firmware_path);
        return Ok(());
    }

    info!("Downloading hypervisor firmware...");

    // Create directory
    let status = Command::new("sudo")
        .args(["mkdir", "-p", firmware_dir])
        .status()
        .context("Failed to create firmware directory")?;

    if !status.success() {
        anyhow::bail!("Failed to create directory {}", firmware_dir);
    }

    // Download firmware
    let firmware_url = "https://github.com/cloud-hypervisor/rust-hypervisor-firmware/releases/download/0.4.2/hypervisor-fw";

    let status = Command::new("sudo")
        .args(["curl", "-L", "-o", &firmware_path, firmware_url])
        .status()
        .context("Failed to download firmware")?;

    if !status.success() {
        anyhow::bail!("Failed to download hypervisor firmware");
    }

    info!("Installed firmware to {}", firmware_path);
    Ok(())
}

fn install_virtiofsd() -> Result<()> {
    info!("Installing virtiofsd...");

    // Check if already installed
    let output = Command::new("which").arg("virtiofsd").output();
    if let Ok(out) = output {
        if out.status.success() {
            let path = String::from_utf8_lossy(&out.stdout);
            info!("virtiofsd already installed at {}", path.trim());
            return Ok(());
        }
    }

    // Try apt-get install first (Ubuntu/Debian)
    info!("Attempting to install virtiofsd via apt...");
    let status = Command::new("sudo")
        .args(["apt-get", "update"])
        .status();

    if status.is_ok() {
        let status = Command::new("sudo")
            .args(["apt-get", "install", "-y", "virtiofsd"])
            .status()
            .context("Failed to run apt-get install")?;

        if status.success() {
            info!("virtiofsd installed via apt (located at /usr/libexec/virtiofsd)");
            return Ok(());
        }
    }

    // Fallback: try to install from Rust crates
    warn!("apt install failed, trying cargo install...");
    let status = Command::new("cargo")
        .args(["install", "virtiofsd"])
        .status();

    if let Ok(s) = status {
        if s.success() {
            info!("virtiofsd installed via cargo");
            return Ok(());
        }
    }

    anyhow::bail!(
        "Failed to install virtiofsd automatically.\n\
         Please install manually:\n\
         - Ubuntu/Debian: sudo apt-get install virtiofsd\n\
         - Cargo: cargo install virtiofsd\n\
         - Or download from: https://gitlab.com/virtio-fs/virtiofsd/-/releases"
    );
}

fn verify_installation() -> Result<()> {
    info!("Verifying installation...");

    // Check cloud-hypervisor
    let output = Command::new("cloud-hypervisor")
        .arg("--version")
        .output()
        .context("Failed to run cloud-hypervisor --version")?;

    if output.status.success() {
        let version = String::from_utf8_lossy(&output.stdout);
        info!("Cloud Hypervisor: {}", version.trim());
    } else {
        warn!("Could not verify cloud-hypervisor version");
    }

    // Check virtiofsd
    let output = Command::new("virtiofsd")
        .arg("--version")
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let version = String::from_utf8_lossy(&out.stdout);
            info!("virtiofsd: {}", version.trim());
        }
        _ => {
            warn!("Could not verify virtiofsd version");
        }
    }

    Ok(())
}
