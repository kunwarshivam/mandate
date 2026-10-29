# A customer production cell's cluster, planned against a mocked provider (DEC-2064 item 3): the HA
# control plane, the pinned version, auto-upgrade off, the control plane fenced to the founder's
# CIDRs, fixed counts, and taints that keep the workspace and dedicated pools to their own pods.

mock_provider "digitalocean" {
  mock_data "digitalocean_kubernetes_versions" {
    defaults = {
      valid_versions = ["1.35.9-do.2"]
    }
  }
}

variables {
  name                       = "mandate-prod-cell1"
  region                     = "sfo2"
  vpc_id                     = "vpc-1"
  node_tag                   = "mandate-prod-cell1-nodes"
  kubernetes_version         = "1.35.9-do.2"
  ha_control_plane           = true
  system_pool                = { size = "s-2vcpu-4gb", count = 3 }
  workspace_pool             = { size = "s-4vcpu-8gb", count = 2 }
  kube_api_allowed_addresses = ["203.0.113.7/32"]
}

run "the_cluster_is_fenced_and_pinned" {
  command = plan

  assert {
    condition     = digitalocean_kubernetes_cluster.this.ha && !digitalocean_kubernetes_cluster.this.auto_upgrade && !digitalocean_kubernetes_cluster.this.registry_integration && digitalocean_kubernetes_cluster.this.version == "1.35.9-do.2"
    error_message = "HA on, auto-upgrade and registry integration off, the version pinned."
  }

  assert {
    condition     = digitalocean_kubernetes_cluster.this.control_plane_firewall[0].enabled && length(nonsensitive(digitalocean_kubernetes_cluster.this.control_plane_firewall[0].allowed_addresses)) == 1 && contains(nonsensitive(digitalocean_kubernetes_cluster.this.control_plane_firewall[0].allowed_addresses), "203.0.113.7/32")
    error_message = "The control plane admits only the founder's CIDRs."
  }

  assert {
    condition     = !digitalocean_kubernetes_cluster.this.node_pool[0].auto_scale && digitalocean_kubernetes_cluster.this.node_pool[0].node_count == 3 && !digitalocean_kubernetes_node_pool.workspaces.auto_scale && digitalocean_kubernetes_node_pool.workspaces.node_count == 2
    error_message = "Every count is fixed, so the estimate is the bill."
  }

  assert {
    condition     = one(digitalocean_kubernetes_node_pool.workspaces.taint).key == "mandate.dev/pool" && one(digitalocean_kubernetes_node_pool.workspaces.taint).effect == "NoSchedule"
    error_message = "Only workspace pods tolerate the workspace pool."
  }

  assert {
    condition     = length(output.pools) == 2
    error_message = "With no business workspace there are two pools."
  }
}

run "a_business_workspace_gets_its_own_tainted_pool" {
  command = plan

  variables {
    dedicated_pools = { acme1 = { size = "s-4vcpu-8gb", count = 1 } }
  }

  assert {
    condition     = one(digitalocean_kubernetes_node_pool.dedicated["acme1"].taint).key == "mandate.dev/workspace" && one(digitalocean_kubernetes_node_pool.dedicated["acme1"].taint).value == "acme1" && length(output.pools) == 3
    error_message = "A dedicated pool admits only its own workspace and is counted."
  }
}

run "refuses_a_control_plane_without_ha" {
  command = plan

  variables {
    ha_control_plane = false
  }

  expect_failures = [var.ha_control_plane]
}

run "refuses_a_two_node_system_pool" {
  command = plan

  variables {
    system_pool = { size = "s-2vcpu-4gb", count = 2 }
  }

  expect_failures = [var.system_pool]
}

run "refuses_an_open_control_plane" {
  command = plan

  variables {
    kube_api_allowed_addresses = ["0.0.0.0/0"]
  }

  expect_failures = [var.kube_api_allowed_addresses]
}

run "refuses_no_cidr" {
  command = plan

  variables {
    kube_api_allowed_addresses = []
  }

  expect_failures = [var.kube_api_allowed_addresses]
}

run "refuses_a_version_doks_does_not_offer" {
  command = plan

  variables {
    kubernetes_version = "1.34.1-do.1"
  }

  expect_failures = [digitalocean_kubernetes_cluster.this]
}
