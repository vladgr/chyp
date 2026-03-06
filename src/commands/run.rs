use anyhow::{Context, Result};
use log::{info, warn};
use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::settings::Settings;

const TAP_NAME: &str = "chyp-tap0";
const VM_USER: &str = "ubuntu";
const VM_IP: &str = "192.168.100.10";

pub fn execute(settings: &Settings) -> Result<()> {
    info!("Starting VM: {}", settings.vm_name);
    info!("Configuration: {} CPUs, {} GB RAM", settings.cpus, settings.memory_size);

    // Ensure directories exist
    for dir in [settings.base_dir(), settings.vm_dir(), settings.images_dir(), settings.shared_dir()] {
        fs::create_dir_all(&dir)?;
    }

    // Download image if not present
    let image_path = download_image(settings)?;

    // Create overlay disk
    let disk_path = create_overlay_disk(settings, &image_path)?;

    // Extract kernel and initrd for direct boot
    let (kernel_path, initrd_path) = extract_kernel_initrd(settings, &disk_path)?;

    // Create cloud-init ISO
    let cloudinit_path = create_cloud_init(settings)?;

    // Ensure user ownership of .chyp directory
    crate::chown_chyp_dir()?;

    // Start virtiofsd
    let virtiofsd_socket = start_virtiofsd(settings)?;

    // Run cloud-hypervisor
    run_cloud_hypervisor(settings, &disk_path, &cloudinit_path, &virtiofsd_socket, &kernel_path, &initrd_path)?;

    Ok(())
}

fn download_image(settings: &Settings) -> Result<std::path::PathBuf> {
    let image_name = settings
        .image_url
        .split('/')
        .last()
        .unwrap_or("cloud-image.img");

    // Use .qcow2 extension for converted image
    let base_name = image_name.trim_end_matches(".img").trim_end_matches(".qcow2");
    let converted_name = format!("{}-uncompressed.qcow2", base_name);
    let converted_path = settings.images_dir().join(&converted_name);

    if converted_path.exists() {
        info!("Image already exists: {:?}", converted_path);
        return Ok(converted_path);
    }

    let download_path = settings.images_dir().join(image_name);

    if !download_path.exists() {
        info!("Downloading image from {}...", settings.image_url);

        let status = Command::new("curl")
            .args([
                "-L",
                "-o",
                download_path.to_str().unwrap(),
                "--progress-bar",
                &settings.image_url,
            ])
            .status()
            .context("Failed to execute curl")?;

        if !status.success() {
            anyhow::bail!("Failed to download image");
        }

        info!("Image downloaded to {:?}", download_path);
    }

    // Convert to uncompressed qcow2 (Cloud Hypervisor doesn't support compressed)
    info!("Converting image to uncompressed qcow2...");
    let status = Command::new("qemu-img")
        .args([
            "convert",
            "-O", "qcow2",
            download_path.to_str().unwrap(),
            converted_path.to_str().unwrap(),
        ])
        .status()
        .context("Failed to convert image")?;

    if !status.success() {
        anyhow::bail!("Failed to convert image to uncompressed qcow2");
    }

    info!("Image converted to {:?}", converted_path);

    // Remove original compressed image to save space
    let _ = fs::remove_file(&download_path);

    Ok(converted_path)
}

fn create_overlay_disk(settings: &Settings, base_image: &Path) -> Result<std::path::PathBuf> {
    let disk_path = settings.vm_dir().join("disk.qcow2");

    if disk_path.exists() {
        info!("Overlay disk already exists: {:?}", disk_path);
        return Ok(disk_path);
    }

    info!("Creating VM disk (copying base image)...");

    // Copy base image instead of overlay to avoid compression issues with Cloud Hypervisor
    let status = Command::new("cp")
        .args([
            base_image.to_str().unwrap(),
            disk_path.to_str().unwrap(),
        ])
        .status()
        .context("Failed to copy disk image")?;

    if !status.success() {
        anyhow::bail!("Failed to create overlay disk");
    }

    // Resize disk to configured size
    info!("Resizing disk to {} GB...", settings.disk_size);
    let status = Command::new("qemu-img")
        .args([
            "resize",
            disk_path.to_str().unwrap(),
            &format!("{}G", settings.disk_size),
        ])
        .status()
        .context("Failed to resize disk")?;

    if !status.success() {
        warn!("Failed to resize disk, continuing with original size");
    }

    info!("Disk created: {:?} ({}GB)", disk_path, settings.disk_size);
    Ok(disk_path)
}

