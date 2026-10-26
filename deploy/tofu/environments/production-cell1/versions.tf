# Customer production, cell 1 (DEC-2064, Proposed: founder-reserved spending). Never applied:
# infra.yml dispatches only `staging` and `production`, `cargo xtask infra` refuses any other
# environment, and the costing module refuses every plan until the founder's approved amount is
# set. The state would sit in production's own bucket under its own key, encrypted on the client
# exactly as production's is. CI's checks run `tofu init -backend=false`.

terraform {
  required_version = "1.13.1"

  required_providers {
    digitalocean = {
      source  = "digitalocean/digitalocean"
      version = "2.103.0"
    }
    cloudflare = {
      source  = "cloudflare/cloudflare"
      version = "5.27.0"
    }
  }

  backend "s3" {
    bucket                      = "mandate-tofu-state-west-production"
    key                         = "production-cell1/tofu.tfstate"
    region                      = "us-east-1"
    endpoints                   = { s3 = "https://sfo2.digitaloceanspaces.com" }
    skip_credentials_validation = true
    skip_region_validation      = true
    skip_requesting_account_id  = true
    skip_metadata_api_check     = true
    skip_s3_checksum            = true
  }

  encryption {
    key_provider "pbkdf2" "state" {
      passphrase    = var.state_passphrase
      hash_function = "sha512"
      iterations    = 600000
      salt_length   = 32
      key_length    = 32
    }

    method "aes_gcm" "state" {
      keys = key_provider.pbkdf2.state
    }

    state {
      method   = method.aes_gcm.state
      enforced = true
    }

    plan {
      method   = method.aes_gcm.state
      enforced = true
    }
  }
}
