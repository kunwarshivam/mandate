# The environment's managed Kubernetes cluster: one pool of exactly two nodes, the version pinned,
# auto-upgrade off, the control plane's firewall on (DEC-2011 item 1, DEC-2012 item 5).

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
  description = "The tag the network module's firewall applies to."
  type        = string
}

variable "kubernetes_version" {
  description = "The exact DOKS version slug; it moves only by a reviewed change and an approved apply."
  type        = string

  validation {
    condition     = can(regex("^1[.][0-9]+[.][0-9]+-do[.][0-9]+$", var.kubernetes_version))
    error_message = "The version must be an exact DOKS slug such as 1.35.9-do.2, never a prefix or latest."
  }
}

variable "node_size" {
  type = string
}

variable "kube_api_allowed_addresses" {
  description = "The founder's CIDRs, the only ones the control plane's public endpoint admits besides a run's window. Production's root refuses an empty list; staging's may be empty (DEC-2028)."
  type        = list(string)
  sensitive   = true

  validation {
    condition = alltrue([
      for cidr in var.kube_api_allowed_addresses :
      can(cidrhost(cidr, 0)) && tonumber(split("/", cidr)[1]) >= (strcontains(cidr, ":") ? 48 : 24)
    ])
    error_message = "Each allowed CIDR must be valid and no wider than /24 (IPv4) or /48 (IPv6); a world range is refused."
  }
}

variable "bootstrap_runner_cidr" {
  description = "One run's GitHub runner, opened for the bootstrap and closed again (DEC-2028); empty otherwise."
  type        = string
  sensitive   = true
  default     = ""

  validation {
    condition = var.bootstrap_runner_cidr == "" || (
      can(regex("^((0|[1-9][0-9]{0,2})[.]){3}(0|[1-9][0-9]{0,2})/32$", var.bootstrap_runner_cidr)) &&
      can(cidrhost(var.bootstrap_runner_cidr, 0)) &&
      !can(regex("^(0|10|127|169[.]254|172[.](1[6-9]|2[0-9]|3[01])|192[.]168|100[.](6[4-9]|[7-9][0-9]|1[01][0-9]|12[0-7])|22[4-9]|2[3-5][0-9])[.]", var.bootstrap_runner_cidr))
    )
    error_message = "The runner window is empty or exactly one public IPv4 /32, written without leading zeros."
  }
}

locals {
  node_count = 2
  requested_addresses = concat(
    var.kube_api_allowed_addresses,
    var.bootstrap_runner_cidr == "" ? [] : [var.bootstrap_runner_cidr],
  )
  # With no CIDR at all the firewall stays on and admits only TEST-NET-1's 192.0.2.1, which no
  # host on the internet holds (RFC 5737), so an empty list never reads as "no restriction".
  admits_nobody     = ["192.0.2.1/32"]
  allowed_addresses = length(local.requested_addresses) > 0 ? local.requested_addresses : local.admits_nobody
}

data "digitalocean_kubernetes_versions" "offered" {}

resource "digitalocean_kubernetes_cluster" "this" {
  name                 = var.name
  region               = var.region
  version              = var.kubernetes_version
  vpc_uuid             = var.vpc_id
  auto_upgrade         = false
  surge_upgrade        = true
  ha                   = false
  registry_integration = false
  tags                 = [var.node_tag]

  node_pool {
    name       = "${var.name}-pool"
    size       = var.node_size
    node_count = local.node_count
    auto_scale = false
    tags       = [var.node_tag]
  }

  control_plane_firewall {
    enabled           = true
    allowed_addresses = local.allowed_addresses
  }

  lifecycle {
    precondition {
      condition     = contains(data.digitalocean_kubernetes_versions.offered.valid_versions, var.kubernetes_version)
      error_message = "DOKS does not offer ${var.kubernetes_version}; it offers: ${join(", ", data.digitalocean_kubernetes_versions.offered.valid_versions)}."
    }
  }
}

output "cluster_id" {
  value = digitalocean_kubernetes_cluster.this.id
}

output "endpoint" {
  value = digitalocean_kubernetes_cluster.this.endpoint
}

output "node_count" {
  value = local.node_count
}

output "region" {
  value = digitalocean_kubernetes_cluster.this.region
}
