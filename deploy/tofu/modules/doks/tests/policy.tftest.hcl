# The cluster is pinned, small and fenced, and refuses a version DOKS does not offer, a floating
# version, and a control plane open wider than the founder's CIDRs (DEC-2012 items 5 and 8).

mock_provider "digitalocean" {
  mock_data "digitalocean_kubernetes_versions" {
    defaults = {
      valid_versions = ["1.35.9-do.2"]
    }
  }
}

variables {
  name                       = "mandate-staging"
  region                     = "nyc3"
  vpc_id                     = "c0ffee00-0000-4000-8000-000000000000"
  node_tag                   = "mandate-staging-nodes"
  kubernetes_version         = "1.35.9-do.2"
  node_size                  = "s-2vcpu-4gb"
  kube_api_allowed_addresses = ["203.0.113.7/32"]
}

run "cluster_is_pinned_small_and_fenced" {
  command = plan

  assert {
    condition     = digitalocean_kubernetes_cluster.this.version == "1.35.9-do.2"
    error_message = "The cluster does not run the pinned version."
  }

  assert {
    condition     = digitalocean_kubernetes_cluster.this.auto_upgrade == false && digitalocean_kubernetes_cluster.this.ha == false
    error_message = "Auto-upgrade and the HA control plane must be off."
  }

  assert {
    condition     = digitalocean_kubernetes_cluster.this.node_pool[0].node_count == 2 && digitalocean_kubernetes_cluster.this.node_pool[0].auto_scale == false
    error_message = "The node pool must be exactly two nodes with autoscaling off."
  }

  assert {
    condition     = digitalocean_kubernetes_cluster.this.control_plane_firewall[0].enabled && digitalocean_kubernetes_cluster.this.control_plane_firewall[0].allowed_addresses == tolist(["203.0.113.7/32"])
    error_message = "The control plane's firewall must be on and admit only the configured CIDRs."
  }

  assert {
    condition     = digitalocean_kubernetes_cluster.this.vpc_uuid == var.vpc_id && digitalocean_kubernetes_cluster.this.node_pool[0].tags == toset([var.node_tag])
    error_message = "The cluster sits in the environment's VPC and its nodes carry the firewall's tag."
  }
}

run "refuses_a_version_doks_does_not_offer" {
  command = plan

  variables {
    kubernetes_version = "1.36.0-do.0"
  }

  expect_failures = [digitalocean_kubernetes_cluster.this]
}

run "refuses_a_floating_version" {
  command = plan

  variables {
    kubernetes_version = "latest"
  }

  expect_failures = [var.kubernetes_version]
}

run "refuses_a_world_control_plane" {
  command = plan

  variables {
    kube_api_allowed_addresses = ["203.0.113.7/32", "::/0"]
  }

  expect_failures = [var.kube_api_allowed_addresses]
}

run "refuses_a_wide_control_plane_range" {
  command = plan

  variables {
    kube_api_allowed_addresses = ["203.0.0.0/16"]
  }

  expect_failures = [var.kube_api_allowed_addresses]
}

run "an_empty_list_admits_nobody" {
  command = plan

  variables {
    kube_api_allowed_addresses = []
  }

  assert {
    condition     = digitalocean_kubernetes_cluster.this.control_plane_firewall[0].enabled && digitalocean_kubernetes_cluster.this.control_plane_firewall[0].allowed_addresses == tolist(["192.0.2.1/32"])
    error_message = "With no CIDR and no window the firewall stays on and admits only TEST-NET-1, never everyone."
  }
}

run "an_empty_list_admits_the_window_alone" {
  command = plan

  variables {
    kube_api_allowed_addresses = []
    bootstrap_runner_cidr      = "198.51.100.23/32"
  }

  assert {
    condition     = digitalocean_kubernetes_cluster.this.control_plane_firewall[0].allowed_addresses == tolist(["198.51.100.23/32"])
    error_message = "With no founder CIDR the window is the only address admitted."
  }
}

run "opens_the_window_to_one_runner_only" {
  command = plan

  variables {
    bootstrap_runner_cidr = "198.51.100.23/32"
  }

  assert {
    condition     = digitalocean_kubernetes_cluster.this.control_plane_firewall[0].allowed_addresses == tolist(["203.0.113.7/32", "198.51.100.23/32"])
    error_message = "The window adds exactly the runner's /32 to the founder's addresses."
  }
}

run "the_window_is_closed_by_default" {
  command = plan

  assert {
    condition     = var.bootstrap_runner_cidr == "" && length(digitalocean_kubernetes_cluster.this.control_plane_firewall[0].allowed_addresses) == 1
    error_message = "With no runner the allowlist is the founder's alone."
  }
}

run "refuses_a_wider_window" {
  command = plan

  variables {
    bootstrap_runner_cidr = "198.51.100.0/24"
  }

  expect_failures = [var.bootstrap_runner_cidr]
}

run "refuses_a_private_runner" {
  command = plan

  variables {
    bootstrap_runner_cidr = "10.1.2.3/32"
  }

  expect_failures = [var.bootstrap_runner_cidr]
}

run "refuses_two_runners" {
  command = plan

  variables {
    bootstrap_runner_cidr = "198.51.100.23/32,198.51.100.24/32"
  }

  expect_failures = [var.bootstrap_runner_cidr]
}

run "refuses_an_ipv6_runner" {
  command = plan

  variables {
    bootstrap_runner_cidr = "2001:db8::1/128"
  }

  expect_failures = [var.bootstrap_runner_cidr]
}

run "refuses_a_loopback_runner" {
  command = plan

  variables {
    bootstrap_runner_cidr = "127.0.0.1/32"
  }

  expect_failures = [var.bootstrap_runner_cidr]
}

run "refuses_a_cgnat_runner" {
  command = plan

  variables {
    bootstrap_runner_cidr = "100.64.1.2/32"
  }

  expect_failures = [var.bootstrap_runner_cidr]
}

run "refuses_a_link_local_runner" {
  command = plan

  variables {
    bootstrap_runner_cidr = "169.254.1.1/32"
  }

  expect_failures = [var.bootstrap_runner_cidr]
}

run "refuses_a_unspecified_runner" {
  command = plan

  variables {
    bootstrap_runner_cidr = "0.0.0.0/32"
  }

  expect_failures = [var.bootstrap_runner_cidr]
}

run "refuses_a_slash_31_runner" {
  command = plan

  variables {
    bootstrap_runner_cidr = "198.51.100.22/31"
  }

  expect_failures = [var.bootstrap_runner_cidr]
}

run "refuses_a_leading_zero_runner" {
  command = plan

  variables {
    bootstrap_runner_cidr = "198.051.100.23/32"
  }

  expect_failures = [var.bootstrap_runner_cidr]
}

run "refuses_a_multicast_runner" {
  command = plan

  variables {
    bootstrap_runner_cidr = "224.0.0.1/32"
  }

  expect_failures = [var.bootstrap_runner_cidr]
}
