# A customer production cell's managed Kubernetes cluster (DEC-2064 items 2 and 3, proposed and
# never applied until the founder approves its spend): the control plane highly available, a
# system pool for the cell's shared services, a workspace pool that only workspace processes
# tolerate, and a dedicated pool per business workspace that only that workspace tolerates (HLD §8,
# infrastructure §3.2). Every count is fixed, so the estimate is the bill; the version is pinned,
# auto-upgrade is off, and the control plane admits only the founder's CIDRs.

terraform {
  required_version = "1.13.1"

  required_providers {
    digitalocean = {
      source  = "digitalocean/digitalocean"
      version = "2.103.0"
    }
  }
}

variable "name" {
  type = string
}

variable "region" {
  type = string
}

variable "vpc_id" {
  type = string
}

variable "node_tag" {
  description = "The tag the network module's firewall applies to; every pool carries it."
  type        = string
}

variable "kubernetes_version" {
  description = "The exact DOKS version slug; it moves only by a reviewed change and an approved apply."
  type        = string

  validation {
    condition     = can(regex("^1[.][0-9]+[.][0-9]+-do[.][0-9]+$", var.kubernetes_version))
    error_message = "The version must be an exact DOKS slug such as 1.35.9-do.1, never a prefix or latest."
  }
}

variable "ha_control_plane" {
  description = "Customer production runs an HA control plane: the kill switch's Job is created through the Kubernetes API (DEC-2064 item 3)."
  type        = bool

  validation {
    condition     = var.ha_control_plane
    error_message = "A customer production cell runs an HA control plane (DEC-2064 item 3)."
  }
}

variable "system_pool" {
  description = "The cell's shared services: Flux, admission, CloudNativePG, the vault, monitoring, the tunnel connector."
  type = object({
    size  = string
    count = number
  })

  validation {
    condition     = var.system_pool.count >= 3
    error_message = "The system pool has at least three nodes, so the vault's Raft and the journal's three instances each keep a quorum through one node's loss."
  }
}

variable "workspace_pool" {
  description = "The shared retail workspace pool; its count holds every workspace with one node spare (DEC-2064 item 3)."
  type = object({
    size  = string
    count = number
  })

  validation {
    condition     = var.workspace_pool.count >= 2
    error_message = "The workspace pool has at least two nodes, so a node's workspaces have somewhere to restart."
  }
}

variable "dedicated_pools" {
  description = "A pool per business workspace, keyed by workspace id, that only that workspace's pods tolerate (HLD §8)."
  type = map(object({
    size  = string
    count = number
  }))
  default = {}

  validation {
    condition     = alltrue([for id, pool in var.dedicated_pools : can(regex("^[a-z0-9]{1,20}$", id)) && pool.count >= 1])
    error_message = "A dedicated pool is keyed by a workspace id of lower-case letters and digits and has at least one node."
  }
}

variable "kube_api_allowed_addresses" {
  description = "The founder's CIDRs, the only ones the control plane's public endpoint admits."
  type        = list(string)
  sensitive   = true

  validation {
    condition = length(var.kube_api_allowed_addresses) > 0 && alltrue([
      for cidr in var.kube_api_allowed_addresses :
      can(cidrhost(cidr, 0)) && tonumber(split("/", cidr)[1]) >= (strcontains(cidr, ":") ? 48 : 24)
    ])
    error_message = "At least one CIDR, each valid and no wider than /24 (IPv4) or /48 (IPv6); a world range is refused."
  }
}

data "digitalocean_kubernetes_versions" "offered" {}

resource "digitalocean_kubernetes_cluster" "this" {
  name                 = var.name
  region               = var.region
  version              = var.kubernetes_version
  vpc_uuid             = var.vpc_id
  auto_upgrade         = false
  surge_upgrade        = true
  ha                   = var.ha_control_plane
  registry_integration = false
  tags                 = [var.node_tag]

  maintenance_policy {
    day        = "sunday"
    start_time = "10:00"
  }

  node_pool {
    name       = "${var.name}-system"
    size       = var.system_pool.size
    node_count = var.system_pool.count
    auto_scale = false
    tags       = [var.node_tag]
    labels = {
      "mandate.dev/pool" = "system"
    }
  }

  control_plane_firewall {
    enabled           = true
    allowed_addresses = var.kube_api_allowed_addresses
  }

  lifecycle {
    precondition {
      condition     = contains(data.digitalocean_kubernetes_versions.offered.valid_versions, var.kubernetes_version)
      error_message = "DOKS does not offer ${var.kubernetes_version}; it offers: ${join(", ", data.digitalocean_kubernetes_versions.offered.valid_versions)}."
    }
  }
}

resource "digitalocean_kubernetes_node_pool" "workspaces" {
  cluster_id = digitalocean_kubernetes_cluster.this.id
  name       = "${var.name}-workspaces"
  size       = var.workspace_pool.size
  node_count = var.workspace_pool.count
  auto_scale = false
  tags       = [var.node_tag]
  labels = {
    "mandate.dev/pool" = "workspaces"
  }

  taint {
    key    = "mandate.dev/pool"
    value  = "workspaces"
    effect = "NoSchedule"
  }
}

resource "digitalocean_kubernetes_node_pool" "dedicated" {
  for_each   = var.dedicated_pools
  cluster_id = digitalocean_kubernetes_cluster.this.id
  name       = "${var.name}-ws-${each.key}"
  size       = each.value.size
  node_count = each.value.count
  auto_scale = false
  tags       = [var.node_tag]
  labels = {
    "mandate.dev/pool"      = "dedicated"
    "mandate.dev/workspace" = each.key
  }

  taint {
    key    = "mandate.dev/workspace"
    value  = each.key
    effect = "NoSchedule"
  }
}

output "cluster_id" {
  value = digitalocean_kubernetes_cluster.this.id
}

output "endpoint" {
  value = digitalocean_kubernetes_cluster.this.endpoint
}

output "region" {
  value = digitalocean_kubernetes_cluster.this.region
}

output "pools" {
  description = "Every pool's size and count, for the estimate."
  value = concat(
    [var.system_pool, var.workspace_pool],
    [for id, pool in var.dedicated_pools : pool],
  )
}
