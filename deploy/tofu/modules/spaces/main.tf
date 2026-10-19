# OpenTofu's state bucket, and nothing else (DEC-2012 item 5): `cargo xtask infra` makes it when it
# is missing (DEC-2030 item 4) and the root adopts it with an import block; it stays private and
# versioned and cannot be destroyed by a plan. It lives in the US West (DEC-2047 item 6), under a
# new name, since a bucket cannot change region; the nyc3 bucket it replaces is forgotten, never
# destroyed, and kept as the rollback until the founder deletes it. No journal spec §6.2 object and
# no backup is ever written here; the journal archive is R2 (DEC-2012 item 9, the cloudflare
# module).

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

resource "digitalocean_spaces_bucket" "west" {
  name   = "${var.prefix}-tofu-state-west-${var.environment}"
  region = var.region
  acl    = "private"

  versioning {
    enabled = true
  }

  lifecycle {
    prevent_destroy = true
  }
}

removed {
  from = digitalocean_spaces_bucket.state

  lifecycle {
    destroy = false
  }
}

output "state_bucket" {
  value = digitalocean_spaces_bucket.west.name
}

output "region" {
  value = digitalocean_spaces_bucket.west.region
}
