# Every bucket is private and versioned, the archive never expires a version, and Barman's key
# reaches the archive bucket alone (DEC-2012 items 3, 8 and 9).

mock_provider "digitalocean" {}

variables {
  prefix      = "mandate"
  environment = "staging"
  region      = "nyc3"
}

run "buckets_are_private_and_versioned" {
  command = plan

  assert {
    condition = alltrue([
      for bucket in [digitalocean_spaces_bucket.state, digitalocean_spaces_bucket.journal_archive] :
      bucket.acl == "private" && bucket.versioning[0].enabled
    ])
    error_message = "Every bucket must be private and versioned."
  }

  assert {
    condition     = digitalocean_spaces_bucket.state.name == "mandate-tofu-state-staging"
    error_message = "The state bucket's name is the one the backend and the runbook use."
  }

  assert {
    condition = alltrue([
      for rule in digitalocean_spaces_bucket.journal_archive.lifecycle_rule :
      length(rule.expiration) == 0 && length(rule.noncurrent_version_expiration) == 0
    ])
    error_message = "The journal archive must never expire an object or an old version."
  }

  assert {
    condition = (
      length(digitalocean_spaces_key.journal_archive.grant) == 1 &&
      digitalocean_spaces_key.journal_archive.grant[0].permission == "readwrite" &&
      digitalocean_spaces_key.journal_archive.grant[0].bucket == "mandate-journal-archive-staging"
    )
    error_message = "Barman's key must be read-write on the archive bucket alone."
  }
}
