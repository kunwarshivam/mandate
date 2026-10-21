# One VPC per environment and the cloud firewall on the cluster's node tag (DEC-2012 item 4).
# DigitalOcean firewalls add up, so this one cannot close what DOKS's own worker firewall opens;
# what it guarantees is that nothing here admits a public address.

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
  description = "The environment's resource name prefix, for example mandate-staging."
  type        = string
}

variable "region" {
  description = "The DigitalOcean region (DEC-995 item 2)."
  type        = string
}

variable "ip_range" {
  description = "The VPC's private range, distinct per environment."
  type        = string

  validation {
    condition     = can(cidrhost(var.ip_range, 0)) && startswith(var.ip_range, "10.")
    error_message = "The VPC range must be a CIDR inside 10.0.0.0/8."
  }
}

resource "digitalocean_tag" "nodes" {
  name = "${var.name}-nodes"
}

# The VPC is named for its region and never destroyed by a plan (DEC-2054). DigitalOcean makes the
# first VPC in a region that region's default and refuses to delete a default VPC, so staging's
# first VPC, in nyc3, could not be replaced when the cluster moved to sfo2. It is forgotten below,
# never destroyed, for the founder to delete by hand once it is no longer the default.
resource "digitalocean_vpc" "west" {
  name        = "${var.name}-${var.region}"
  region      = var.region
  ip_range    = var.ip_range
  description = "Managed by deploy/tofu (E21-56)."

  lifecycle {
    prevent_destroy = true
  }
}

removed {
  from = digitalocean_vpc.this

  lifecycle {
    destroy = false
  }
}

locals {
  egress = [
    { protocol = "tcp", port_range = "443" },
    { protocol = "tcp", port_range = "80" },
    { protocol = "tcp", port_range = "53" },
    { protocol = "udp", port_range = "53" },
    { protocol = "udp", port_range = "123" },
    { protocol = "tcp", port_range = "7844" },
    { protocol = "udp", port_range = "7844" },
  ]
}

resource "digitalocean_firewall" "nodes" {
  name = "${var.name}-nodes"
  tags = [digitalocean_tag.nodes.name]

  inbound_rule {
    protocol         = "tcp"
    port_range       = "1-65535"
    source_addresses = [digitalocean_vpc.west.ip_range]
  }

  inbound_rule {
    protocol         = "udp"
    port_range       = "1-65535"
    source_addresses = [digitalocean_vpc.west.ip_range]
  }

  inbound_rule {
    protocol         = "icmp"
    source_addresses = [digitalocean_vpc.west.ip_range]
  }

  dynamic "outbound_rule" {
    for_each = local.egress
    content {
      protocol              = outbound_rule.value.protocol
      port_range            = outbound_rule.value.port_range
      destination_addresses = ["0.0.0.0/0", "::/0"]
    }
  }

  outbound_rule {
    protocol              = "icmp"
    destination_addresses = [digitalocean_vpc.west.ip_range]
  }
}

output "vpc_id" {
  value = digitalocean_vpc.west.id
}

output "node_tag" {
  value = digitalocean_tag.nodes.name
}

output "region" {
  value = digitalocean_vpc.west.region
}
