- **E21-license-features-byte-order (Proposed; debt; lane D1)** As an agent, I want
  `LicenseApplied`'s `features` (`crates/mandate-cp-site/src/lib.rs`) collected as the strings it
  journals, strictly ascending by bytes, rather than in `Feature`'s declaration order mapped through
  `as_str()`, so that a variant declared out of alphabetical order cannot write a list the journal
  refuses (found in the audit for the rule 9 `outputs_used` fix). *Accepted when:* a test with
  features whose declaration order differs from their byte order journals them ascending.
