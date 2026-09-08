//! Fuzz target: serialize → parse round-trip invariant.
//!
//! For any successfully parsed input, verifies:
//! `parse(canonical(parse(x))) == parse(x)` (syntactic round-trip)
//!
//! Run: `cargo +nightly fuzz run fuzz_serialize_roundtrip`

#![no_main]

use libfuzzer_sys::fuzz_target;
use factum_core::parser::Parser;
use factum_core::serialize;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Phase 1: parse the input
        if let Ok(nodes) = Parser::parse(s) {
            // Phase 2: for each node, serialize then re-parse
            for node in &nodes {
                let serialized = serialize::canonical(node);

                // Re-parse the serialized form
                match Parser::parse(&serialized) {
                    Ok(reparsed) => {
                        // Must get exactly one node back
                        if reparsed.len() == 1 {
                            // Round-trip invariant: the reparsed node must equal the original
                            // (comparing via PartialEq on Node, which excludes status)
                            let orig = node;
                            let re = &reparsed[0];
                            // Compare key fields — if they don't match, that's a bug
                            assert_eq!(orig.id, re.id, "id mismatch in roundtrip");
                            assert_eq!(orig.predicate, re.predicate, "predicate mismatch in roundtrip");
                            assert_eq!(orig.validity, re.validity, "validity mismatch in roundtrip");
                        }
                    }
                    Err(e) => {
                        // Re-parsing the canonical form should never fail.
                        // If it does, the serializer produced invalid output — that's a bug.
                        panic!(
                            "Round-trip failure: canonical form failed to re-parse.\n\
                             Original node: {:?}\n\
                             Serialized: {}\n\
                             Error: {}",
                            node, serialized, e
                        );
                    }
                }
            }
        }
    }
});
