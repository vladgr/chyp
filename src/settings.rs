use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Default settings embedded at compile time
const DEFAULT_SETTINGS: &str = include_str!("../settings.json");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub user: String,
    pub vm_name: String,
    pub image_url: String,
    pub cpus: u32,
    pub memory_size: u32,
    pub disk_size: u32,
    pub project_folder: String,
    pub shared_folder: String,
}

impl Default for Settings {
    fn default() -> Self {
        serde_json::from_str(DEFAULT_SETTINGS).expect("Invalid embedded settings.json")
    }
}

impl Settings {
    /// Load default settings and apply CLI overrides
    pub fn with_overrides(
        user: Option<String>,
        vm_name: Option<String>,
        image_url: Option<String>,
        cpus: Option<u32>,
        memory_size: Option<u32>,
        disk_size: Option<u32>,
        project_folder: Option<String>,
        shared_folder: Option<String>,
    ) -> Self {
        let mut settings = Self::default();

        if let Some(v) = user {
            settings.user = v;
        }
        if let Some(v) = vm_name {
            settings.vm_name = v;
        }
        if let Some(v) = image_url {
            settings.image_url = v;
        }
        if let Some(v) = cpus {
            settings.cpus = v;
        }
        if let Some(v) = memory_size {
            settings.memory_size = v;
        }
        if let Some(v) = disk_size {
            settings.disk_size = v;
        }
        if let Some(v) = project_folder {
            settings.project_folder = v;
        }
        if let Some(v) = shared_folder {
            settings.shared_folder = v;
        }

        settings
    }

    /// Get the project directory (base directory for all VM files)
    pub fn base_dir(&self) -> PathBuf {
        PathBuf::from(&self.project_folder)
    }

    /// Get the images directory
    pub fn images_dir(&self) -> PathBuf {
        self.base_dir().join("images")
    }

    /// Get the VMs directory
    pub fn vms_dir(&self) -> PathBuf {
        self.base_dir().join("vms")
    }

    /// Get the shared folder directory (shared with VM)
    pub fn shared_dir(&self) -> PathBuf {
        PathBuf::from(&self.shared_folder)
    }

    /// Get the VM-specific directory
    pub fn vm_dir(&self) -> PathBuf {
        self.vms_dir().join(&self.vm_name)
    }
}
