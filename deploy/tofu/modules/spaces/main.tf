# OpenTofu's state bucket, and nothing else (DEC-2012 item 5): the founder creates it once, by
# hand, and the root adopts it with an import block; it stays private and versioned and cannot be
# destroyed by a plan. No journal spec §6.2 object and no backup is ever written here; the journal
# archive is R2 (DEC-2012 item 9, the cloudflare module).

terraform {
  required_version = "1.13.1"

  required_providers {
    digitalocean = {
      source  = "digitalocean/digitalocean"
      version = "2.103.0"
    }
  }
}

variable "prefix" {
  type = string
}

variable "environment" {
  type = string
}

variable "region" {
  type = string
}

resource "digitalocean_spaces_bucket" "state" {
  name   = "${var.prefix}-tofu-state-${var.environment}"
  region = var.region
  acl    = "private"

  versioning {
    enabled = true
  }

  lifecycle {
    prevent_destroy = true
  }
}

output "state_bucket" {
  value = digitalocean_spaces_bucket.state.name
}

output "region" {
  value = digitalocean_spaces_bucket.state.region
}
