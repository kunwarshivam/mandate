# The runners' plan (DEC-2065): the fleet within its cap, a firewall that admits nothing and lets
# out only DNS, NTP and ports 80 and 443, and a boot script that carries the key but is sensitive.

mock_provider "digitalocean" {
  mock_data "digitalocean_sizes" {
    defaults = {
      sizes = [{ slug = "s-4vcpu-8gb", price_monthly = 48, price_hourly = 0.07143, available = true, vcpus = 4, memory = 8192, disk = 160, transfer = 5, regions = ["sfo2"], gpu_info = [] }]
    }
  }
}

variables {
  region                     = "sfo2"
  size                       = "s-4vcpu-8gb"
  runner_count               = 3
  cap_usd                    = 150
  repository                 = "kunwarshivam/mandate"
  runner_label               = "mandate-ci-do"
  github_app_client_id       = "Iv23liExample0"
  github_app_installation_id = "12345678"
  github_app_private_key     = <<-KEY
    -----BEGIN RSA PRIVATE KEY-----
    not-a-key
    -----END RSA PRIVATE KEY-----
  KEY
  runner_version             = "2.338.1"
  runner_sha256              = "08e9ccb3604d2462c24b5e33604dc30810296ca15cc65f23cffd4317eb144c37"
  kubectl_version            = "1.35.9"
  kubectl_sha256             = "3cfeaf80be482b435b0aa214aff6e0b2c312ee23c0ff20810c75517b6004c6eb"
  sysbox_version             = "0.7.1"
  sysbox_sha256              = "9d6d5484f980d0a17f86c492c1262015c2afb66280bdb97215b79fde6a0261c5"
  base_image                 = "ubuntu:24.04@sha256:68ee22004a9109b44973313b02ff7b1de250f1de0f5ecb628d73f579aaff33ac"
  toolchain_ref              = "main"
}

run "three_runners_plan_within_the_cap" {
  command = plan

  assert {
    condition     = output.estimate_usd == 144
    error_message = "Three s-4vcpu-8gb droplets at 48 USD should estimate 144 USD."
  }

  assert {
    condition     = length(digitalocean_droplet.runner) == 3
    error_message = "One droplet per runner."
  }

  assert {
    condition     = alltrue([for d in digitalocean_droplet.runner : !d.ipv6 && !d.backups && !d.monitoring && !d.droplet_agent])
    error_message = "A runner has no IPv6, backups, monitoring or droplet agent."
  }

  assert {
    condition     = alltrue([for d in digitalocean_droplet.runner : issensitive(d.user_data)])
    error_message = "The boot script carries the App key, so it must be sensitive."
  }
}

run "the_firewall_admits_nothing_and_lets_out_only_what_jobs_need" {
  command = plan

  assert {
    condition     = length(digitalocean_firewall.runner.inbound_rule) == 0
    error_message = "The runners admit nothing inbound."
  }

  assert {
    condition     = toset([for r in digitalocean_firewall.runner.outbound_rule : "${r.protocol}/${r.port_range}"]) == toset(["tcp/443", "tcp/80", "udp/53", "tcp/53", "udp/123"])
    error_message = "The runners' egress is DNS, NTP and ports 80 and 443 only."
  }
}

# A fleet whose estimate passes the cap fails the plan in modules/budget's precondition, which
# its own cap.tftest.hcl tests; expect_failures cannot name a resource inside a called module.
run "refuses_a_cap_above_the_founders_150" {
  command = plan

  variables {
    cap_usd = 151
  }

  expect_failures = [var.cap_usd]
}

run "refuses_a_label_that_other_runners_carry" {
  command = plan

  variables {
    runner_label = "self-hosted"
  }

  expect_failures = [var.runner_label]
}

run "refuses_a_base_image_not_pinned_by_digest" {
  command = plan

  variables {
    base_image = "ubuntu:24.04"
  }

  expect_failures = [var.base_image]
}
