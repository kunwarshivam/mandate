# The interim `executor` image for the local and CI cell (DEC-2031): the account executor process,
# `mandate-executord`, as a statically linked binary on an empty base, non-root, with no shell and
# no other file. Its broker is the in-process fixture paper broker in every mode, so it reaches no
# broker host and reads no credential (DEC-2007). Lane R2 (E21-35) replaces it with its
# reproducible, signed build. `cargo xtask cell up` builds it from a context holding only the
# binary, and never with the `live` feature.
FROM scratch
COPY mandate-executord /mandate-executord
USER 65532:65532
ENTRYPOINT ["/mandate-executord"]
