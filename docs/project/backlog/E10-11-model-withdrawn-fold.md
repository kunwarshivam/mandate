# E10-11 follow-up: a withdrawn model leaves the registry V-007 reads

- **E10-11w (Proposed, M8; lane WB)** As an owner, I want a model the platform operator withdrew to be
  refused when I create or deploy a version, and gone from the models the set-up offers, so that no
  new agent pins it. Today `mandate-spec`'s validation context removes a model on
  `JournaledFact::ModelWithdrawn`, but `JournaledFact::from_record` maps no
  `PlatformOperatorAction` `model_withdrawn` to it, so the create's V-007 still admits a withdrawn
  model and `GET /signal-models` (DEC-2136) still lists it. *Accepted when:* `from_record` maps the
  action to `ModelWithdrawn` (with the reference vector), a create naming a withdrawn model is
  refused V-007, and `GET /signal-models` omits it, both through the one fold.
<!-- section: E10 Workspace services -->
