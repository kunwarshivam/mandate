# A customer production cell's operator alerts outside the cell (DEC-2064 item 6, proposed):
# DigitalOcean's own node alerts on the cell's node tag, and an uptime check with an alert on each
# workspace's API host, so a cell that is down, whose in-cluster Prometheus and Alertmanager are down
# with it, still pages. They carry no content: a node, a host and a threshold (OPS-10). Inside the
# cell, the metrics and the safety-rule alerts of infrastructure §8.2 are lane GO's Prometheus and
# Alertmanager, whose cost is the system pool's.

terraform {
  required_version = "1.13.1"

  required_providers {
    digitalocean = {
      source  = "digitalocean/digitalocean"
      version = "2.103.0"
    }
  }
}

variable "name" {
  type = string
}

variable "node_tag" {
  type = string
}

variable "api_hosts" {
  description = "Per workspace id, its API host."
  type        = map(string)
}

variable "alert_emails" {
  description = "Who DigitalOcean pages; from the GitHub environment, never committed."
  type        = list(string)
  sensitive   = true

  validation {
    condition     = length(var.alert_emails) > 0 && alltrue([for email in var.alert_emails : can(regex("^[^@[:space:]*]+@[^@[:space:]*]+[.][^@[:space:]*]+$", email))])
    error_message = "At least one full email address."
  }
}

locals {
  node_alerts = {
    cpu    = { type = "v1/insights/droplet/cpu", value = 85, description = "node CPU over 85% for 10 minutes" }
    memory = { type = "v1/insights/droplet/memory_utilization_percent", value = 90, description = "node memory over 90% for 10 minutes" }
    disk   = { type = "v1/insights/droplet/disk_utilization_percent", value = 80, description = "node disk over 80% for 10 minutes" }
  }
}

resource "digitalocean_monitor_alert" "node" {
  for_each    = local.node_alerts
  type        = each.value.type
  compare     = "GreaterThan"
  value       = each.value.value
  window      = "10m"
  tags        = [var.node_tag]
  enabled     = true
  description = "${var.name}: ${each.value.description}"

  alerts {
    email = var.alert_emails
  }
}

resource "digitalocean_uptime_check" "api" {
  for_each = var.api_hosts
  name     = "${var.name}-ws-${each.key}"
  target   = "https://${each.value}/"
  type     = "https"
  regions  = ["us_west", "us_east"]
  enabled  = true
}

resource "digitalocean_uptime_alert" "api_down" {
  for_each = var.api_hosts
  check_id = digitalocean_uptime_check.api[each.key].id
  name     = "${var.name}-ws-${each.key}-down"
  type     = "down"
  period   = "2m"

  notifications {
    email = var.alert_emails
  }
}

output "uptime_checks" {
  value = length(digitalocean_uptime_check.api)
}
