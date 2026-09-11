#[cfg(test)]
mod test {
    use factum_core::parser::Parser;
    use factum_core::serialize;

    #[test]
    fn test_validity_in_node() {
        let input = r#"(node n004
  :pred (shareholder-major @ACME-CORP @FOUNDER-1 0.73 :since #date(2001-03-15))
  :valid (window "2001-03-15T00:00:00+00:00" "2025-12-31T00:00:00+00:00")
  :conf 0.85 :auth 0.8 :perm confidential
  :src (extracted "earnings-2024" [120 350] (model "gpt-4" "2024-06")))"#;
        let nodes = Parser::parse(input).unwrap();
        assert_eq!(nodes.len(), 1);
        let canonical = serialize::canonical(&nodes[0]);
        println!("Canonical:\n{}", canonical);
        // Verify round-trip
        serialize::verify_roundtrip(&nodes[0]).unwrap();
    }
}
