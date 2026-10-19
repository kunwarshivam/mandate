# The interim `runtime` image for the local and CI cell (DEC-1013): the agent runtime process this
# change built, `mandate-daemon`, as a statically linked binary on an empty base, non-root, with no
# shell. Beside it, read-only, is the demo's recorded research bundle, which `--research` names when
# the demo's researching agent is deployed (DEC-2210, DEC-2246): recorded data and a recorded model
# reply, never a provider key. It runs on the deny-all wiring otherwise. Lane R2 (E21-35) replaces
# this image with its reproducible, signed build, which carries no research bundle (`cargo xtask
# release images` refuses one). `cargo xtask cell up` builds it from a context holding only the
# binary and the bundle, and never with the `live` feature.
FROM scratch
COPY mandate-daemon /mandate-daemon
COPY research/ /usr/local/share/mandate/research/
USER 65532:65532
ENTRYPOINT ["/mandate-daemon"]
