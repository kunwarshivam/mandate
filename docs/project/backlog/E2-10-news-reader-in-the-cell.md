# E2-10 follow-up: the news reader runs in the cell against a recorded news fixture

- **E2-10c (Proposed, M5; lane SC)** As the founder, I want the local and CI cell to run the news
  reader against a recorded news fixture, so that the loop where the agent acts on news (an item
  stored, the research agent reading it, an opinion recorded) is proven in CI before staging
  runs it with the paper data key. Today the chart renders the reader in staging alone
  (DEC-2084 item 2): with no key it is never ready, and `helm --wait` would wait on it. *Accepted
  when:* the cell's values run `news-reader` over a recorded news transport and a fixture key,
  neither of which can reach a vendor host. The fixture holds a few items for the cell's followed
  agent's instrument. The cell builds and imports a `news-reader` image from a Dockerfile that
  copies exactly the binary onto an empty base as non-root. A cell drill asserts each item is
  journaled once on `news:{workspace}`, and that the reader's writes to any other stream are
  refused by migration 0002. The research agent's next run reads the stored items.
<!-- section: E2 Market data -->
