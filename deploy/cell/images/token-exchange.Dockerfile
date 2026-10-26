# The token-exchange process (DEC-821 item 2, DEC-1175) for a dev or ci cell: `mandate-token-exchange`,
# a dev-profile build like the cell's other binaries, so its cell token URL (DEC-2633) compiles in,
# as a statically linked binary on an empty base, non-root, with no shell and no file but the host's
# CA bundle: without one its HTTP client finds no roots and cannot be built, so every exchange ended
# `endpoint_failed` before it sent anything (DEC-2063 item 8). The deployment manager starts it once
# per OAuth connect from the chart's pod template, which names `/mandate-token-exchange` as its
# command. `cargo xtask cell up` builds it from a context holding the binary and the bundle.
FROM scratch
COPY ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY mandate-token-exchange /mandate-token-exchange
USER 65532:65532
ENTRYPOINT ["/mandate-token-exchange"]
