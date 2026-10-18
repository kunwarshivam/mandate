# The state bucket is private and versioned, and is the only bucket in Spaces (DEC-2012 items 5
# and 13).

mock_provider "digitalocean" {}

variables {
  prefix      = "mandate"
  environment = "staging"
  region      = "nyc3"
}

run "the_state_bucket_is_private_and_versioned" {
  command = plan

  assert {
    condition     = digitalocean_spaces_bucket.state.acl == "private" && digitalocean_spaces_bucket.state.versioning[0].enabled
    error_message = "The state bucket must be private and versioned."
  }

  assert {
    condition     = digitalocean_spaces_bucket.state.name == "mandate-tofu-state-staging"
    error_message = "The state bucket's name is the one the backend and the runbook use."
  }

  assert {
    condition     = output.state_bucket == digitalocean_spaces_bucket.state.name
    error_message = "The module's one output is the state bucket."
  }
}
