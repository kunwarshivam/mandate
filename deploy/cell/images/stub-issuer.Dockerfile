# The local stub token issuer (DEC-2100, DEC-1025) for a dev or ci cell's sign-in only: lane SI's
# `mandate-stub-issuer`, a dev-profile build (it is compiled only with debug assertions), as a
# statically linked binary on an empty base, non-root, with no shell and no other file. It runs as
# a sidecar on the API pod's loopback; the chart names it only from a local build, which only the
# local cell's values allow. `cargo xtask cell up` builds it from a context holding only the binary.
FROM scratch
COPY mandate-stub-issuer /usr/local/bin/mandate-stub-issuer
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/mandate-stub-issuer"]
