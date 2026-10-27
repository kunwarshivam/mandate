# Self-hosted CI runners (DEC-2065): three s-4vcpu-8gb droplets in sfo2, one ephemeral job each, at
# 144 USD a month against the founder's cap of 150 USD, kept apart from DEC-2011's hosting budget.
# DigitalOcean's provider reads its token from DIGITALOCEAN_TOKEN in the founder's shell.

provider "digitalocean" {}

module "runners" {
  source                     = "../../modules/ci-runners"
  region                     = "sfo2"
  size                       = "s-4vcpu-8gb"
  runner_count               = 3
  cap_usd                    = 150
  repository                 = "kunwarshivam/mandate"
  runner_label               = "mandate-ci-do"
  github_app_client_id       = var.github_app_client_id
  github_app_installation_id = var.github_app_installation_id
  github_app_private_key     = var.github_app_private_key
  runner_version             = "2.338.1"
  runner_sha256              = "08e9ccb3604d2462c24b5e33604dc30810296ca15cc65f23cffd4317eb144c37"
  sysbox_version             = "0.7.1"
  sysbox_sha256              = "9d6d5484f980d0a17f86c492c1262015c2afb66280bdb97215b79fde6a0261c5"
  kubectl_version            = "1.35.9"
  kubectl_sha256             = "3cfeaf80be482b435b0aa214aff6e0b2c312ee23c0ff20810c75517b6004c6eb"
  base_image                 = "ubuntu:24.04@sha256:68ee22004a9109b44973313b02ff7b1de250f1de0f5ecb628d73f579aaff33ac"
  toolchain_ref              = "main"
  debug_ssh_key              = var.debug_ssh_key
  debug_ssh_cidr             = var.debug_ssh_cidr
}

output "estimate_usd" {
  value = module.runners.estimate_usd
}

output "runner_names" {
  value = module.runners.runner_names
}
