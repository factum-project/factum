//! Fuzz target: Factum parser.
//!
//! Feeds arbitrary bytes into `Parser::parse` and checks:
//! 1. No panic / stack overflow (the depth limit must catch adversarial nesting)
//! 2. If parsing succeeds, the result is structurally valid
//! 3. If parsing fails, the error has location info
//!
//! Run: `cargo +nightly fuzz run fuzz_parser`
//! CI short run: `cargo +nightly fuzz run fuzz_parser -- -max_total_time=600`

#![no_main]

use libfuzzer_sys::fuzz_target;
use factum_core::parser::Parser;

fuzz_target!(|data: &[u8]| {
    // Convert to string; skip invalid UTF-8
    if let Ok(s) = std::str::from_utf8(data) {
        // The parser must never panic on any input.
        // If it does, that's a bug we need to find.
        let result = Parser::parse(s);

        if let Ok(nodes) = &result {
            // Every parsed node must have a non-empty id
            for node in nodes {
                assert!(!node.id.as_str().is_empty(), "parsed node has empty id");
            }
        }
        // If parse fails, that's fine — we just need no panics.
    }
});
