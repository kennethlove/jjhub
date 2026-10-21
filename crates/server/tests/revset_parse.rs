// story: e11s01
//! t1 — parse revset queries with the jj_lib revset parser.

mod revset_parse {
    #[test]
    fn revset_parse_accepts_builtins_and_open_dialect() {
        for query in [
            "all()",
            "open()",
            "trunk()",
            "description(\"hello jjhub\")",
            "open() | all()",
            "all() ~ open()",
        ] {
            server::jj_meta::parse_revset(query).unwrap_or_else(|e| panic!("parse {query:?}: {e}"));
        }
    }

    #[test]
    fn revset_parse_rejects_garbage() {
        let err = server::jj_meta::parse_revset("((((").expect_err("garbage must not parse");
        assert!(!err.is_empty(), "error message must not be empty");
    }
}
