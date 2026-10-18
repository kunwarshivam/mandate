# Every value here comes from the GitHub environment (infra.yml sets TF_VAR_*), never a committed
# file: the runbook names each one.

variable "state_passphrase" {
  description = "TOFU_STATE_PASSPHRASE: encrypts this environment's state and plans."
  type        = string
  sensitive   = true

  validation {
    condition     = length(var.state_passphrase) >= 32
    error_message = "The state passphrase must be at least 32 characters."
  }
}

variable "kube_api_allowed_addresses" {
  description = "KUBE_API_ALLOWED_CIDRS: the only CIDRs the control plane admits, as a JSON list."
  type        = list(string)
  sensitive   = true

  validation {
    condition     = length(var.kube_api_allowed_addresses) > 0
    error_message = "Production's control plane needs at least one of the founder's CIDRs; only staging may rely on a run's window alone."
  }
}

variable "cloudflare_api_token" {
  description = "CF_INFRA_API_TOKEN: the Cloudflare token scoped to what this plan needs."
  type        = string
  sensitive   = true
}

variable "adopt_state_bucket" {
  description = "Adopts the state bucket the founder made by hand; only the tests, whose mocked providers cannot import, turn it off."
  type        = bool
  default     = true
}

variable "api_origin" {
  description = "Where cloudflared sends the API host's requests: the API's loopback listener, with cloudflared as a sidecar in the API pod (lane GO, E21-58)."
  type        = string
  default     = "http://127.0.0.1:8080"

  validation {
    condition     = can(regex("^http://(127[.]0[.]0[.]1|localhost|[a-z0-9-]+([.][a-z0-9-]+)*[.]svc[.]cluster[.]local):[0-9]{1,5}$", var.api_origin))
    error_message = "The API origin is a loopback or in-cluster Service address with a port, never a public host."
  }
}

variable "bootstrap_runner_cidr" {
  description = "Set only by `cargo xtask infra apply` for its own run, then cleared (DEC-2028)."
  type        = string
  sensitive   = true
  default     = ""
}

variable "commit_sha" {
  description = "The commit `cargo xtask infra` plans and applies, which the founder named when dispatching it; state keeps the last one applied, so the next run lists what changed since."
  type        = string
  default     = ""

  validation {
    condition     = var.commit_sha == "" || can(regex("^[0-9a-f]{40}$", var.commit_sha))
    error_message = "The commit is empty or a full 40-character lower-case hex SHA."
  }
}
