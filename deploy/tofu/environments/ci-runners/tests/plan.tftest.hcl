# The root's plan (DEC-2065): the committed values plan within the cap.

mock_provider "digitalocean" {
  mock_data "digitalocean_sizes" {
    defaults = {
      sizes = [{ slug = "s-4vcpu-8gb", price_monthly = 48, price_hourly = 0.07143, available = true, vcpus = 4, memory = 8192, disk = 160, transfer = 5, regions = ["sfo2"], gpu_info = [] }]
    }
  }
}

variables {
  state_passphrase           = "a-test-passphrase-of-at-least-thirty-two-characters"
  github_app_client_id       = "Iv23liExample0"
  github_app_installation_id = "12345678"
  github_app_private_key     = <<-KEY
    -----BEGIN RSA PRIVATE KEY-----
    not-a-key
    -----END RSA PRIVATE KEY-----
  KEY
}

run "the_committed_fleet_plans_within_the_cap" {
  command = plan

  assert {
    condition     = output.estimate_usd == 144
    error_message = "The committed fleet should estimate 144 USD."
  }

  assert {
    condition     = length(output.runner_names) == 3
    error_message = "Three runners."
  }
}

run "one_runner_is_a_48_usd_canary" {
  command = plan

  variables {
    runner_count = 1
  }

  assert {
    condition     = output.estimate_usd == 48
    error_message = "One s-4vcpu-8gb canary should estimate 48 USD."
  }

  assert {
    condition     = length(output.runner_names) == 1
    error_message = "The canary plan should create one runner."
  }
}

run "runner_count_stays_within_the_approved_fleet" {
  command = plan

  variables {
    runner_count = 4
  }

  expect_failures = [var.runner_count]
}
