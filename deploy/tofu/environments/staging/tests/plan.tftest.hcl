# Staging's own values plan against mocked providers, with no credential: the pinned version is
# one DOKS offers, the estimate is under staging's share of the cap, and the API host is staging's
# (DEC-2012 item 8). Each rule is tested in its module's tests.

mock_provider "digitalocean" {
  mock_data "digitalocean_kubernetes_versions" {
    defaults = {
      valid_versions = ["1.35.1-do.0"]
    }
  }

  mock_data "digitalocean_sizes" {
    defaults = {
      sizes = [{ slug = "s-2vcpu-4gb", price_monthly = 24, price_hourly = 0.03571, available = true, vcpus = 2, memory = 4096, disk = 80, transfer = 4, regions = ["nyc3"], gpu_info = [] }]
    }
  }
}

mock_provider "cloudflare" {
  mock_data "cloudflare_zone" {
    defaults = {
      zone_id = "0123456789abcdef0123456789abcdef"
    }
  }
}

variables {
  state_passphrase           = "a-test-passphrase-that-is-not-a-secret"
  kube_api_allowed_addresses = ["203.0.113.7/32"]
  cloudflare_api_token       = "not-a-token"
  cloudflare_account_id      = "fedcba9876543210fedcba9876543210"
  access_allowed_emails      = ["founder@example.com"]
  adopt_state_bucket         = false
}

run "staging_plans_under_its_cap" {
  command = plan

  assert {
    condition     = output.monthly_estimate_usd == 57
    error_message = "Staging should estimate 57 USD a month at list price."
  }

  assert {
    condition     = output.archive_bucket == "mandate-journal-archive-staging"
    error_message = "The archive bucket is staging's."
  }

  assert {
    condition     = output.archive_destination_path == "s3://mandate-journal-archive-staging/journal-db" && output.archive_jurisdiction == "default" && output.cloudflare_account_id == var.cloudflare_account_id
    error_message = "Lane GO's archive inputs: the destination path, the default jurisdiction and the account id."
  }
}

run "refuses_a_short_passphrase" {
  command = plan

  variables {
    state_passphrase = "too-short"
  }

  expect_failures = [var.state_passphrase]
}

run "refuses_a_public_api_origin" {
  command = plan

  variables {
    api_origin = "https://api.example.com"
  }

  expect_failures = [var.api_origin]
}
