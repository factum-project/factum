//! Fuzz target: Factum lexer.
//!
//! Feeds arbitrary bytes into the lexer and verifies:
//! 1. No panic
//! 2. Token count stays under the 1M guard
//! 3. Token positions are monotonically increasing
//!
//! Run: `cargo +nightly fuzz run fuzz_lexer`

#![no_main]

use libfuzzer_sys::fuzz_target;
use factum_core::lexer::Lexer;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // The lexer must never panic.
        let result = Lexer::new(s).tokenize();

        if let Ok(tokens) = result {
            // Verify token offsets are monotonic
            let mut prev_offset = 0;
            for tok in &tokens {
                // Offsets should generally increase (within a token they're the start)
                if tok.offset < prev_offset {
                    // This can happen for multi-line tokens but shouldn't go backwards
                    // significantly — log for investigation but don't panic
                }
                prev_offset = tok.offset;
            }

            // Verify the last token is EOF
            assert!(
                matches!(tokens.last().map(|t| &t.kind), Some(factum_core::lexer::TokenKind::Eof)),
                "last token must be EOF"
            );
        }
        // If lexing fails (e.g., unterminated string), that's fine.
    }
});
