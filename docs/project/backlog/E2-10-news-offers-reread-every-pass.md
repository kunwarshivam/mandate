# E2-10 follow-up: the research pass stops re-reading every stored news record on every pass

- **E2-10e (Proposed, M5, debt; lane SC)** As the founder, I want the runtime's research pass to
  read each stored news record once, so that a workspace whose news stream has grown to thousands
  of items does not read thousands of artifacts on every pass. Today `CellSource` (DEC-2288) reads
  the whole `news:{workspace}` stream, and the record of every `NewsItemStored` on it, on every
  pass under news triggers. The core then refuses all but the newest items, as older than the last
  research call or already started. That is correct but linear in the stream's length. *Accepted
  when:* the source keeps each row's offer or refusal by its event id and hash, since the journal
  never changes a row, and reads only the rows after the last one it read, as the process's tails
  do (DEC-2852). A test counts the artifact reads over many passes against a growing stream and
  shows each record read once. A restarted process reads the stream once again.
<!-- section: E2 Market data -->
