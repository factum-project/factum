# Factum-F Conformance Test Vectors

## Format

Each JSON file contains an array of test vectors. See `manifest.json` for spec version and error taxonomy.

### Positive vectors (`parse_basic.json`)

Each vector has:
- `id`: unique identifier
- `description`: what this vector tests
- `input`: Factum-F source string
- `expected_canonical`: the canonical serialization that `parse(input)` must produce

**Conformance rule**: An implementation passes this vector if:
1. `parse(input)` succeeds
2. `serialize(result)` produces output structurally identical to `expected_canonical`

Two implementations that pass the same set of vectors are structurally identical — canonical form is the deepest possible assertion.

### Negative vectors (`parse_errors.json`)

Each vector has:
- `id`: unique identifier
- `description`: what error this vector tests
- `input`: Factum-F source that should fail to parse
- `error_class`: categorized error type (see manifest.json for taxonomy)

**Conformance rule**: An implementation passes this vector if:
1. `parse(input)` fails (does not produce a result)
2. The error message contains the `error_class` string (case-insensitive)

### Manifest (`manifest.json`)

Declares:
- `spec_version`: the spec version these vectors target
- `error_taxonomy`: the complete list of error classes implementations must distinguish

When the spec advances to a new version, new vector files may be added with a different `spec_version`. Implementations can declare which spec versions they conform to.
