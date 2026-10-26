# The role provisioner's image (DEC-2350, DEC-2630 item 7): `mandate-provisionerd` as this change
# built it, a statically linked binary on an empty base, non-root, with no shell and no other file,
# as deploy-manager.Dockerfile builds the deployment manager. It trusts only the vault's CA, which
# its vault directory holds, so it needs no CA bundle. It is built from a context holding only the
# binary. The chart never starts it with `--fixture`, and a release build refuses it
# (`cargo xtask ci provisionerd-release`).
FROM scratch
COPY mandate-provisionerd /mandate-provisionerd
USER 65532:65532
ENTRYPOINT ["/mandate-provisionerd"]
