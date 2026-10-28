- **E2-22 (Proposed, M7; EX)** As the operator, I want the data process's vault session ended as
  soon as it has read the platform data key, so that no token able to read that key outlives its
  one read. `mandate-datad`'s IEX source (DEC-2283 item 2) logs in as the bars reader, reads the key
  once, and drops the session, whose token then lives until its TTL because
  `OpenBaoSource::revoke_token` is private to `mandate-vault`. *Accepted when:* the session is
  revoked right after the read (a public revoke, or a short token TTL on the `bars-reader` role),
  and a test against the OpenBao harness shows the token refused after the read.
<!-- section: E2 Market data -->
