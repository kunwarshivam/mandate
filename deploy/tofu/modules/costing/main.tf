# A customer production cell's monthly estimate (DEC-2064 items 1 and 9, proposed). Node prices, and the regions that offer each size, are DigitalOcean's own size
# list at plan time. Everything else is a list price read on 2026-10-26 and cited in
# docs/design/production-cell.md §6: DOKS's HA control plane, block storage, Spaces, R2 Standard
# storage and Class A operations, Cloudflare Zero Trust seats past the free plan's 50, and uptime
# checks past the first. No free tier is credited that staging may already use. The root refuses
# to plan an estimate past the founder's approved amount, which is 0 until the founder approves a
# spend (DEC-79), so nothing here can be applied before then.

terraform {
  required_version = "1.13.1"

  required_providers {
    digitalocean = {
      source  = "digitalocean/digitalocean"
      version = "2.103.0"
    }
  }
}

variable "region" {
  description = "The cluster's region, where DigitalOcean must offer every pool's size."
  type        = string
}

variable "pools" {
  description = "Every node pool's size and count."
  type = list(object({
    size  = string
    count = number
  }))
}

variable "ha_control_plane" {
  type = bool
}

variable "block_storage_gb" {
  type = number
}

variable "r2_storage_gb" {
  description = "The archive buckets' expected size, journal and vault together."
  type        = number
}

variable "r2_class_a_millions" {
  description = "The archive buckets' expected writes a month, in millions of Class A operations."
  type        = number
}

variable "access_users" {
  description = "The distinct people Access admits."
  type        = number
}

variable "uptime_checks" {
  type = number
}

variable "prices" {
  description = "List prices read on 2026-10-26 (docs/design/production-cell.md §6); a price change is a reviewed change."
  type = object({
    ha_control_plane_usd     = number
    block_storage_usd_per_gb = number
    spaces_usd               = number
    r2_usd_per_gb            = number
    r2_class_a_usd_per_m     = number
    access_free_users        = number
    access_usd_per_user      = number
    uptime_free_checks       = number
    uptime_usd_per_check     = number
  })
  default = {
    ha_control_plane_usd     = 40
    block_storage_usd_per_gb = 0.10
    spaces_usd               = 5
    r2_usd_per_gb            = 0.015
    r2_class_a_usd_per_m     = 4.50
    access_free_users        = 50
    access_usd_per_user      = 7
    uptime_free_checks       = 1
    uptime_usd_per_check     = 1
  }
}

data "digitalocean_sizes" "pools" {
  filter {
    key    = "slug"
    values = distinct([for pool in var.pools : pool.size])
  }
}

locals {
  price_of = { for size in data.digitalocean_sizes.pools.sizes : size.slug => size.price_monthly if size.available && contains(size.regions, var.region) }
  unpriced = sort(distinct([for pool in var.pools : pool.size if !contains(keys(local.price_of), pool.size)]))
  breakdown = {
    nodes            = sum(concat([0], [for pool in var.pools : lookup(local.price_of, pool.size, 0) * pool.count]))
    ha_control_plane = var.ha_control_plane ? var.prices.ha_control_plane_usd : 0
    block_storage    = var.block_storage_gb * var.prices.block_storage_usd_per_gb
    spaces           = var.prices.spaces_usd
    r2               = var.r2_storage_gb * var.prices.r2_usd_per_gb + var.r2_class_a_millions * var.prices.r2_class_a_usd_per_m
    access           = var.access_users > var.prices.access_free_users ? var.access_users * var.prices.access_usd_per_user : 0
    uptime           = max(0, var.uptime_checks - var.prices.uptime_free_checks) * var.prices.uptime_usd_per_check
  }
  estimate = sum(values(local.breakdown))
}

resource "terraform_data" "estimate" {
  input = local.estimate

  lifecycle {
    precondition {
      condition     = length(local.unpriced) == 0
      error_message = "DigitalOcean does not offer these sizes in ${var.region}, so their price is unknown: ${join(", ", local.unpriced)}."
    }

  }
}

output "estimate_usd" {
  value = local.estimate
}

output "breakdown_usd" {
  value = local.breakdown
}
