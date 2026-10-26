- **E11-9y (Proposed, M9; EX)** As the owner, I want the alerts read to say what it does with a
  control-stream row naming a stream that does not read, so that "never a shorter list" (workspace
  API spec §4.7, DEC-2141 item 6) holds without exception. Today `GET /alerts`, like the
  dispatcher (DEC-2516 item 1), treats an `AgentDeployed` or connection row whose payload does not
  read as naming nothing, so a stream only that row names is not read. *Accepted when:* the spec
  states the reading (a row that does not read names nothing, or fails the read) and the route and
  the dispatcher follow it, each with a test.
