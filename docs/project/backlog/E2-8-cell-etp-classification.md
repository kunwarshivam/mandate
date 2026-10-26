# E2-8 follow-up: the cell's demo snapshot classified by the real source

- **E2-8c (Proposed; intelligence and data)** As an owner, I want the cell's demo to read its
  instrument's ETP classification from the classifier E2-8 builds, so that the snapshot it registers
  is classified by a source rather than asserted as of the run's start (DEC-2073).
  *Accepted when:* the cell's data process, or a step before registration, classifies the demo's
  instruments from the decided source, `cell demo` registers that classification and its instant,
  and DEC-2073's stamp is removed.
<!-- section: E2 Market data -->
