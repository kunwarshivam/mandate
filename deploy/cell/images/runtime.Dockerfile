# The interim `runtime` image for the local and CI cell (DEC-1013): the agent runtime process this
# change built, `mandate-daemon`, as a statically linked binary on an empty base, non-root, with no
# shell and no other file. It runs on the deny-all wiring until the post-R0 hook wires production
# ports. Lane R2 (E21-35) replaces it with its reproducible, signed build. `cargo xtask cell up`
# builds it from a context holding only the binary, and never with the `live` feature.
FROM scratch
COPY mandate-daemon /usr/local/bin/mandate-daemon
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/mandate-daemon"]
