# The interim `mandate` image for the local and CI cell (E21-37): the CLI this change built, and the
# backtest runner beside it (DEC-1144), as statically linked binaries on an empty base, non-root,
# with no shell and no other file. Lane R2 (E21-35) replaces it with its reproducible, signed build.
# `cargo xtask cell up` builds it from a context holding only the two binaries, and never with the
# `live` feature.
FROM scratch
COPY mandate /usr/local/bin/mandate
COPY mandate-backtest-runner /usr/local/bin/mandate-backtest-runner
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/mandate"]
