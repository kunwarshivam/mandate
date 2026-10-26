# The cell's estimate (DEC-2064 items 1 and 9), against a mocked size list at the prices read on
# 2026-10-26: the demo-scale sum, the Access seat cliff, and a size the region does not offer.

mock_provider "digitalocean" {
  mock_data "digitalocean_sizes" {
    defaults = {
      sizes = [
        { slug = "s-2vcpu-4gb", price_monthly = 24, price_hourly = 0.03571, available = true, vcpus = 2, memory = 4096, disk = 80, transfer = 4, regions = ["sfo2"], gpu_info = [] },
        { slug = "s-4vcpu-8gb", price_monthly = 48, price_hourly = 0.07143, available = true, vcpus = 4, memory = 8192, disk = 160, transfer = 5, regions = ["sfo2"], gpu_info = [] },
      ]
    }
  }
}

variables {
  region              = "sfo2"
  pools               = [{ size = "s-2vcpu-4gb", count = 3 }, { size = "s-4vcpu-8gb", count = 2 }]
  ha_control_plane    = true
  block_storage_gb    = 105
  r2_storage_gb       = 25
  r2_class_a_millions = 0.02
  access_users        = 5
  uptime_checks       = 2
}

run "the_demo_scale_estimate" {
  command = plan

  assert {
    condition     = abs(output.estimate_usd - 224.965) < 0.001
    error_message = "Demo scale: 168 of nodes, 40 HA, 10.50 block, 5 Spaces, 0.465 R2, 0 Access, 1 uptime."
  }

  assert {
    condition     = output.breakdown_usd.nodes == 168 && output.breakdown_usd.access == 0 && output.breakdown_usd.uptime == 1
    error_message = "The node, Access and uptime lines."
  }
}

run "seats_past_the_free_plan_are_all_paid" {
  command = plan

  variables {
    access_users = 51
  }

  assert {
    condition     = output.breakdown_usd.access == 357
    error_message = "Past 50 people every seat is 7 USD."
  }
}


run "refuses_a_size_the_region_does_not_offer" {
  command = plan

  variables {
    pools = [{ size = "s-8vcpu-16gb", count = 3 }]
  }

  expect_failures = [terraform_data.estimate]
}
