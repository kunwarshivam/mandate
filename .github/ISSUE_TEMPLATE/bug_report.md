---
name: Bug report
about: A defect with a reproduction (every fix starts with a failing test)
labels: ["bug"]
title: "<crate or area>: <what is wrong, one line>"
---

<!-- The tdd rule applies: a defect is fixed by first making it fail. Give the reproduction before any diagnosis. -->

**Reproduction:** the exact command or failing test

```text
<paste the command and its output, or the failing test's name and its failure>
```

**Expected:** <what the spec or the decision log says should happen — cite the clause or DEC-NN>

**Actual:** <what happened instead>

**Revision:** `git rev-parse HEAD` = <sha> (`main` or the PR head)

**Safety-critical?** <accounting / risk gate / executor / connectors / credentials / auth / notifications — if yes, the fix ships as the DEC-77 sequence (AGENTS.md)>

<!-- If the defect is a safety rule being violated rather than a plain bug, say so in the title — those changes need a DEC and the founder may need to rule. -->
