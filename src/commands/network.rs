use anyhow::{Context, Result};
use log::{info, warn};
use std::process::Command;

const BRIDGE_NAME: &str = "chyp-br0";
const TAP_NAME: &str = "chyp-tap0";
const BRIDGE_IP: &str = "192.168.100.1/24";
const BRIDGE_SUBNET: &str = "192.168.100.0/24";

pub fn execute() -> Result<()> {
    info!("Setting up network for Cloud Hypervisor VMs...");

    // Enable IP forwarding
    enable_ip_forwarding()?;

    // Create bridge interface
    create_bridge()?;

    // Create TAP device
    create_tap()?;

    // Setup NAT/masquerading
    setup_nat()?;

    info!("Network setup completed successfully!");
    info!("Bridge: {} with IP {}", BRIDGE_NAME, BRIDGE_IP);
    info!("TAP device: {} attached to {}", TAP_NAME, BRIDGE_NAME);
    info!("VMs will get IPs in range 192.168.100.2-254 via DHCP or static config");

    Ok(())
}

fn enable_ip_forwarding() -> Result<()> {
    info!("Enabling IP forwarding...");

    let status = Command::new("sudo")
        .args(["sysctl", "-w", "net.ipv4.ip_forward=1"])
        .status()
        .context("Failed to enable IP forwarding")?;

    if !status.success() {
        anyhow::bail!("Failed to enable IP forwarding");
    }

    // Make persistent
    let sysctl_conf = "net.ipv4.ip_forward=1";
    let status = Command::new("sudo")
        .args([
            "sh",
            "-c",
            &format!(
                "grep -q 'net.ipv4.ip_forward=1' /etc/sysctl.conf || echo '{}' >> /etc/sysctl.conf",
                sysctl_conf
            ),
        ])
        .status();

    if status.is_err() {
        warn!("Could not make IP forwarding persistent in /etc/sysctl.conf");
    }

    Ok(())
}

fn create_bridge() -> Result<()> {
    info!("Creating bridge interface {}...", BRIDGE_NAME);

    // Check if bridge already exists
    let output = Command::new("ip")
        .args(["link", "show", BRIDGE_NAME])
        .output()?;

    if output.status.success() {
        info!("Bridge {} already exists", BRIDGE_NAME);
    } else {
        // Create bridge
        let status = Command::new("sudo")
            .args(["ip", "link", "add", "name", BRIDGE_NAME, "type", "bridge"])
            .status()
            .context("Failed to create bridge")?;

        if !status.success() {
            anyhow::bail!("Failed to create bridge {}", BRIDGE_NAME);
        }
    }

    // Set bridge up
    let status = Command::new("sudo")
        .args(["ip", "link", "set", BRIDGE_NAME, "up"])
        .status()
        .context("Failed to bring bridge up")?;

    if !status.success() {
        anyhow::bail!("Failed to bring bridge {} up", BRIDGE_NAME);
    }

    // Assign IP to bridge
    // First check if IP is already assigned
    let output = Command::new("ip")
        .args(["addr", "show", BRIDGE_NAME])
        .output()?;

    let output_str = String::from_utf8_lossy(&output.stdout);
    if !output_str.contains("192.168.100.1") {
        let status = Command::new("sudo")
            .args(["ip", "addr", "add", BRIDGE_IP, "dev", BRIDGE_NAME])
            .status()
            .context("Failed to assign IP to bridge")?;

        if !status.success() {
            anyhow::bail!("Failed to assign IP {} to bridge {}", BRIDGE_IP, BRIDGE_NAME);
        }
    }

    info!("Bridge {} configured with IP {}", BRIDGE_NAME, BRIDGE_IP);
    Ok(())
}

