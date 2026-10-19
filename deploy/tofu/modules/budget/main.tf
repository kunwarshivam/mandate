# The environment's monthly estimate and its share of DEC-2011's cap (DEC-2012 item 7). The node
# price is DigitalOcean's own size list; Spaces and block storage are list prices. The plan fails
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

data "digitalocean_sizes" "node" {
  filter {
    key    = "slug"
    values = [var.node_size]
  }
}

locals {
  node_prices = [for size in data.digitalocean_sizes.node.sizes : size.price_monthly]
  estimate    = (length(local.node_prices) == 1 ? local.node_prices[0] : 0) * var.node_count + var.spaces_monthly_usd + var.block_storage_gb * var.block_storage_usd_per_gb
}

resource "terraform_data" "cap" {
  input = local.estimate

  lifecycle {
    precondition {
      condition     = length(local.node_prices) == 1
      error_message = "DigitalOcean lists no single size ${var.node_size}, so its price is unknown."
    }

    precondition {
      condition     = local.estimate <= var.cap_usd
      error_message = "The monthly estimate of ${local.estimate} USD passes this environment's cap of ${var.cap_usd} USD (DEC-2011 item 2)."
    }
  }
}

output "estimate_usd" {
  value = local.estimate
}
