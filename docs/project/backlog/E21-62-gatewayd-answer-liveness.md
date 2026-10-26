# E21-62 debt: the model gateway's liveness sees a wedged answer thread

- **E21-62e (Proposed, M8; debt; lane GW)** `mandate-gatewayd`'s heartbeat thread reports the
  process live while its serve loop runs, whether or not the one thread that answers calls is making
  progress. A call wedged in a journal request past its deadline keeps `livez` green. Drive the
  liveness bound from the answer path, with an idle tick while no call waits, so a wedged answer
  fails `livez` within its bound and the pod restarts. A wedged call already reserves at its
  maximum (DEC-2265 item 3), so the cap stays true meanwhile. Found by the independent review of
  #2537.
