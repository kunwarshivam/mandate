# The CI runners' root (DEC-2065). The founder applies it from his own machine, so its state is
# local, beside this root, and encrypted on the client with a passphrase only he holds, as
# DEC-2012 item 5 encrypts the hosted environments' state. CI's checks run
# `tofu init -backend=false` and never read it.

terraform {
  required_version = "1.13.1"

  required_providers {
    digitalocean = {
      source  = "digitalocean/digitalocean"
      version = "2.103.0"
    }
  }

  backend "local" {
    path = "ci-runners.tfstate"
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
