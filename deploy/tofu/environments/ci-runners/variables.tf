# Every value here is the founder's, entered at apply time through TF_VAR_* in his own shell, never
# committed: docs/runbooks/ci-runners.md names each one.

variable "state_passphrase" {
  description = "Encrypts this root's state and plans. Keep a copy in a password manager."
  type        = string
  sensitive   = true

  validation {
    condition     = length(var.state_passphrase) >= 32
    error_message = "The state passphrase must be at least 32 characters."
  }
}

variable "github_app_client_id" {
  description = "The runner App's client id (Settings, Developer settings, GitHub Apps)."
  type        = string
}

variable "github_app_installation_id" {
  description = "The number at the end of the App installation's settings URL."
  type        = string
}

variable "github_app_private_key" {
  description = "The runner App's private key, the whole PEM."
  type        = string
  sensitive   = true
}
