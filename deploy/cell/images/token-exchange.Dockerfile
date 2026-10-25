# The token-exchange process (DEC-821 item 2, DEC-1175) for a dev or ci cell: `mandate-token-exchange`,
# a dev-profile build like the cell's other binaries, so its cell token URL (DEC-2633) compiles in,
# as a statically linked binary on an empty base, non-root, with no shell and no other file. The
# deployment manager starts it once per OAuth connect from the chart's pod template, which names
# `/mandate-token-exchange` as its command. `cargo xtask cell up` builds it from a context holding
# only the binary.
FROM scratch
COPY mandate-token-exchange /mandate-token-exchange
USER 65532:65532
ENTRYPOINT ["/mandate-token-exchange"]
