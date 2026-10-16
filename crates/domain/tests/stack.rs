// story: e03s02
mod stack_derivation {
    use domain::{derive_stacks, StackInput};
    use std::collections::HashMap;

    fn parents(pairs: &[(&str, &[&str])]) -> HashMap<String, Vec<String>> {
        pairs
            .iter()
            .map(|(commit, parents)| {
                (
                    commit.to_string(),
                    parents.iter().map(|p| p.to_string()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn single_mr_on_trunk_is_one_stack() {
        let inputs = vec![StackInput {
            change_id: "aaaa".into(),
            latest_commit: "c1".into(),
        }];
        let stacks = derive_stacks(&inputs, parents(&[("c1", &["trunk1"])]));
        assert_eq!(stacks, vec![vec!["aaaa".to_string()]]);
    }

    // story: e03s02 — invariant 3: stack order comes from commit parents.
    #[test]
    fn chain_orders_bottom_first() {
        let inputs = vec![
            StackInput {
                change_id: "ccc".into(),
                latest_commit: "c3".into(),
            },
            StackInput {
                change_id: "aaa".into(),
                latest_commit: "c1".into(),
            },
            StackInput {
                change_id: "bbb".into(),
                latest_commit: "c2".into(),
            },
        ];
        let stacks = derive_stacks(
            &inputs,
            parents(&[("c1", &["trunk1"]), ("c2", &["c1"]), ("c3", &["c2"])]),
        );
        assert_eq!(
            stacks,
            vec![vec![
                "aaa".to_string(),
                "bbb".to_string(),
                "ccc".to_string()
            ]]
        );
    }

    #[test]
    fn independent_stacks_stay_separate() {
        let inputs = vec![
            StackInput {
                change_id: "aaa".into(),
                latest_commit: "c1".into(),
            },
            StackInput {
                change_id: "zzz".into(),
                latest_commit: "z1".into(),
            },
        ];
        let stacks = derive_stacks(
            &inputs,
            parents(&[("c1", &["trunk1"]), ("z1", &["trunk2"])]),
        );
        assert_eq!(stacks.len(), 2);
        assert_eq!(stacks[0], vec!["aaa".to_string()]);
        assert_eq!(stacks[1], vec!["zzz".to_string()]);
    }

    #[test]
    fn middle_rewrite_still_links_through() {
        // b's latest commit c2b rewrites c2: parent chain c2b -> c1 (skipping
        // the stale c2). Derivation must follow actual parents, not history.
        let inputs = vec![
            StackInput {
                change_id: "aaa".into(),
                latest_commit: "c1".into(),
            },
            StackInput {
                change_id: "bbb".into(),
                latest_commit: "c2b".into(),
            },
        ];
        let stacks = derive_stacks(&inputs, parents(&[("c1", &["trunk1"]), ("c2b", &["c1"])]));
        assert_eq!(stacks, vec![vec!["aaa".to_string(), "bbb".to_string()]]);
    }

    #[test]
    fn orphan_mr_is_its_own_stack() {
        // parent neither trunk nor another MR: treat as bottom (data we
        // cannot explain gets rendered, never dropped)
        let inputs = vec![StackInput {
            change_id: "ooo".into(),
            latest_commit: "o1".into(),
        }];
        let stacks = derive_stacks(&inputs, parents(&[("o1", &["unknown-parent"])]));
        assert_eq!(stacks, vec![vec!["ooo".to_string()]]);
    }
}
