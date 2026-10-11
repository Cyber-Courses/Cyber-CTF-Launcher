
terraform {
  required_version = ">= 1.6"
  backend "local" {}
  required_providers {
    proxmox = {
      source  = "bpg/proxmox"
      version = "~> 0.115"
    }
  }
}

provider "proxmox" {
  endpoint = var.proxmox_endpoint
  insecure = var.proxmox_insecure
  # Token mode (username is a token id) uses api_token; otherwise username/password.
  api_token = var.proxmox_api_token != "" ? var.proxmox_api_token : null
  username  = var.proxmox_api_token != "" ? null : var.proxmox_username
  password  = var.proxmox_api_token != "" ? null : var.proxmox_password
  ssh {
    agent    = false
    username = coalesce(var.proxmox_ssh_username, split("@", var.proxmox_username)[0])
    # With a key file (token hosts), authenticate by key; otherwise by password.
    password    = var.proxmox_ssh_private_key_file != "" ? null : var.proxmox_password
    private_key = var.proxmox_ssh_private_key_file != "" ? file(var.proxmox_ssh_private_key_file) : null
    dynamic "node" {
      for_each = var.proxmox_ssh_address == "" ? [] : [var.proxmox_ssh_address]
      content {
        name    = var.node
        address = node.value
      }
    }
  }
}

locals {
  name = "cyberctf-selftest"
}

resource "proxmox_download_file" "debian" {
  node_name           = var.node
  datastore_id        = var.image_datastore
  content_type        = "iso"
  url                 = "https://cloud.debian.org/images/cloud/bookworm/latest/debian-12-genericcloud-amd64.qcow2"
  file_name           = "cyberctf-debian-12-genericcloud-amd64.img"
  overwrite           = false
  overwrite_unmanaged = true
}

resource "proxmox_virtual_environment_file" "user_data" {
  node_name    = var.node
  datastore_id = var.snippets_datastore
  content_type = "snippets"
  source_raw {
    file_name = "${local.name}-user-data.yaml"
    data      = <<-EOT
      #cloud-config
      hostname: cyberctf-selftest
      ssh_authorized_keys:
        - ${var.ssh_public_key}
      packages:
        - qemu-guest-agent
      runcmd:
        - [systemctl, enable, --now, qemu-guest-agent]
    EOT
  }
}

resource "proxmox_virtual_environment_vm" "labhost" {
  name      = local.name
  node_name = var.node
  tags      = ["cyberctf", "selftest"]
  on_boot   = false

  agent {
    enabled = true
  }
  cpu {
    cores = var.cores
    type  = var.cpu_type
  }
  memory {
    dedicated = var.memory_mb
  }
  disk {
    datastore_id = var.datastore
    file_id      = proxmox_download_file.debian.id
    interface    = "virtio0"
    size         = var.disk_gb
    discard      = "on"
  }
  network_device {
    bridge = var.uplink_bridge
  }
  operating_system {
    type = "l26"
  }
  serial_device {}
  initialization {
    datastore_id      = var.datastore
    user_data_file_id = proxmox_virtual_environment_file.user_data.id
    ip_config {
      ipv4 {
        address = "dhcp"
      }
    }
  }
}
