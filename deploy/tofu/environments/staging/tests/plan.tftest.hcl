# Staging's own values plan against mocked providers, with no credential: the pinned version is
# one DOKS offers, the estimate is under staging's share of the cap, and the API host is staging's
# (DEC-2012 item 8). Each rule is tested in its module's tests.

mock_provider "digitalocean" {
  mock_data "digitalocean_kubernetes_versions" {
    defaults = {
      valid_versions = ["1.35.9-do.2"]
    }
  }

  mock_data "digitalocean_sizes" {
    defaults = {
      sizes = [{ slug = "s-2vcpu-4gb", price_monthly = 24, price_hourly = 0.03571, available = true, vcpus = 2, memory = 4096, disk = 80, transfer = 4, regions = ["nyc3", "sfo2"], gpu_info = [] }]
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
  adopt_state_bucket         = false
}

run "staging_plans_under_its_cap" {
  command = plan

  assert {
    condition     = output.monthly_estimate_usd == 57
    error_message = "Staging should estimate 57 USD a month at list price."
  }

  assert {
    condition     = output.archive_bucket == "mandate-journal-archive-west-staging"
    error_message = "The archive bucket is staging's."
  }

  assert {
    condition     = output.archive_destination_path == "s3://mandate-journal-archive-west-staging/journal-db" && output.archive_jurisdiction == "default" && output.cloudflare_account_id == "fedcba9876543210fedcba9876543210"
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

# The listing provider 5.27.0 returned in the founder's first plan: one account, once per page.
run "a_listing_that_repeats_one_account_is_that_account" {
  command = plan

  override_data {
    target = data.cloudflare_accounts.visible
    values = {
      result = [{ id = "fedcba9876543210fedcba9876543210", name = "founder", type = "standard", created_on = "2026-01-01T00:00:00Z", managed_by = { parent_org_id = "", parent_org_name = "" }, settings = { abuse_contact_email = "", enforce_twofactor = true } }, { id = "fedcba9876543210fedcba9876543210", name = "founder", type = "standard", created_on = "2026-01-01T00:00:00Z", managed_by = { parent_org_id = "", parent_org_name = "" }, settings = { abuse_contact_email = "", enforce_twofactor = true } }, { id = "", name = "founder", type = "standard", created_on = "2026-01-01T00:00:00Z", managed_by = { parent_org_id = "", parent_org_name = "" }, settings = { abuse_contact_email = "", enforce_twofactor = true } }]
    }
  }

  assert {
    condition     = nonsensitive(output.cloudflare_account_id) == "fedcba9876543210fedcba9876543210"
    error_message = "The repeated account is the one account the plan uses."
  }
}

run "refuses_a_repeating_listing_of_two_accounts" {
  command = plan

  override_data {
    target = data.cloudflare_accounts.visible
    values = {
      result = [{ id = "fedcba9876543210fedcba9876543210", name = "founder", type = "standard", created_on = "2026-01-01T00:00:00Z", managed_by = { parent_org_id = "", parent_org_name = "" }, settings = { abuse_contact_email = "", enforce_twofactor = true } }, { id = "0123456789abcdef0123456789abcdef", name = "founder", type = "standard", created_on = "2026-01-01T00:00:00Z", managed_by = { parent_org_id = "", parent_org_name = "" }, settings = { abuse_contact_email = "", enforce_twofactor = true } }, { id = "fedcba9876543210fedcba9876543210", name = "founder", type = "standard", created_on = "2026-01-01T00:00:00Z", managed_by = { parent_org_id = "", parent_org_name = "" }, settings = { abuse_contact_email = "", enforce_twofactor = true } }, { id = "0123456789abcdef0123456789abcdef", name = "founder", type = "standard", created_on = "2026-01-01T00:00:00Z", managed_by = { parent_org_id = "", parent_org_name = "" }, settings = { abuse_contact_email = "", enforce_twofactor = true } }]
    }
  }

  expect_failures = [data.cloudflare_accounts.visible]
}

run "refuses_a_listing_with_no_account" {
  command = plan

  override_data {
    target = data.cloudflare_accounts.visible
    values = {
      result = [{ id = "", name = "founder", type = "standard", created_on = "2026-01-01T00:00:00Z", managed_by = { parent_org_id = "", parent_org_name = "" }, settings = { abuse_contact_email = "", enforce_twofactor = true } }]
    }
  }

  expect_failures = [data.cloudflare_accounts.visible]
}

run "refuses_a_token_that_sees_two_accounts" {
  command = plan

  override_data {
    target = data.cloudflare_accounts.visible
    values = {
      result = [{ id = "fedcba9876543210fedcba9876543210", name = "founder", type = "standard", created_on = "2026-01-01T00:00:00Z", managed_by = { parent_org_id = "", parent_org_name = "" }, settings = { abuse_contact_email = "", enforce_twofactor = true } }, { id = "0123456789abcdef0123456789abcdef", name = "other", type = "standard", created_on = "2026-01-01T00:00:00Z", managed_by = { parent_org_id = "", parent_org_name = "" }, settings = { abuse_contact_email = "", enforce_twofactor = true } }]
    }
  }

  expect_failures = [data.cloudflare_accounts.visible]
}

run "staging_may_rely_on_the_window_alone" {
  command = plan

  variables {
    kube_api_allowed_addresses = []
  }

  assert {
    condition     = length(var.kube_api_allowed_addresses) == 0 && output.monthly_estimate_usd == 57 && output.archive_bucket == "mandate-journal-archive-west-staging"
    error_message = "Staging plans whole with no founder CIDR: the root refuses nothing, and the cluster's own test shows its firewall then admits nobody."
  }
}

run "records_the_commit_it_applies" {
  command = plan

  variables {
    commit_sha = "0123456789abcdef0123456789abcdef01234567"
  }

  assert {
    condition     = terraform_data.applied_commit.input == "0123456789abcdef0123456789abcdef01234567"
    error_message = "The run records the commit it applies, so the next run can list what changed since."
  }
}

run "refuses_a_short_commit" {
  command = plan

  variables {
    commit_sha = "0123456"
  }

  expect_failures = [var.commit_sha]
}

# DEC-2047: the cluster, its VPC and the state bucket are in the US West, in sfo2.
run "the_cluster_and_the_state_run_in_sfo2" {
  command = plan

  assert {
    condition     = module.cell.regions == { vpc = "sfo2", cluster = "sfo2", state_bucket = "sfo2" }
    error_message = "Staging's VPC, cluster and state bucket are in sfo2."
  }
}
