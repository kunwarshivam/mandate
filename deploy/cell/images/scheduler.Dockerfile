# The interim `scheduler` image for the local and CI cell (E21-29): the workspace scheduler,
# `mandate-scheduler`, as a statically linked binary on an empty base, non-root, with no shell and
# no other file. It holds nothing secret: its one credential is its journal role's pgpass, mounted
# as a file, and it asks no time server until values.egress.time names one. Lane R2 (E21-35)
# replaces it with its reproducible, signed build. `cargo xtask cell up` builds it from a context
# holding only the binary, and never with the `live` feature.
FROM scratch
COPY mandate-scheduler /mandate-scheduler
USER 65532:65532
ENTRYPOINT ["/mandate-scheduler"]
