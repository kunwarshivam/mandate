# The budget's refusals (DEC-2012 item 7): an estimate over the cap, and a size whose price
# DigitalOcean does not list.

mock_provider "digitalocean" {
  mock_data "digitalocean_sizes" {
    defaults = {
      sizes = [{ slug = "s-2vcpu-4gb", price_monthly = 24, price_hourly = 0.03571, available = true, vcpus = 2, memory = 4096, disk = 80, transfer = 4, regions = ["nyc3"], gpu_info = [] }]
    }
  }
}

variables {
  node_size          = "s-2vcpu-4gb"
  region             = "nyc3"
  node_count         = 2
  spaces_monthly_usd = 5
  block_storage_gb   = 40
  cap_usd            = 65
}

run "plans_at_or_under_the_cap" {
  command = plan

  variables {
    cap_usd = 57
  }

  assert {
    condition     = output.estimate_usd == 57
    error_message = "An estimate equal to the cap should plan."
  }
}

run "refuses_an_estimate_over_the_cap" {
  command = plan

  variables {
    cap_usd = 56
  }

  expect_failures = [terraform_data.cap]
}

run "refuses_an_unlisted_size" {
  command = plan

  override_data {
    target = data.digitalocean_sizes.node
    values = {
      sizes = []
    }
  }

  expect_failures = [terraform_data.cap]
}

# DEC-2044 item 2: a size DigitalOcean does not offer in the cluster's region fails the plan by name.
run "refuses_a_size_the_region_does_not_offer" {
  command = plan

  variables {
    region = "nyc1"
  }

  override_data {
    target = data.digitalocean_sizes.region
    values = {
      sizes = [{ slug = "s-2vcpu-4gb-amd", price_monthly = 28, price_hourly = 0.04167, available = true, vcpus = 2, memory = 4096, disk = 80, transfer = 4, regions = ["nyc1"], gpu_info = [] }]
    }
  }

  expect_failures = [terraform_data.cap]
}
