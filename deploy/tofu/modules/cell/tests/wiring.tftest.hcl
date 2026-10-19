# The environment's modules wired together plan against mocked providers, and the estimate the
# plan checks against the cap is the one the parts add up to (DEC-2012 items 1 and 7). Each rule
# is tested in its own module's tests, where the plan's resources are visible.

mock_provider "digitalocean" {
  mock_data "digitalocean_kubernetes_versions" {
    defaults = {
      valid_versions = ["1.35.9-do.1"]
    }
  }

  mock_data "digitalocean_sizes" {
    defaults = {
      sizes = [{ slug = "s-2vcpu-4gb", price_monthly = 24, price_hourly = 0.03571, available = true, vcpus = 2, memory = 4096, disk = 80, transfer = 4, regions = ["nyc3", "sfo2"], gpu_info = [] }]
    }
  }
}

variables {
  environment                = "staging"
  vpc_ip_range               = "10.110.0.0/20"
  kubernetes_version         = "1.35.9-do.1"
  kube_api_allowed_addresses = ["203.0.113.7/32"]
  spaces_monthly_usd         = 5
  block_storage_gb           = 40
  cap_usd                    = 65
}

run "plans_with_the_estimate_under_the_cap" {
  command = plan

  assert {
    condition     = output.monthly_estimate_usd == 57
    error_message = "Two 24 USD nodes, 5 USD of Spaces and 40 GB at 0.10 USD should estimate 57 USD."
  }

  assert {
    condition     = output.state_bucket == "mandate-tofu-state-west-staging"
    error_message = "The state bucket is named for the environment."
  }
}

run "refuses_an_unknown_environment" {
  command = plan

  variables {
    environment = "dev"
  }

  expect_failures = [var.environment]
}

run "refuses_a_cap_over_65" {
  command = plan

  variables {
    cap_usd = 75
  }

  expect_failures = [var.cap_usd]
}

run "refuses_spaces_counted_as_free" {
  command = plan

  variables {
    spaces_monthly_usd = 0
  }

  expect_failures = [var.spaces_monthly_usd]
}

# DEC-2047: a cluster outside the US West is refused.
run "refuses_a_region_outside_the_west" {
  command = plan

  variables {
    region = "nyc3"
  }

  expect_failures = [var.region]
}

# DEC-2047 item 6: a state bucket outside the US West is refused.
run "refuses_a_state_bucket_outside_the_west" {
  command = plan

  variables {
    state_region = "nyc3"
  }

  expect_failures = [var.state_region]
}

run "refuses_nyc1" {
  command = plan

  variables {
    region = "nyc1"
  }

  expect_failures = [var.region]
}

run "refuses_tor1" {
  command = plan

  variables {
    region = "tor1"
  }

  expect_failures = [var.region]
}
