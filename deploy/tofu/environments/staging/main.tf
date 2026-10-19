# Staging (DEC-2011 item 1): one DOKS cluster of two nodes in nyc3, its buckets, and the API host
# behind the tunnel and Access. DigitalOcean's provider reads its credentials from the environment
# (DIGITALOCEAN_TOKEN, SPACES_ACCESS_KEY_ID, SPACES_SECRET_ACCESS_KEY) and Cloudflare's takes its
# token as a variable; only infra.yml's job under infra-staging receives either.

provider "digitalocean" {}

provider "cloudflare" {
  api_token = var.cloudflare_api_token
}

locals {
  environment = "staging"
  region      = "nyc3"
  account_id  = data.cloudflare_accounts.visible.result[0].id
}

data "cloudflare_accounts" "visible" {
  lifecycle {
    postcondition {
      condition     = length(self.result) == 1
      error_message = "CF_INFRA_API_TOKEN must see exactly one Cloudflare account, whose id this plan uses."
    }
  }
}

import {
  for_each = var.adopt_state_bucket ? toset(["state"]) : toset([])
  to       = module.cell.module.spaces.digitalocean_spaces_bucket.state
  id       = "${local.region},mandate-tofu-state-${local.environment}"
}

module "cell" {
  source                     = "../../modules/cell"
  environment                = local.environment
  region                     = local.region
  vpc_ip_range               = "10.110.0.0/20"
  kubernetes_version         = "1.35.1-do.0"
  node_size                  = "s-2vcpu-4gb"
  kube_api_allowed_addresses = var.kube_api_allowed_addresses
  bootstrap_runner_cidr      = var.bootstrap_runner_cidr
  spaces_monthly_usd         = 5
  block_storage_gb           = 40
  cap_usd                    = 65
}

module "cloudflare" {
  source         = "../../modules/cloudflare"
  name           = "mandate-${local.environment}"
  account_id     = local.account_id
  zone_name      = "owlhead.ai"
  api_hostname   = "api-staging.owlhead.ai"
  api_origin     = var.api_origin
  archive_bucket = "mandate-journal-archive-${local.environment}"
}

output "cluster_id" {
  value = module.cell.cluster_id
}

output "cluster_endpoint" {
  value = module.cell.cluster_endpoint
}

output "archive_bucket" {
  value = module.cloudflare.archive_bucket
}

output "archive_endpoint" {
  value = module.cloudflare.archive_endpoint
}

output "archive_destination_path" {
  value = "s3://${module.cloudflare.archive_bucket}/journal-db"
}

output "archive_access_key_id" {
  value     = module.cloudflare.archive_access_key_id
  sensitive = true
}

output "archive_secret_access_key" {
  value     = module.cloudflare.archive_secret_access_key
  sensitive = true
}

output "archive_jurisdiction" {
  value = module.cloudflare.archive_jurisdiction
}

output "cloudflare_account_id" {
  value = local.account_id
}

output "tunnel_id" {
  value = module.cloudflare.tunnel_id
}

output "tunnel_token" {
  value     = module.cloudflare.tunnel_token
  sensitive = true
}

output "api_hostname" {
  value = module.cloudflare.api_hostname
}

output "monthly_estimate_usd" {
  value = module.cell.monthly_estimate_usd
}
