# The environment's buckets (DEC-2012 items 3 and 9): the OpenTofu state bucket, which the founder
# creates once and the root adopts, and the journal archive for E21-41's Barman. Spaces has no
# object lock, so the archive keeps every version, and Barman's key is granted that bucket alone.

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

resource "digitalocean_spaces_bucket" "journal_archive" {
  name   = "${var.prefix}-journal-archive-${var.environment}"
  region = var.region
  acl    = "private"

  versioning {
    enabled = true
  }

  lifecycle_rule {
    id                                     = "abort-incomplete-uploads"
    enabled                                = true
    abort_incomplete_multipart_upload_days = 7
  }

  lifecycle {
    prevent_destroy = true
  }
}

resource "digitalocean_spaces_key" "journal_archive" {
  name = "${var.prefix}-${var.environment}-journal-archive"

  grant {
    bucket     = digitalocean_spaces_bucket.journal_archive.name
    permission = "readwrite"
  }
}

output "state_bucket" {
  value = digitalocean_spaces_bucket.state.name
}

output "journal_archive_bucket" {
  value = digitalocean_spaces_bucket.journal_archive.name
}

output "journal_archive_endpoint" {
  value = "https://${var.region}.digitaloceanspaces.com"
}

output "journal_archive_access_key_id" {
  value = digitalocean_spaces_key.journal_archive.access_key
}

output "journal_archive_secret_access_key" {
  value     = digitalocean_spaces_key.journal_archive.secret_key
  sensitive = true
}
