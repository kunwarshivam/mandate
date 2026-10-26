# E10-11 follow-up: a model registration the list cannot carry is refused, or the list carries it

- **E10-11v (Proposed, M8; lane WB)** As an owner, I want the models V-007 admits and the models
  `GET /signal-models` lists to be the same set, so that nothing registered is admitted that the
  set-up cannot show. Today V-007 reads only the registration row and admits a `model_version`
  registration that no mandate document can name (a version off the mandate schema's pattern, more
  than 32 parameters, or a parameter name off its key pattern), and a model's content may hold text
  longer than `read-models/signal-models` carries; the list omits both (DEC-2142). *Accepted when:*
  either the registration path (or V-007) refuses such a registration, with a reference vector, or
  the read model's bounds are widened to V-007's in a spec change of their own; and a property test
  shows the listed models and the admitted ones equal for every registration.
<!-- section: E10 Mandate authoring -->
