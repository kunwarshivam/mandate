# The interim `api` image for the local and CI cell: the workspace API server this change built, as
# a statically linked binary on an empty base, non-root, with no shell and no other file. Lane R2
# (E21-35) replaces it with its reproducible, signed build. `cargo xtask cell up` builds it from a
# context holding only the binary.
FROM scratch
COPY mandate-api-server /usr/local/bin/mandate-api-server
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/mandate-api-server"]
