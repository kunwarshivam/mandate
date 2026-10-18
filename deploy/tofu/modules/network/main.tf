# One VPC per environment and the cloud firewall on the cluster's node tag (DEC-2012 item 4).
# DigitalOcean firewalls add up, so this one cannot close what DOKS's own worker firewall opens;
# what it guarantees is that nothing here admits a public source.

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

resource "digitalocean_vpc" "this" {
  name        = var.name
  region      = var.region
  ip_range    = var.ip_range
  description = "Managed by deploy/tofu (E21-56)."
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
    source_addresses = [digitalocean_vpc.this.ip_range]
  }

  inbound_rule {
    protocol         = "udp"
    port_range       = "1-65535"
    source_addresses = [digitalocean_vpc.this.ip_range]
  }

  inbound_rule {
    protocol         = "icmp"
    source_addresses = [digitalocean_vpc.this.ip_range]
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
    destination_addresses = [digitalocean_vpc.this.ip_range]
  }
}

output "vpc_id" {
  value = digitalocean_vpc.this.id
}

output "node_tag" {
  value = digitalocean_tag.nodes.name
}