fn extract_kernel_initrd(settings: &Settings, disk_path: &Path) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
    let vm_dir = settings.vm_dir();
    let kernel_path = vm_dir.join("vmlinuz");
    let initrd_path = vm_dir.join("initrd.img");

    if kernel_path.exists() && initrd_path.exists() {
        info!("Kernel and initrd already extracted");
        return Ok((kernel_path, initrd_path));
    }

    info!("Extracting kernel and initrd from disk image...");

    // Use virt-ls to find kernel files
    let output = Command::new("sudo")
        .args(["virt-ls", "-a", disk_path.to_str().unwrap(), "/boot/"])
        .output()
        .context("Failed to run virt-ls. Install libguestfs-tools: sudo apt-get install libguestfs-tools")?;

    if !output.status.success() {
        anyhow::bail!("Failed to list /boot directory. Install libguestfs-tools: sudo apt-get install libguestfs-tools");
    }

    let files = String::from_utf8_lossy(&output.stdout);
    let mut kernel_name = None;
    let mut initrd_name = None;

    for line in files.lines() {
        if line.starts_with("vmlinuz-") && !line.ends_with(".old") && kernel_name.is_none() {
            kernel_name = Some(line.to_string());
        }
        if line.starts_with("initrd.img-") && !line.ends_with(".old") && initrd_name.is_none() {
            initrd_name = Some(line.to_string());
        }
    }

    let kernel_name = kernel_name.context("Could not find kernel in /boot")?;
    let initrd_name = initrd_name.context("Could not find initrd in /boot")?;

    info!("Found kernel: {}", kernel_name);
    info!("Found initrd: {}", initrd_name);

    // Extract kernel
    let status = Command::new("sudo")
        .args([
            "virt-copy-out",
            "-a", disk_path.to_str().unwrap(),
            &format!("/boot/{}", kernel_name),
            vm_dir.to_str().unwrap(),
        ])
        .status()
        .context("Failed to extract kernel")?;

    if !status.success() {
        anyhow::bail!("Failed to extract kernel from image");
    }

    // Rename to vmlinuz
    fs::rename(vm_dir.join(&kernel_name), &kernel_path)?;

    // Extract initrd
    let status = Command::new("sudo")
        .args([
            "virt-copy-out",
            "-a", disk_path.to_str().unwrap(),
            &format!("/boot/{}", initrd_name),
            vm_dir.to_str().unwrap(),
        ])
        .status()
        .context("Failed to extract initrd")?;

    if !status.success() {
        anyhow::bail!("Failed to extract initrd from image");
    }

    // Rename to initrd.img
    fs::rename(vm_dir.join(&initrd_name), &initrd_path)?;

    info!("Extracted kernel and initrd");

    Ok((kernel_path, initrd_path))
}

