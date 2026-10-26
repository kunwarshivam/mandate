# Customer production cell 1's two costed scales, planned against mocked providers with no
# credential (DEC-2064 item 1): each estimate is the one docs/design/production-cell.md §6 tables,
# and with no approval, the default, the plan refuses.

mock_provider "digitalocean" {
  mock_data "digitalocean_kubernetes_versions" {
    defaults = {
      valid_versions = ["1.35.9-do.1"]
    }
  }

  mock_data "digitalocean_sizes" {
    defaults = {
      sizes = [
        { slug = "s-2vcpu-4gb", price_monthly = 24, price_hourly = 0.03571, available = true, vcpus = 2, memory = 4096, disk = 80, transfer = 4, regions = ["sfo2"], gpu_info = [] },
        { slug = "s-4vcpu-8gb", price_monthly = 48, price_hourly = 0.07143, available = true, vcpus = 4, memory = 8192, disk = 160, transfer = 5, regions = ["sfo2"], gpu_info = [] },
      ]
    }
  }
}

mock_provider "cloudflare" {
  mock_data "cloudflare_accounts" {
    defaults = {
      result = [{ id = "fedcba9876543210fedcba9876543210", name = "founder", type = "standard", created_on = "2026-01-01T00:00:00Z", managed_by = { parent_org_id = "", parent_org_name = "" }, settings = { abuse_contact_email = "", enforce_twofactor = true } }]
    }
  }

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
  workspace_ids              = ["ws1", "ws2"]
  access_emails = {
    ws1 = ["founder@example.com", "ops1@example.com"]
    ws2 = ["partner1@example.com", "partner2@example.com", "founder@example.com"]
  }
  alert_emails         = ["founder@example.com"]
  approved_monthly_usd = 250
}

run "demo_scale" {
  command = plan

  assert {
    condition     = output.workspace_nodes == 2 && output.block_storage_gb == 105
    error_message = "Two workspaces: one node of load and one spare; 60 GB journal, 15 vault, 20 Prometheus, 10 artifacts."
  }

  assert {
    condition     = abs(output.monthly_estimate_usd - 224.965) < 0.001
    error_message = "Demo scale is 224.965 USD a month: 72 system, 96 workspace, 40 HA, 10.50 block, 5 Spaces, 0.465 R2, 1 uptime."
  }
}

run "first_customers_scale" {
  command = plan

  variables {
    scale                = "first_customers"
    workspace_ids        = ["ws1", "ws2", "ws3", "ws4", "ws5", "ws6", "ws7", "ws8", "ws9", "ws10"]
    access_emails        = { for n in range(1, 11) : "ws${n}" => ["owner${n}@example.com", "approver${n}@example.com", "founder@example.com"] }
    approved_monthly_usd = 570
  }

  assert {
    condition     = output.workspace_nodes == 6 && output.block_storage_gb == 265
    error_message = "Ten workspaces: five nodes of load and one spare; 150 GB journal, 15 vault, 50 Prometheus, 50 artifacts."
  }

  assert {
    condition     = abs(output.monthly_estimate_usd - 515.35) < 0.001
    error_message = "First customers is 515.35 USD a month: 144 system, 288 workspace, 40 HA, 26.50 block, 5 Spaces, 2.85 R2, 9 uptime; 21 people, inside Access's free 50."
  }
}

run "refuses_to_plan_before_the_founder_approves" {
  command = plan

  variables {
    approved_monthly_usd = 0
  }

  expect_failures = [terraform_data.founder_approval]
}
