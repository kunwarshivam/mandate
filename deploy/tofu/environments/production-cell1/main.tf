# Customer production, cell 1 (DEC-2064, Proposed: founder-reserved spending; never applied). One
# managed cell in sfo2 for many retail workspaces (HLD §4 and §8): a VPC and node firewall, an HA
# DOKS cluster with a system pool and a tainted workspace pool sized one node spare, a tunnel with a
# host and an Access application per workspace, two locked R2 buckets (the journal's WAL and base
# backups, the vault's Raft snapshots), out-of-cell alerts, and the estimate that refuses to plan
# past the founder's approval. Everything inside the cluster is lane GO's, by GitOps.

provider "digitalocean" {}

provider "cloudflare" {
  api_token = var.cloudflare_api_token
}

locals {
  name   = "mandate-prod-cell1"
  region = "sfo2"
  account_id = sensitive(one(distinct([
    for account in data.cloudflare_accounts.visible.result : account.id
    if account.id != null && account.id != ""
  ])))

  sizing = {
    demo = {
      system_size     = "s-2vcpu-4gb"
      journal_gb      = 20
      prometheus_gb   = 20
      r2_gb           = 25
      r2_class_a_m    = 0.02
      ws_size         = "s-4vcpu-8gb"
      ws_per_node     = 2
      artifacts_gb_ws = 5
    }
    first_customers = {
      system_size     = "s-4vcpu-8gb"
      journal_gb      = 50
      prometheus_gb   = 50
      r2_gb           = 160
      r2_class_a_m    = 0.1
      ws_size         = "s-4vcpu-8gb"
      ws_per_node     = 2
      artifacts_gb_ws = 5
    }
  }
  size = local.sizing[var.scale]

  journal_instances = 3
  vault_replicas    = 3
  vault_gb          = 5
  workspace_nodes   = ceil(length(var.workspace_ids) / local.size.ws_per_node) + 1
  block_storage_gb = (
    local.journal_instances * local.size.journal_gb
    + local.vault_replicas * local.vault_gb
    + local.size.prometheus_gb
    + length(var.workspace_ids) * local.size.artifacts_gb_ws
  )
}

data "cloudflare_accounts" "visible" {
  max_items = 2

  lifecycle {
    postcondition {
      condition = length(distinct([
        for account in self.result : account.id
        if account.id != null && account.id != ""
      ])) == 1
      error_message = "The Cloudflare token must see exactly one account, whose id this plan uses."
    }
  }
}

module "network" {
  source   = "../../modules/network"
  name     = local.name
  region   = local.region
  ip_range = "10.130.0.0/20"
  egress = [
    { protocol = "tcp", port_range = "443" },
    { protocol = "tcp", port_range = "53" },
    { protocol = "udp", port_range = "53" },
    { protocol = "udp", port_range = "123" },
    { protocol = "tcp", port_range = "7844" },
    { protocol = "udp", port_range = "7844" },
  ]
}

module "cluster" {
  source                     = "../../modules/cluster"
  name                       = local.name
  region                     = local.region
  vpc_id                     = module.network.vpc_id
  node_tag                   = module.network.node_tag
  kubernetes_version         = "1.35.9-do.2"
  ha_control_plane           = true
  system_pool                = { size = local.size.system_size, count = 3 }
  workspace_pool             = { size = local.size.ws_size, count = local.workspace_nodes }
  dedicated_pools            = var.dedicated_pools
  kube_api_allowed_addresses = var.kube_api_allowed_addresses
}

module "ingress" {
  source        = "../../modules/ingress"
  name          = local.name
  account_id    = local.account_id
  zone_name     = "owlhead.ai"
  workspace_ids = var.workspace_ids
  access_emails = var.access_emails
}

module "journal_archive" {
  source         = "../../modules/archive"
  account_id     = local.account_id
  bucket         = "mandate-journal-archive-prod-cell1"
  token_name     = "${local.name}-journal-archive"
  retention_days = 30
}

module "vault_snapshots" {
  source         = "../../modules/archive"
  account_id     = local.account_id
  bucket         = "mandate-vault-snapshots-prod-cell1"
  token_name     = "${local.name}-vault-snapshots"
  retention_days = 30
}

module "monitoring" {
  source       = "../../modules/monitoring"
  name         = local.name
  node_tag     = module.network.node_tag
  api_hosts    = module.ingress.api_hosts
  alert_emails = var.alert_emails
}

module "costing" {
  source              = "../../modules/costing"
  region              = local.region
  pools               = module.cluster.pools
  ha_control_plane    = true
  block_storage_gb    = local.block_storage_gb
  r2_storage_gb       = local.size.r2_gb
  r2_class_a_millions = local.size.r2_class_a_m
  access_users        = module.ingress.access_users
  uptime_checks       = module.monitoring.uptime_checks
}

resource "terraform_data" "founder_approval" {
  input = module.costing.estimate_usd

  lifecycle {
    precondition {
      condition     = module.costing.estimate_usd <= var.approved_monthly_usd
      error_message = "The monthly estimate of ${module.costing.estimate_usd} USD passes the founder's approved ${var.approved_monthly_usd} USD; spending is founder-reserved (DEC-79, DEC-2064 item 1)."
    }
  }
}

output "cluster_id" {
  value = module.cluster.cluster_id
}

output "api_hosts" {
  value = module.ingress.api_hosts
}

output "tunnel_token" {
  value     = module.ingress.tunnel_token
  sensitive = true
}

output "journal_archive" {
  description = "Barman's inputs; the key pair is sensitive and never printed."
  value = {
    bucket            = module.journal_archive.bucket
    destination_path  = "s3://${module.journal_archive.bucket}/journal-db"
    endpoint          = module.journal_archive.endpoint
    access_key_id     = module.journal_archive.access_key_id
    secret_access_key = module.journal_archive.secret_access_key
  }
  sensitive = true
}

output "vault_snapshots" {
  description = "The vault snapshot job's inputs; sensitive and never printed."
  value = {
    bucket            = module.vault_snapshots.bucket
    endpoint          = module.vault_snapshots.endpoint
    access_key_id     = module.vault_snapshots.access_key_id
    secret_access_key = module.vault_snapshots.secret_access_key
  }
  sensitive = true
}

output "workspace_nodes" {
  value = local.workspace_nodes
}

output "block_storage_gb" {
  value = local.block_storage_gb
}

output "monthly_estimate_usd" {
  value = module.costing.estimate_usd
}

output "monthly_breakdown_usd" {
  value = module.costing.breakdown_usd
}
