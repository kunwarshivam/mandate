# One hosted environment: the network, the cluster, the buckets and the budget, wired together
# (DEC-2012 item 1). Each root under deploy/tofu/environments calls this with its own values, so
# staging and production differ only in values.

terraform {
  required_version = "1.13.1"

  required_providers {
    digitalocean = {
      source  = "digitalocean/digitalocean"
      version = "2.103.0"
    }
  }
}

variable "environment" {
  type = string

  validation {
    condition     = contains(["staging", "production"], var.environment)
    error_message = "The environment is staging or production."
  }
}

variable "prefix" {
  description = "The prefix of every name; bucket names are global, so it is a variable."
  type        = string
  default     = "mandate"
}

variable "region" {
  type    = string
  default = "nyc3"
}

variable "vpc_ip_range" {
  type = string
}

variable "kubernetes_version" {
  type = string
}

variable "node_size" {
  type    = string
  default = "s-2vcpu-4gb"
}

variable "kube_api_allowed_addresses" {
  type = list(string)
}

variable "bootstrap_runner_cidr" {
  description = "The runner window of one bootstrap run (DEC-2028); the doks module validates it."
  type        = string
  default     = ""
}

variable "spaces_monthly_usd" {
  description = "Spaces' subscription, counted in every environment though it is per account (DEC-2012 item 12)."
  type        = number

  validation {
    condition     = var.spaces_monthly_usd >= 5
    error_message = "Spaces costs at least 5 USD a month; counting less would undercount the estimate."
  }
}

variable "block_storage_gb" {
  type = number
}

variable "cap_usd" {
  description = "This environment's refusal threshold; two environments at 65 USD leave headroom under DEC-2011's 150 USD."
  type        = number

  validation {
    condition     = var.cap_usd > 0 && var.cap_usd <= 65
    error_message = "An environment's cap is at most 65 USD a month (DEC-2012 item 12)."
  }
}

locals {
  name = "${var.prefix}-${var.environment}"
}

module "network" {
  source   = "../network"
  name     = local.name
  region   = var.region
  ip_range = var.vpc_ip_range
}

module "doks" {
  source                     = "../doks"
  name                       = local.name
  region                     = var.region
  vpc_id                     = module.network.vpc_id
  node_tag                   = module.network.node_tag
  kubernetes_version         = var.kubernetes_version
  node_size                  = var.node_size
  kube_api_allowed_addresses = var.kube_api_allowed_addresses
  bootstrap_runner_cidr      = var.bootstrap_runner_cidr
}

module "spaces" {
  source      = "../spaces"
  prefix      = var.prefix
  environment = var.environment
  region      = var.region
}

module "budget" {
  source             = "../budget"
  node_size          = var.node_size
  node_count         = module.doks.node_count
  spaces_monthly_usd = var.spaces_monthly_usd
  block_storage_gb   = var.block_storage_gb
  cap_usd            = var.cap_usd
}

output "cluster_id" {
  value = module.doks.cluster_id
}

output "cluster_endpoint" {
  value = module.doks.endpoint
}

output "state_bucket" {
  value = module.spaces.state_bucket
}

output "monthly_estimate_usd" {
  value = module.budget.estimate_usd
}
