# Production's own values plan against mocked providers, with no credential: the pinned version is
# one DOKS offers, the estimate is under production's share of the cap, and the API host is production's
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
  mock_data "cloudflare_account_api_token_permission_groups_list" {
    defaults = {
      result = [{ id = "c0ffee00c0ffee00c0ffee00c0ffee00", name = "Workers R2 Storage Bucket Item Write", category = "r2", is_selectable = true, scopes = ["com.cloudflare.edge.r2.bucket"] }]
    }
  }

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

run "production_plans_under_its_cap" {
  command = plan

  assert {
    condition     = output.monthly_estimate_usd == 55
    error_message = "Production should estimate 55 USD a month at list price."
  }

  assert {
    condition     = output.archive_bucket == "mandate-journal-archive-production"
    error_message = "The archive bucket is production's."
  }

  assert {
    condition     = output.archive_destination_path == "s3://mandate-journal-archive-production/journal-db" && output.archive_jurisdiction == "default" && output.cloudflare_account_id == var.cloudflare_account_id
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
