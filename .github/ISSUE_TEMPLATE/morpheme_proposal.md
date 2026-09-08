---
name: Morpheme Proposal
about: Propose a new morpheme (content predicate) for the Factum vocabulary
title: "[morpheme] "
labels: morpheme-proposal
---

## Morpheme Details

| Field | Value |
|-------|-------|
| Name | `your-morpheme-name` |
| Kind | Entity / Relation / Quantifier / Modal / Temporal |
| Signature | `arg1:Type, arg2:Type -> ReturnType` |

## Documentation

One-sentence description of what this morpheme asserts.

## Example Usages

At least 3 examples showing the morpheme in context:

```scheme
; Example 1
(node n001 :pred (your-morpheme-name @ENTITY-A @ENTITY-B 0.5))

; Example 2
(node n002 :pred (your-morpheme-name @ENTITY-C @ENTITY-D 0.3 :since #date(2024-01-01)))

; Example 3
(node n003 :pred (your-morpheme-name @ENTITY-E @ENTITY-F ?x))
```

## Provenance

What provenance types are expected for this morpheme?

- [ ] Verbatim (direct quote)
- [ ] Summary (human summary)
- [ ] Extracted (LLM extraction — must carry model ref)
- [ ] Derived (formal proof)
- [ ] Asserted (direct assertion)

## Related Morphemes

Does this overlap with or complement any existing morphemes?

## Source References

If this morpheme maps to an existing ontology (Wikidata property, schema.org, etc.), link it here.
