# One locked R2 bucket and the one token that may write it (DEC-2064 item 4, proposed): a customer
# production cell calls it twice, for the journal's WAL and base backups (DEC-973 item 6, DEC-2605
# row 4) and for the vault's Raft snapshots. The lock rule keeps every key for at least the
# retention, and the token is Object Read and Write on this bucket alone, so the process that
# writes backups can neither lift the lock nor reach another bucket. The lock is
# governance-equivalent (DEC-973 item 5): the infra token can lift it. The token lives only in
# encrypted state and leaves it as sensitive outputs (DEC-2029).

terraform {
  required_version = "1.13.1"

  required_providers {
    cloudflare = {
      source  = "cloudflare/cloudflare"
      version = "5.27.0"
    }
  }
}

variable "account_id" {
  type      = string
  sensitive = true

  validation {
    condition     = can(regex("^[0-9a-f]{32}$", var.account_id))
    error_message = "The Cloudflare account id is 32 lower-case hex characters."
  }
}

variable "bucket" {
  type = string

  validation {
    condition     = can(regex("^[a-z0-9][a-z0-9-]{2,62}$", var.bucket))
    error_message = "An R2 bucket name is 3 to 63 lower-case letters, digits and hyphens."
  }
}

variable "token_name" {
  description = "The name of the bucket's one write token."
  type        = string
}

variable "location" {
  description = "The R2 location hint: western North America, where all of the infrastructure lives (DEC-2047)."
  type        = string
  default     = "wnam"

  validation {
    condition     = var.location == "wnam"
    error_message = "The archive's R2 location is wnam (DEC-2047)."
  }
}

variable "retention_days" {
  description = "The writer's own retention, in days; the lock keeps every key at least this long."
  type        = number

  validation {
    condition     = var.retention_days >= 30 && floor(var.retention_days) == var.retention_days
    error_message = "The lock is a whole number of days, at least 30."
  }
}

resource "cloudflare_r2_bucket" "this" {
  account_id   = var.account_id
  name         = var.bucket
  location     = var.location
  jurisdiction = "default"

  lifecycle {
    prevent_destroy = true
  }
}

resource "cloudflare_r2_bucket_lock" "this" {
  account_id   = var.account_id
  bucket_name  = cloudflare_r2_bucket.this.name
  jurisdiction = cloudflare_r2_bucket.this.jurisdiction

  rules = [
    {
      id      = "keep-every-key-for-the-retention"
      enabled = true
      prefix  = ""
      condition = {
        type            = "Age"
        max_age_seconds = var.retention_days * 86400
      }
    },
  ]

  lifecycle {
    prevent_destroy = true
  }
}

data "cloudflare_account_api_token_permission_groups_list" "object_write" {
  account_id = var.account_id
  name       = "Workers R2 Storage Bucket Item Write"
  scope      = "com.cloudflare.edge.r2.bucket"
}

resource "cloudflare_account_token" "writer" {
  account_id = var.account_id
  name       = var.token_name

  policies = [
    {
      effect            = "allow"
      permission_groups = [for group in data.cloudflare_account_api_token_permission_groups_list.object_write.result : { id = group.id }]
      resources = jsonencode({
        "com.cloudflare.edge.r2.bucket.${var.account_id}_default_${cloudflare_r2_bucket.this.name}" = "*"
      })
    },
  ]

  lifecycle {
    precondition {
      condition     = length(data.cloudflare_account_api_token_permission_groups_list.object_write.result) == 1
      error_message = "Cloudflare must list exactly one Workers R2 Storage Bucket Item Write group for a bucket."
    }
  }
}

output "bucket" {
  value = cloudflare_r2_bucket.this.name
}

output "retention_days" {
  value = var.retention_days
}

output "endpoint" {
  value     = "https://${var.account_id}.r2.cloudflarestorage.com"
  sensitive = true
}

output "access_key_id" {
  description = "The writer's R2 access key id: the minted token's id (DEC-2029)."
  value       = cloudflare_account_token.writer.id
  sensitive   = true
}

output "secret_access_key" {
  description = "The writer's R2 secret: the SHA-256 of the minted token's value, R2's S3 derivation (DEC-2029)."
  value       = sha256(cloudflare_account_token.writer.value)
  sensitive   = true
}
