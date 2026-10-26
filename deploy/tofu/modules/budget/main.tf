# The environment's monthly estimate and its share of DEC-2011's cap (DEC-2012 item 7). The node
# price, and the regions that offer the size, are DigitalOcean's own size list; Spaces and block
# storage are list prices. The plan fails when the size is not offered in the cluster's region, or
# when the estimate passes the cap. The 80% billing alert itself is the founder's step: no
# DigitalOcean API sets one.

terraform {
  required_version = "1.13.1"

  required_providers {
    digitalocean = {
      source  = "digitalocean/digitalocean"
      version = "2.103.0"
    }
  }
}

variable "node_size" {
  type = string
}

variable "region" {
  description = "The cluster's region, where DigitalOcean must offer the node size."
  type        = string
}

variable "node_count" {
  type = number
}

variable "spaces_monthly_usd" {
  description = "The Spaces subscription; it is per account, but each environment counts it so neither undercounts."
  type        = number
}

variable "block_storage_gb" {
  description = "The volumes the cluster's claims are expected to take."
  type        = number
}

variable "block_storage_usd_per_gb" {
  type    = number
  default = 0.10
}

variable "cap_usd" {
  description = "This environment's share of the monthly cap."
  type        = number
}

variable "cap_reason" {
  description = "The decision that sets the cap, which the refusal names."
  type        = string
  default     = "DEC-2011 item 2"
}

data "digitalocean_sizes" "node" {
  filter {
    key    = "slug"
    values = [var.node_size]
  }
}

data "digitalocean_sizes" "region" {
  filter {
    key    = "regions"
    values = [var.region]
  }
}

locals {
  node = try(data.digitalocean_sizes.node.sizes[0], null)
  alternatives = local.node == null ? [] : sort([
    for size in data.digitalocean_sizes.region.sizes : "${size.slug} (${size.price_monthly} USD)"
    if size.available && contains(size.regions, var.region) && size.vcpus == local.node.vcpus && size.memory == local.node.memory
  ])
  node_prices  = [for size in data.digitalocean_sizes.node.sizes : size.price_monthly]
  node_regions = sort(distinct(flatten([for size in data.digitalocean_sizes.node.sizes : size.regions if size.available])))
  estimate     = (length(local.node_prices) == 1 ? local.node_prices[0] : 0) * var.node_count + var.spaces_monthly_usd + var.block_storage_gb * var.block_storage_usd_per_gb
}

resource "terraform_data" "cap" {
  input = local.estimate

  lifecycle {
    precondition {
      condition     = length(local.node_prices) == 1
      error_message = "DigitalOcean lists no single size ${var.node_size}, so its price is unknown."
    }

    precondition {
      condition     = contains(local.node_regions, var.region)
      error_message = "DigitalOcean does not offer ${var.node_size} in ${var.region}; it offers it in: ${join(", ", local.node_regions)}. In ${var.region} it offers these sizes of the same vCPUs and memory: ${length(local.alternatives) == 0 ? "none" : join(", ", local.alternatives)} (DEC-2044 item 2, DEC-2047)."
    }

    precondition {
      condition     = local.estimate <= var.cap_usd
      error_message = "The monthly estimate of ${local.estimate} USD passes this environment's cap of ${var.cap_usd} USD (${var.cap_reason})."
    }
  }
}

output "estimate_usd" {
  value = local.estimate
}
