### Cloud Hypervisor CLI tool for VM management

Usage: chyp [OPTIONS] COMMAND


Commands:

```
  install        Install Cloud Hypervisor and virtiofsd
  setup-network  Setup network bridge with internet access
  run            Run virtual machine
  stop           Stop running VM
  help           Print this message or the help of the given subcommand(s)
```

Options:

```
    --vm_name <VM_NAME>                VM name
    --image_url <IMAGE_URL>            Cloud image URL
    --cpus <CPUS>                      Number of CPUs
    --memory_size <MEMORY_SIZE>        Memory size in GB
    --disk_size <DISK_SIZE>            Disk size in GB
    --project_folder <PROJECT_FOLDER>  Project folder path (stores VM images and configs)
    --shared_folder <SHARED_FOLDER>    Shared folder path (shared with VM)
-h, --help                             Print help
-V, --version                          Print version
```