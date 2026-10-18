# The environment's only ingress (DEC-2012 item 6, DEC-967 item 6): a remotely managed tunnel,
# whose token becomes the workspace chart's cloudflared-tunnel Secret (lane GO places it); one
# proxied DNS record for the API's hostname pointing at the tunnel; one Access application on that
# hostname that admits only members of the founder's own Cloudflare account, so no email is
# configured anywhere; and the journal's WAL and base-backup archive, an R2 bucket whose one lock
# rule keeps every key for at least the archive's retention (DEC-973 item 6,
# DEC-2012 item 9); and Barman's token for that bucket alone, Object Read and Write, minted here
# (DEC-2029) and handed on only as sensitive outputs.

terraform {
  required_version = "1.13.1"

  required_providers {
    cloudflare = {
      source  = "cloudflare/cloudflare"
      version = "5.27.0"
    }
  }
}

variable "name" {
  type = string
}

variable "account_id" {
  type      = string
  sensitive = true

  validation {
    condition     = can(regex("^[0-9a-f]{32}$", var.account_id))
    error_message = "The Cloudflare account id is 32 lower-case hex characters."
  }
}

variable "zone_name" {
  type = string
}

variable "api_hostname" {
  type = string

  validation {
    condition     = can(regex("^[a-z0-9-]+([.][a-z0-9-]+)+$", var.api_hostname))
    error_message = "The API hostname is a plain DNS name."
  }
}

variable "api_origin" {
  description = "Where cloudflared sends the API host's requests, inside the cluster."
  type        = string

  validation {
    condition     = can(regex("^https?://", var.api_origin))
    error_message = "The API origin must be a web URL, plain or over TLS."
  }
}

variable "archive_bucket" {
  description = "The journal archive's R2 bucket name."
  type        = string
}

variable "archive_retention_days" {
  description = "The journal-db chart's archive.retention, in days; the lock keeps every key at least this long."
  type        = number
  default     = 30

  validation {
    condition     = var.archive_retention_days >= 30 && floor(var.archive_retention_days) == var.archive_retention_days
    error_message = "The archive's lock is a whole number of days, at least the chart's 30-day retention."
  }
}

data "cloudflare_zone" "this" {
  filter = {
    name = var.zone_name
  }
}

resource "cloudflare_zero_trust_tunnel_cloudflared" "this" {
  account_id = var.account_id
  name       = var.name
  config_src = "cloudflare"
}

resource "cloudflare_zero_trust_tunnel_cloudflared_config" "this" {
  account_id = var.account_id
  tunnel_id  = cloudflare_zero_trust_tunnel_cloudflared.this.id

  config = {
    ingress = [
      {
        hostname = var.api_hostname
        service  = var.api_origin
      },
      {
        service = "http_status:404"
      },
    ]
  }
}

resource "cloudflare_dns_record" "api" {
  zone_id = data.cloudflare_zone.this.zone_id
  name    = var.api_hostname
  type    = "CNAME"
  content = "${cloudflare_zero_trust_tunnel_cloudflared.this.id}.cfargotunnel.com"
  proxied = true
  ttl     = 1
  comment = "Managed by deploy/tofu (E21-56): the API host, through the tunnel, behind Access."
}

resource "cloudflare_zero_trust_access_policy" "operators" {
  account_id       = var.account_id
  name             = "${var.name}-operators"
  decision         = "allow"
  session_duration = "24h"
  include = [
    {
      cloudflare_account_member = {
        account_id = var.account_id
      }
    },
  ]
}

resource "cloudflare_zero_trust_access_application" "api" {
  account_id       = var.account_id
  name             = "${var.name}-api"
  type             = "self_hosted"
  domain           = var.api_hostname
  session_duration = "24h"

  destinations = [
    {
      type = "public"
      uri  = var.api_hostname
    },
  ]

  policies = [
    {
      id         = cloudflare_zero_trust_access_policy.operators.id
      precedence = 1
    },
  ]
}

resource "cloudflare_r2_bucket" "journal_archive" {
  account_id   = var.account_id
  name         = var.archive_bucket
  location     = "enam"
  jurisdiction = "default"

  lifecycle {
    prevent_destroy = true
  }
}

resource "cloudflare_r2_bucket_lock" "journal_archive" {
  account_id   = var.account_id
  bucket_name  = cloudflare_r2_bucket.journal_archive.name
  jurisdiction = cloudflare_r2_bucket.journal_archive.jurisdiction

  rules = [
    {
      id      = "keep-every-key-for-the-retention"
      enabled = true
      prefix  = ""
      condition = {
        type            = "Age"
        max_age_seconds = var.archive_retention_days * 86400
      }
    },
  ]

  lifecycle {
    prevent_destroy = true
  }
}

data "cloudflare_account_api_token_permission_groups_list" "r2_object_write" {
  account_id = var.account_id
  name       = "Workers R2 Storage Bucket Item Write"
  scope      = "com.cloudflare.edge.r2.bucket"
}

resource "cloudflare_account_token" "journal_archive" {
  account_id = var.account_id
  name       = "${var.name}-journal-archive"

  policies = [
    {
      effect            = "allow"
      permission_groups = [for group in data.cloudflare_account_api_token_permission_groups_list.r2_object_write.result : { id = group.id }]
      resources = jsonencode({
        "com.cloudflare.edge.r2.bucket.${var.account_id}_default_${cloudflare_r2_bucket.journal_archive.name}" = "*"
      })
    },
  ]

  lifecycle {
    precondition {
      condition     = length(data.cloudflare_account_api_token_permission_groups_list.r2_object_write.result) == 1
      error_message = "Cloudflare must list exactly one Workers R2 Storage Bucket Item Write group for a bucket."
    }
  }
}

data "cloudflare_zero_trust_tunnel_cloudflared_token" "this" {
  account_id = var.account_id
  tunnel_id  = cloudflare_zero_trust_tunnel_cloudflared.this.id
}

output "tunnel_id" {
  value = cloudflare_zero_trust_tunnel_cloudflared.this.id
}

output "tunnel_token" {
  value     = data.cloudflare_zero_trust_tunnel_cloudflared_token.this.token
  sensitive = true
}

output "api_hostname" {
  value = cloudflare_dns_record.api.name
}

output "archive_bucket" {
  value = cloudflare_r2_bucket.journal_archive.name
}

output "archive_endpoint" {
  value     = "https://${var.account_id}.r2.cloudflarestorage.com"
  sensitive = true
}

output "archive_access_key_id" {
  description = "Barman's R2 access key id: the minted token's id (DEC-2029)."
  value       = cloudflare_account_token.journal_archive.id
  sensitive   = true
}

output "archive_secret_access_key" {
  description = "Barman's R2 secret: the SHA-256 of the minted token's value, R2's S3 derivation (DEC-2029)."
  value       = sha256(cloudflare_account_token.journal_archive.value)
  sensitive   = true
}

output "archive_jurisdiction" {
  value = cloudflare_r2_bucket.journal_archive.jurisdiction
}
