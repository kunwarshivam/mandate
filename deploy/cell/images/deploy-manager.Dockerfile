# The deployment manager's image for the local and CI cell (DEC-1170, DEC-1172): `mandate-deployd`
# as this change built it, a statically linked binary on an empty base, non-root, with no shell and
# no other file, as mandate.Dockerfile builds the CLI. It reaches the API server with the cluster's
# own CA from its service account, so it needs no CA bundle. `cargo xtask cell up` builds it from a
# context holding only the binary, and never with the `live` feature.
FROM scratch
COPY mandate-deployd /mandate-deployd
USER 65532:65532
ENTRYPOINT ["/mandate-deployd"]
