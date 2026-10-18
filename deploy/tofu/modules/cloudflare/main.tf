# The environment's only ingress (DEC-2012 item 6, DEC-967 item 6): a remotely managed tunnel,
# whose token becomes the workspace chart's cloudflared-tunnel Secret (lane GO places it); one
# proxied DNS record for the API's hostname pointing at the tunnel; and one Access application on
# that hostname that admits only the named emails.

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
  type = string

  validation {
    condition     = can(regex("^[0-9a-f]{32}$", var.account_id))
    error_message = "The Cloudflare account id is 32 lower-case hex characters."
  }
}

variable "zone_name" {
  type = string
}

variable "api_hostname" {
  type = string

  validation {
    condition     = can(regex("^[a-z0-9-]+(\\.[a-z0-9-]+)+$", var.api_hostname))
    error_message = "The API hostname is a plain DNS name."
  }
}

variable "api_origin" {
  description = "Where cloudflared sends the API host's requests, inside the cluster."
  type        = string

  validation {
    condition     = can(regex("^https?://", var.api_origin))
    error_message = "The API origin is an http or https URL."
  }
}

variable "access_allowed_emails" {
  description = "The only identities Access admits to the API host."
  type        = list(string)

  validation {
    condition     = length(var.access_allowed_emails) > 0 && alltrue([for email in var.access_allowed_emails : can(regex("^[^@\\s*]+@[^@\\s*]+\\.[^@\\s*]+$", email))])
    error_message = "Access needs at least one email, each a full address; a wildcard or a domain alone is refused."
  }
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
    ingress = [
      {
        hostname = var.api_hostname
        service  = var.api_origin
      },
      {
        service = "http_status:404"
      },
    ]
  }
}

resource "cloudflare_dns_record" "api" {
  zone_id = data.cloudflare_zone.this.zone_id
  name    = var.api_hostname
  type    = "CNAME"
  content = "${cloudflare_zero_trust_tunnel_cloudflared.this.id}.cfargotunnel.com"
  proxied = true
  ttl     = 1
  comment = "Managed by deploy/tofu (E21-56): the API host, through the tunnel, behind Access."
}

resource "cloudflare_zero_trust_access_policy" "operators" {
  account_id       = var.account_id
  name             = "${var.name}-operators"
  decision         = "allow"
  session_duration = "24h"
  include          = [for email in var.access_allowed_emails : { email = { email = email } }]
}

resource "cloudflare_zero_trust_access_application" "api" {
  account_id       = var.account_id
  name             = "${var.name}-api"
  type             = "self_hosted"
  domain           = var.api_hostname
  session_duration = "24h"

  destinations = [
    {
      type = "public"
      uri  = var.api_hostname
    },
  ]

  policies = [
    {
      id         = cloudflare_zero_trust_access_policy.operators.id
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

output "api_hostname" {
  value = cloudflare_dns_record.api.name
}
