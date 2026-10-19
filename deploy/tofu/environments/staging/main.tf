# Staging (DEC-2011 item 1): one DOKS cluster of two nodes in nyc1, its buckets, and the API host
# behind the tunnel and Access. The cluster and its VPC moved from nyc3, which had no capacity for
# its nodes, to nyc1, also New York, at the same price; the state bucket stays in nyc3, since Spaces
# does not run in nyc1 (DEC-2044 item 2). DigitalOcean's provider reads its credentials from the environment
# (DIGITALOCEAN_TOKEN, SPACES_ACCESS_KEY_ID, SPACES_SECRET_ACCESS_KEY) and Cloudflare's takes its
# token as a variable; only infra.yml's job under infra-staging receives either.

provider "digitalocean" {}

provider "cloudflare" {
  api_token = var.cloudflare_api_token
}

locals {
  environment  = "staging"
  region       = "nyc1"
  state_region = "nyc3"
  account_id = sensitive(one(distinct([
    for account in data.cloudflare_accounts.visible.result : account.id
    if account.id != null && account.id != ""
  ])))
}

# Provider 5.27.0 asks for the next page of accounts until a page comes back empty or it holds
# max_items (default 1000), and the accounts API answered every page with the same account, so the
# founder's first plan listed one account 1000 times. So the list is read as its distinct ids. max_items = 2 keeps
# it to two requests: a token that sees a second account lists both on the first page, which the
# rule below refuses.
data "cloudflare_accounts" "visible" {
  max_items = 2

  lifecycle {
    postcondition {
      condition = length(distinct([
        for account in self.result : account.id
        if account.id != null && account.id != ""
      ])) == 1
      error_message = "CF_INFRA_API_TOKEN must see exactly one Cloudflare account, whose id this plan uses."
    }
  }
}

import {
  for_each = var.adopt_state_bucket ? toset(["state"]) : toset([])
  to       = module.cell.module.spaces.digitalocean_spaces_bucket.state
  id       = "${local.state_region},mandate-tofu-state-${local.environment}"
}

module "cell" {
  source                     = "../../modules/cell"
  environment                = local.environment
  region                     = local.region
  state_region               = local.state_region
  vpc_ip_range               = "10.110.0.0/20"
  kubernetes_version         = "1.35.9-do.1"
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

resource "terraform_data" "applied_commit" {
  input = var.commit_sha
}

output "applied_commit" {
  value = terraform_data.applied_commit.output
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
  value     = module.cloudflare.archive_endpoint
  sensitive = true
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
  value     = local.account_id
  sensitive = true
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
