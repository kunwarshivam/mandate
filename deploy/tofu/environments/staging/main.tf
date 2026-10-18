# Staging (DEC-2011 item 1): one DOKS cluster of two nodes in nyc3, its buckets, and the API host
# behind the tunnel and Access. The providers read their credentials from the environment
# (DIGITALOCEAN_TOKEN, SPACES_ACCESS_KEY_ID, SPACES_SECRET_ACCESS_KEY, CLOUDFLARE_API_TOKEN), which
# only infra.yml's jobs under infra-staging receive.

provider "digitalocean" {}

provider "cloudflare" {}

locals {
  environment = "staging"
  region      = "nyc3"
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
  spaces_monthly_usd         = 5
  block_storage_gb           = 40
  cap_usd                    = 65
}

module "cloudflare" {
  source                = "../../modules/cloudflare"
  name                  = "mandate-${local.environment}"
  account_id            = var.cloudflare_account_id
  zone_name             = "owlhead.ai"
  api_hostname          = "api-staging.owlhead.ai"
  api_origin            = "http://127.0.0.1:8080"
  access_allowed_emails = var.access_allowed_emails
  archive_bucket        = "mandate-journal-archive-${local.environment}"
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
