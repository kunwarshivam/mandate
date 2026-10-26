# The interim `model-gateway` image for the local and CI cell (E21-62, DEC-2326 item 3): the
# workspace's model-gateway service, `mandate-gatewayd`, as a statically linked binary on an empty
# base, non-root, with no shell. Beside it, read-only, is the demo's recorded research bundle, whose
# `research.json` its `--recorded` names: a recorded model reply, never a provider key. It reaches
# no provider and reads no key; a live start is refused `live_not_built` until the founder names
# the live model (DEC-2396). Lane R2 (E21-35) replaces this image with its reproducible, signed
# build. `cargo xtask cell up` builds it from a context holding only the binary and the bundle, and
# never with the `live` feature.
FROM scratch
COPY mandate-gatewayd /mandate-gatewayd
COPY research/ /usr/local/share/mandate/research/
USER 65532:65532
ENTRYPOINT ["/mandate-gatewayd"]
