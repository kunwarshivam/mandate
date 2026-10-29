# The interim `news-reader` image for the local and CI cell (E2-10c, DEC-2085): the workspace news
# reader, `mandate-news-reader`, as a statically linked binary on an empty base, non-root, with no
# shell. Beside it, read-only, is the cell's recorded news, whose `recorded-news.json` its
# `--recorded` names: recorded vendor items and no key, so it reaches no vault and no host, and the
# binary refuses a recorded start outside dev and ci. A hosted cell runs the released image, which
# carries no recording (DEC-2084). `cargo xtask cell up` builds it from a context holding only the
# binary and the recording, and never with the `live` feature.
FROM scratch
COPY mandate-news-reader /mandate-news-reader
COPY news/ /usr/local/share/mandate/news/
USER 65532:65532
ENTRYPOINT ["/mandate-news-reader"]
