# The release signing key: set it up once, then sign a release

For the founder. The route is the one you decided (DEC-2605 rows 8 and 9); the design is
[DEC-2024](../project/decisions/DEC-2024.md). There is no hardware key. One cosign key pair signs
every release. Its private half lives only in the GitHub environment `release-signing`, and CI
uses it only when you approve a run from `main`. Its public half is in `RELEASE_KEYS`
(`crates/mandate-release/release-keys/cosign.pub`, #1689), so a binary starts at tier `staging` or
`production` only when that key signed it.

## Once: make the key and store it

You need [cosign](https://github.com/sigstore/cosign/releases) v3 on your own machine. Never do
these steps in a Claude session or any other shared machine, and never paste the private half or
its password anywhere but GitHub's secret form.

1. **Generate the pair** in an empty directory:

   ```bash
   mkdir release-key && cd release-key
   cosign generate-key-pair
   ```

   cosign asks for a password twice. Use a long random one from your password manager and save it
   there. This writes `cosign.key`, the encrypted private half, and `cosign.pub`, the public half.

2. **Create the environment.** In the repository, open Settings → Environments → New environment
   and name it `release-signing`. Then:
   - **Required reviewers:** add yourself. Leave "Prevent self-review" off, since you are the one
     who dispatches the run.
   - **Deployment branches and tags:** choose "Selected branches and tags" and add the rule `main`.

3. **Store the two secrets** under that environment's "Environment secrets", not as repository
   secrets:
   - `RELEASE_SIGNING_KEY`: the whole contents of `cosign.key`, from
     `-----BEGIN ENCRYPTED SIGSTORE PRIVATE KEY-----` to the end line.
   - `RELEASE_SIGNING_PASSWORD`: the password from step 1.

4. **Put the private half away.** Keep `cosign.key` only in your password manager, or delete it.
   GitHub holds the working copy, and a new pair can always replace a lost one (see Rotating
   below).

5. **Commit the public half.** The public half is not secret. It is
   `crates/mandate-release/release-keys/cosign.pub`, and a test pins its fingerprint, so replacing
   it is a PR that needs your CODEOWNERS review. For the first key this was #1689. From then on, a
   binary starts outside dev and CI only when that key signed it.

## Each release: sign it

1. Open Actions → `release` → "Run workflow", choose the branch `main`, and run it.
2. The builds and `compare` run as on every `main` commit. Then each arch's `release-signing` job
   waits for your approval. Choose "Review deployments", tick `release-signing`, and approve.
3. Each job rebuilds the release, requires the digest its `build-a` printed, signs every binary
   and `SHA256SUMS`, and then runs the signed `mandate-agentd` at tier `production`. The job
   passes only if that binary starts, so a green job means the release is signed by the key the
   binaries trust.
4. The signatures are kept for 90 days as the run's artifacts `release-signatures-<commit>-amd64`
   and `release-signatures-<commit>-arm64`, with `SHA256SUMS`. To check one by hand:

   ```bash
   cosign verify-blob --key cosign.pub --signature SHA256SUMS.sig \
     --insecure-ignore-tlog=true SHA256SUMS
   ```

**If the job fails at "did not start at tier production",** the key in the environment is not
the one in `RELEASE_KEYS`: the secrets hold a different pair from the committed public half.
Nothing was published, and nothing changes until the two match.

## Rotating or revoking the key

Generate a new pair as above, replace both secrets, and add the new public half to `RELEASE_KEYS`
in a PR, with the fingerprint its test pins. To revoke the old key, remove its public half in the
same PR. Releases it signed then stop starting outside dev and CI once the new binaries roll out. If you think the secret leaked,
delete it from the environment first, then rotate.
