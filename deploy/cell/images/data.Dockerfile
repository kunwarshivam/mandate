# The interim `data` image for the local and CI cell (E21-29 lane SC): the workspace data process,
# `mandate-datad`, as a statically linked binary on an empty base, non-root, with no shell and no
# other file. It holds nothing secret, reaches no host, and stores the cell's recorded daily series,
# never market data. Lane R2 (E21-35) replaces it with its reproducible, signed build. `cargo xtask
# cell up` builds it from a context holding only the binary, and never with the `live` feature.
FROM scratch
COPY mandate-datad /mandate-datad
USER 65532:65532
ENTRYPOINT ["/mandate-datad"]