fn create_cloud_init(settings: &Settings) -> Result<std::path::PathBuf> {
    let vm_dir = settings.vm_dir();
    let iso_path = vm_dir.join("cloud-init.iso");

    // Read SSH public key
    let ssh_key = read_ssh_public_key()?;

    // Create cloud-init directory
    let ci_dir = vm_dir.join("cloud-init");
    fs::create_dir_all(&ci_dir)?;

    // Create meta-data
    let meta_data = format!(
        "instance-id: {}\nlocal-hostname: {}\n",
        settings.vm_name, settings.vm_name
    );
    fs::write(ci_dir.join("meta-data"), meta_data)?;

    // Create user-data
    let user_data = format!(
        r#"#cloud-config
hostname: {}
manage_etc_hosts: true

ssh_pwauth: true
password: ubuntu
chpasswd:
  expire: false
  list:
    - ubuntu:ubuntu

users:
  - name: {}
    sudo: ALL=(ALL) NOPASSWD:ALL
    shell: /bin/bash
    lock_passwd: false
    passwd: $6$rounds=4096$xyz$LZrAspP0/WTXdThXlHkByL0KCAvyTi9e6HLFxwFqINFnZdH.LJH8gfLlnBPvkpvT5T21yMYiSj9zaH.FLif.q0
    ssh_authorized_keys:
      - {}

write_files:
  - path: /etc/netplan/99-chyp.yaml
    content: |
      network:
        version: 2
        ethernets:
          enp0s3:
            addresses: [{}/24]
            routes:
              - to: default
                via: 192.168.100.1
            nameservers:
              addresses: [8.8.8.8, 8.8.4.4]

bootcmd:
  - systemctl disable systemd-networkd-wait-online.service
  - systemctl mask systemd-networkd-wait-online.service

runcmd:
  - |
    # Find and configure network interface
    for iface in $(ls /sys/class/net/ | grep -E '^en|^eth'); do
      ip addr add {}/24 dev $iface 2>/dev/null || true
      ip link set $iface up
      ip route add default via 192.168.100.1 2>/dev/null || true
    done
    echo "nameserver 8.8.8.8" > /etc/resolv.conf
  - mkdir -p /mnt/shared
  - echo "VM {} is ready!" > /var/log/chyp-ready.log
"#,
        settings.vm_name, VM_USER, ssh_key, VM_IP, VM_IP, settings.vm_name
    );
    fs::write(ci_dir.join("user-data"), user_data)?;

    // Create network-config
    let network_config = format!(
        r#"version: 2
ethernets:
  enp0s3:
    addresses:
      - {}/24
    gateway4: 192.168.100.1
    nameservers:
      addresses:
        - 8.8.8.8
        - 8.8.4.4
"#,
        VM_IP
    );
    fs::write(ci_dir.join("network-config"), network_config)?;

    info!("Creating cloud-init ISO...");

    // Create ISO using cloud-localds or genisoimage
    let status = Command::new("cloud-localds")
        .args([
            iso_path.to_str().unwrap(),
            ci_dir.join("user-data").to_str().unwrap(),
            ci_dir.join("meta-data").to_str().unwrap(),
        ])
        .status();

    if status.is_err() || !status.unwrap().success() {
        // Fallback to genisoimage
        info!("cloud-localds not found, using genisoimage...");
        let status = Command::new("genisoimage")
            .args([
                "-output",
                iso_path.to_str().unwrap(),
                "-volid", "cidata",
                "-joliet",
                "-rock",
                ci_dir.join("user-data").to_str().unwrap(),
                ci_dir.join("meta-data").to_str().unwrap(),
                ci_dir.join("network-config").to_str().unwrap(),
            ])
            .status()
            .context("Failed to create cloud-init ISO. Install cloud-image-utils or genisoimage.")?;

        if !status.success() {
            anyhow::bail!("Failed to create cloud-init ISO");
        }
    }

    info!("Cloud-init ISO created: {:?}", iso_path);
    Ok(iso_path)
}

fn read_ssh_public_key() -> Result<String> {
    let home = std::env::var("HOME").context("HOME not set")?;
    let key_paths = [
        format!("{}/.ssh/id_ed25519.pub", home),
        format!("{}/.ssh/id_rsa.pub", home),
        format!("{}/.ssh/id_ecdsa.pub", home),
    ];

    for path in &key_paths {
        if let Ok(key) = fs::read_to_string(path) {
            info!("Using SSH key from {}", path);
            return Ok(key.trim().to_string());
        }
    }

    warn!("No SSH public key found. Generating new key pair...");
    generate_ssh_key()
}

fn generate_ssh_key() -> Result<String> {
    let home = std::env::var("HOME").context("HOME not set")?;
    let key_path = format!("{}/.ssh/id_ed25519", home);
    let pub_key_path = format!("{}.pub", key_path);

    // Create .ssh directory
    fs::create_dir_all(format!("{}/.ssh", home))?;

    let status = Command::new("ssh-keygen")
        .args(["-t", "ed25519", "-f", &key_path, "-N", "", "-q"])
        .status()
        .context("Failed to generate SSH key")?;

    if !status.success() {
        anyhow::bail!("Failed to generate SSH key pair");
    }

    // Set proper permissions
    let mut perms = fs::metadata(&key_path)?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(&key_path, perms)?;

    let key = fs::read_to_string(&pub_key_path)?;
    info!("Generated new SSH key: {}", pub_key_path);
    Ok(key.trim().to_string())
}

