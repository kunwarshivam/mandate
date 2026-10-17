# The interim `api` image for the local and CI cell: the workspace API server this change built, as
# a statically linked binary on an empty base, non-root, with no shell. Its one other file is the
# building host's CA bundle, without which the server finds no TLS roots, cannot build its key-set
# client, and refuses to start. Lane R2 (E21-35) replaces it with its reproducible, signed build,
# which must carry roots too. `cargo xtask cell up` builds it from a context holding only those two
# files.
FROM scratch
COPY ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY mandate-api-server /usr/local/bin/mandate-api-server
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/mandate-api-server"]