fn create_tap() -> Result<()> {
    info!("Creating TAP device {}...", TAP_NAME);

    // Check if TAP already exists
    let output = Command::new("ip")
        .args(["link", "show", TAP_NAME])
        .output()?;

    if output.status.success() {
        info!("TAP device {} already exists", TAP_NAME);
    } else {
        // Create TAP device
        let status = Command::new("sudo")
            .args(["ip", "tuntap", "add", "dev", TAP_NAME, "mode", "tap"])
            .status()
            .context("Failed to create TAP device")?;

        if !status.success() {
            anyhow::bail!("Failed to create TAP device {}", TAP_NAME);
        }
    }

    // Set TAP up
    let status = Command::new("sudo")
        .args(["ip", "link", "set", TAP_NAME, "up"])
        .status()
        .context("Failed to bring TAP up")?;

    if !status.success() {
        anyhow::bail!("Failed to bring TAP device {} up", TAP_NAME);
    }

    // Add TAP to bridge
    let status = Command::new("sudo")
        .args(["ip", "link", "set", TAP_NAME, "master", BRIDGE_NAME])
        .status()
        .context("Failed to add TAP to bridge")?;

    if !status.success() {
        anyhow::bail!("Failed to add {} to bridge {}", TAP_NAME, BRIDGE_NAME);
    }

    info!("TAP device {} attached to bridge {}", TAP_NAME, BRIDGE_NAME);
    Ok(())
}

fn setup_nat() -> Result<()> {
    info!("Setting up NAT/masquerading...");

    // Get default interface for internet
    let output = Command::new("ip")
        .args(["route", "get", "8.8.8.8"])
        .output()
        .context("Failed to get default route")?;

    let output_str = String::from_utf8_lossy(&output.stdout);
    let default_iface = output_str
        .split_whitespace()
        .skip_while(|&s| s != "dev")
        .nth(1)
        .unwrap_or("eth0");

    info!("Using {} as default interface for NAT", default_iface);

    // Setup iptables MASQUERADE
    let status = Command::new("sudo")
        .args([
            "iptables",
            "-t", "nat",
            "-C", "POSTROUTING",
            "-s", BRIDGE_SUBNET,
            "-o", default_iface,
            "-j", "MASQUERADE",
        ])
        .status();

    // If rule doesn't exist, add it
    if status.is_err() || !status.unwrap().success() {
        let status = Command::new("sudo")
            .args([
                "iptables",
                "-t", "nat",
                "-A", "POSTROUTING",
                "-s", BRIDGE_SUBNET,
                "-o", default_iface,
                "-j", "MASQUERADE",
            ])
            .status()
            .context("Failed to setup NAT")?;

        if !status.success() {
            anyhow::bail!("Failed to setup iptables MASQUERADE rule");
        }
    }

    // Allow forwarding from bridge
    let status = Command::new("sudo")
        .args([
            "iptables",
            "-C", "FORWARD",
            "-i", BRIDGE_NAME,
            "-o", default_iface,
            "-j", "ACCEPT",
        ])
        .status();

    if status.is_err() || !status.unwrap().success() {
        Command::new("sudo")
            .args([
                "iptables",
                "-A", "FORWARD",
                "-i", BRIDGE_NAME,
                "-o", default_iface,
                "-j", "ACCEPT",
            ])
            .status()
            .context("Failed to add forward rule")?;
    }

    // Allow return traffic
    let status = Command::new("sudo")
        .args([
            "iptables",
            "-C", "FORWARD",
            "-i", default_iface,
            "-o", BRIDGE_NAME,
            "-m", "state",
            "--state", "RELATED,ESTABLISHED",
            "-j", "ACCEPT",
        ])
        .status();

    if status.is_err() || !status.unwrap().success() {
        Command::new("sudo")
            .args([
                "iptables",
                "-A", "FORWARD",
                "-i", default_iface,
                "-o", BRIDGE_NAME,
                "-m", "state",
                "--state", "RELATED,ESTABLISHED",
                "-j", "ACCEPT",
            ])
            .status()
            .context("Failed to add return traffic rule")?;
    }

    info!("NAT configured: {} -> {} (masquerade)", BRIDGE_SUBNET, default_iface);
    Ok(())
}
