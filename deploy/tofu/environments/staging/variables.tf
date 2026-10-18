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
}

variable "cloudflare_account_id" {
  description = "CLOUDFLARE_ACCOUNT_ID."
  type        = string
}

variable "access_allowed_emails" {
  description = "ACCESS_ALLOWED_EMAILS: who Access admits to the API host, as a JSON list."
  type        = list(string)
}

variable "adopt_state_bucket" {
  description = "Adopts the state bucket the founder made by hand; only the tests, whose mocked providers cannot import, turn it off."
  type        = bool
  default     = true
}
