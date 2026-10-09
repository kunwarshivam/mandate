# Feature map

What exists, where it lives, what proves it, and how to run it: one file a feature in this
directory, each opening with its `# ` title. There is no hand-kept list:
`cargo xtask feature-map --index` prints every feature file with its title.

`cargo xtask ci lint` checks the map against the workspace: every workspace crate is named in some
feature as `` `crate-name` ``, every reference-case suite in `fixtures/refcases/` by its path, every
repository path a feature names exists, and every feature file opens with its title.

A PR adding a feature adds a file named for its title (`features/<slug>.md`); a PR changing a
feature edits only that feature's file. This directory was the single `feature-map.md` until
most open PRs edited it at once and conflicted.
