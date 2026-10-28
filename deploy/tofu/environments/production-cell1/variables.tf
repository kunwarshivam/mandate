# Every secret here would come from the GitHub environment, never from anything committed
# (DEC-2064 item 10). No workflow sets them today: this root is never dispatched.

variable "state_passphrase" {
  description = "Encrypts this cell's state and plans; its own, never staging's or production's."
  type        = string
  sensitive   = true

  validation {
    condition     = length(var.state_passphrase) >= 32
    error_message = "The state passphrase must be at least 32 characters."
  }
}

variable "kube_api_allowed_addresses" {
  description = "The only CIDRs the control plane admits, as a JSON list; never empty."
  type        = list(string)
  sensitive   = true
}

variable "cloudflare_api_token" {
  description = "The Cloudflare token scoped to what this plan needs (docs/design/production-cell.md §5)."
  type        = string
  sensitive   = true
}

variable "scale" {
  description = "Which costed sizing to plan: `demo` (two workspaces) or `first_customers` (ten), docs/design/production-cell.md §6."
  type        = string
  default     = "demo"

  validation {
    condition     = contains(["demo", "first_customers"], var.scale)
    error_message = "The scale is demo or first_customers."
  }
}

variable "workspace_ids" {
  description = "The cell's retail workspaces, as opaque ids (namespace ws-<id>, host api-<id>)."
  type        = set(string)
}

variable "dedicated_pools" {
  description = "A pool per business workspace, keyed by its id (HLD §8); empty in both costed scales."
  type = map(object({
    size  = string
    count = number
  }))
  default = {}
}

variable "access_emails" {
  description = "Per workspace id, the full addresses its Access policy admits."
  type        = map(list(string))
  sensitive   = true
}

variable "alert_emails" {
  description = "Who DigitalOcean's node and uptime alerts page."
  type        = list(string)
  sensitive   = true
}

variable "approved_monthly_usd" {
  description = "The monthly spend the founder approved for this cell (DEC-79, DEC-2064 item 1). 0 until then, which refuses every plan."
  type        = number
  default     = 0
}