/// Find virtiofsd binary in common locations
fn find_virtiofsd() -> Result<String> {
    let paths = [
        "/usr/local/bin/virtiofsd",
        "/usr/libexec/virtiofsd",
        "/usr/lib/qemu/virtiofsd",
        "virtiofsd", // PATH lookup
    ];

    for path in &paths {
        if path.starts_with('/') {
            if std::path::Path::new(path).exists() {
                return Ok(path.to_string());
            }
        } else {
            // Check if in PATH
            if Command::new("which").arg(path).output().map(|o| o.status.success()).unwrap_or(false) {
                return Ok(path.to_string());
            }
        }
    }

    anyhow::bail!("virtiofsd not found. Please run 'chyp install' first.");
}

fn start_virtiofsd(settings: &Settings) -> Result<String> {
    let socket_path = format!("/tmp/virtiofsd-{}.sock", settings.vm_name);
    let pid_path = format!("{}.pid", socket_path);

    // Kill any existing virtiofsd for this VM
    if let Ok(pid_str) = fs::read_to_string(&pid_path) {
        if let Ok(pid) = pid_str.trim().parse::<i32>() {
            info!("Killing existing virtiofsd (pid {})", pid);
            unsafe { libc::kill(pid, libc::SIGTERM); }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    // Remove old socket and pid file
    let _ = fs::remove_file(&socket_path);
    let _ = fs::remove_file(&pid_path);

    let virtiofsd_path = find_virtiofsd()?;
    info!("Starting virtiofsd ({}) for shared folder {:?}...", virtiofsd_path, settings.shared_dir());

    // Start virtiofsd in background
    // Try new-style arguments first (virtiofsd 1.x from rust-vmm)
    let mut child = Command::new("sudo")
        .args([
            &virtiofsd_path,
            &format!("--socket-path={}", socket_path),
            &format!("--shared-dir={}", settings.shared_dir().display()),
            "--cache=always",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to start virtiofsd")?;

    // Give it a moment to start
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Check if it's still running
    match child.try_wait() {
        Ok(Some(status)) => {
            if !status.success() {
                let stderr = child.stderr.take();
                let err_msg = if let Some(mut err) = stderr {
                    let mut buf = String::new();
                    let _ = err.read_to_string(&mut buf);
                    buf
                } else {
                    "unknown error".to_string()
                };
                anyhow::bail!("virtiofsd exited with error: {}", err_msg);
            }
        }
        Ok(None) => {
            info!("virtiofsd started with socket: {}", socket_path);
        }
        Err(e) => {
            anyhow::bail!("Failed to check virtiofsd status: {}", e);
        }
    }

    Ok(socket_path)
}

fn run_cloud_hypervisor(
    settings: &Settings,
    disk_path: &Path,
    cloudinit_path: &Path,
    virtiofsd_socket: &str,
    kernel_path: &Path,
    initrd_path: &Path,
) -> Result<()> {
    info!("Starting Cloud Hypervisor...");

    let memory_mb = settings.memory_size * 1024;

    let args = vec![
        "--cpus".to_string(),
        format!("boot={}", settings.cpus),
        "--memory".to_string(),
        format!("size={}M,shared=on", memory_mb),
        "--disk".to_string(),
        format!("path={}", disk_path.display()),
        format!("path={}", cloudinit_path.display()),
        "--net".to_string(),
        format!("tap={},mac=52:54:00:12:34:56", TAP_NAME),
        "--fs".to_string(),
        format!("tag=shared,socket={},num_queues=1,queue_size=512", virtiofsd_socket),
        "--serial".to_string(),
        "tty".to_string(),
        "--console".to_string(),
        "off".to_string(),
        // Direct kernel boot with initrd
        "--kernel".to_string(),
        kernel_path.to_str().unwrap().to_string(),
        "--initramfs".to_string(),
        initrd_path.to_str().unwrap().to_string(),
        "--cmdline".to_string(),
        "root=/dev/vda1 ro console=tty1 console=ttyS0".to_string(),
    ];

    info!("Starting VM...");
    info!("Exit console: Ctrl+A X");
    info!("");
    info!("SSH: ssh {}@{}", VM_USER, VM_IP);
    info!("Shared folder: /mnt/shared");
    info!("");

    let status = Command::new("sudo")
        .arg("cloud-hypervisor")
        .args(&args)
        .status()
        .context("Failed to start cloud-hypervisor")?;

    if !status.success() {
        anyhow::bail!("Cloud Hypervisor exited with error");
    }

    Ok(())
}
