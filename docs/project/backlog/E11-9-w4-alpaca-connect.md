- **E11-9 W-4 (Must, M9, before the founder's demo; SC)** As the owner, I want to connect Alpaca
  paper trading from the web app, so that a workspace gets its broker account without the CLI
  ([DEC-2376](../decisions/DEC-2376.md)). *Accepted when:* Connections lists every served connection
  with its state; the connect shows DEC-2622's disclosure, asks a `connection` challenge bound to it,
  starts with that challenge's evidence under one key, and sends the browser only to an `https`
  authorization page; `/connections/{id}` reads the connection until it settles; and only an
  established connection is Stop-all's scope.
<!-- section: E11 Web app: dashboard and controls -->
