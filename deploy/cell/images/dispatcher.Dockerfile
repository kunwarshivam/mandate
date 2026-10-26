# The interim `dispatcher` image for the local and CI cell (X8, DEC-2500, DEC-2516): the workspace
# notification dispatcher, `mandate-dispatcher`, as a statically linked binary on an empty base,
# non-root, with no shell and no other file. It holds nothing secret: its one credential is its
# journal role's pgpass, mounted as a file, and its configuration file, mounted from the ConfigMap
# `dispatcher-config`, holds no secret and no address. It mails only through the recorded
# transport, which sends nothing. Lane R2 (E21-35) replaces it with its reproducible, signed build.
# `cargo xtask cell up` builds it from a context holding only the binary, and never with the `live`
# feature.
FROM scratch
COPY mandate-dispatcher /mandate-dispatcher
USER 65532:65532
ENTRYPOINT ["/mandate-dispatcher"]
