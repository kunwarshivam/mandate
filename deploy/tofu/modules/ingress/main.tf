# A customer production cell's only ingress (DEC-2064 item 5, proposed): one remotely managed
# tunnel for the cell, and for each workspace its own API host, `api-<id>.<zone>`, one label deep so
# the zone's free Universal SSL certificate covers it. Each host is a proxied record to the tunnel,
# routed only to that workspace's own API Service, behind its own Access application whose policy
# admits only that workspace's named people. So a person admitted to one workspace's host is not
# admitted to another's, and every other name reaching the tunnel gets a 404. Customer production
# has no account-members fallback: a workspace with no named email fails the plan.

terraform {
  required_version = "1.13.1"

  required_providers {
    cloudflare = {
      source  = "cloudflare/cloudflare"
      version = "5.27.0"
    }
  }
}

variable "name" {
  type = string
}

variable "account_id" {
  type      = string
  sensitive = true

  validation {
    condition     = can(regex("^[0-9a-f]{32}$", var.account_id))
    error_message = "The Cloudflare account id is 32 lower-case hex characters."
  }
}

variable "zone_name" {
  type = string
}

variable "workspace_ids" {
  description = "The cell's workspaces, as the opaque ids their namespaces carry (ws-<id>)."
  type        = set(string)

  validation {
    condition     = length(var.workspace_ids) > 0 && alltrue([for id in var.workspace_ids : can(regex("^[a-z0-9]{1,20}$", id))])
    error_message = "At least one workspace, each id 1 to 20 lower-case letters and digits."
  }
}

variable "access_emails" {
  description = "Per workspace id, the full addresses its Access policy admits; from the GitHub environment, never committed."
  type        = map(list(string))
  sensitive   = true

  validation {
    condition = alltrue(flatten([
      for id, emails in var.access_emails : [
        for email in emails : can(regex("^[^@[:space:]*]+@[^@[:space:]*]+[.][^@[:space:]*]+$", email))
      ]
    ]))
    error_message = "Each Access email is a full address; a wildcard or a domain alone is refused."
  }
}

variable "api_port" {
  description = "The port of each workspace's in-cluster API Service."
  type        = number
  default     = 8080
}

locals {
  hosts = { for id in var.workspace_ids : id => "api-${id}.${var.zone_name}" }
}

data "cloudflare_zone" "this" {
  filter = {
    name = var.zone_name
  }
}

resource "cloudflare_zero_trust_tunnel_cloudflared" "this" {
  account_id = var.account_id
  name       = var.name
  config_src = "cloudflare"
}

resource "cloudflare_zero_trust_tunnel_cloudflared_config" "this" {
  account_id = var.account_id
  tunnel_id  = cloudflare_zero_trust_tunnel_cloudflared.this.id

  config = {
    ingress = concat(
      [
        for id in sort(tolist(var.workspace_ids)) : {
          hostname = local.hosts[id]
          service  = "http://api.ws-${id}.svc.cluster.local:${var.api_port}"
        }
      ],
      [
        {
          service = "http_status:404"
        },
      ],
    )
  }
}

resource "cloudflare_dns_record" "api" {
  for_each = local.hosts
  zone_id  = data.cloudflare_zone.this.zone_id
  name     = each.value
  type     = "CNAME"
  content  = "${cloudflare_zero_trust_tunnel_cloudflared.this.id}.cfargotunnel.com"
  proxied  = true
  ttl      = 1
  comment  = "Managed by deploy/tofu (DEC-2064): a workspace's API host, through the tunnel, behind its own Access."
}

resource "cloudflare_zero_trust_access_policy" "workspace" {
  for_each         = local.hosts
  account_id       = var.account_id
  name             = "${var.name}-ws-${each.key}"
  decision         = "allow"
  session_duration = "24h"
  include          = [for email in lookup(var.access_emails, each.key, []) : { email = { email = email } }]

  lifecycle {
    precondition {
      condition     = length(lookup(var.access_emails, each.key, [])) > 0
      error_message = "Every workspace's Access policy names at least one person; customer production has no account-members fallback (DEC-2064 item 5)."
    }
  }
}

resource "cloudflare_zero_trust_access_application" "api" {
  for_each         = local.hosts
  account_id       = var.account_id
  name             = "${var.name}-ws-${each.key}-api"
  type             = "self_hosted"
  domain           = each.value
  session_duration = "24h"

  # As staging's and production's (DEC-2381): only an unauthenticated OPTIONS preflight is
  # forwarded, and the API answers it with an empty 204 for the app's origin alone.
  options_preflight_bypass = true

  destinations = [
    {
      type = "public"
      uri  = each.value
    },
  ]

  policies = [
    {
      id         = cloudflare_zero_trust_access_policy.workspace[each.key].id
      precedence = 1
    },
  ]
}

data "cloudflare_zero_trust_tunnel_cloudflared_token" "this" {
  account_id = var.account_id
  tunnel_id  = cloudflare_zero_trust_tunnel_cloudflared.this.id
}

output "tunnel_id" {
  value = cloudflare_zero_trust_tunnel_cloudflared.this.id
}

output "tunnel_token" {
  value     = data.cloudflare_zero_trust_tunnel_cloudflared_token.this.token
  sensitive = true
}

output "api_hosts" {
  description = "Per workspace id, its API host."
  value       = { for id, record in cloudflare_dns_record.api : id => record.name }
}

output "access_users" {
  description = "How many distinct people Access admits across the cell, for the estimate's seat count."
  value       = nonsensitive(length(distinct(flatten(values(var.access_emails)))))
}
