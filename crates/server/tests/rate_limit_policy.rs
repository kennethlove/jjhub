// story: e20s02 — the policy table: push/create/token rows parse with
// the documented defaults and env overrides. Pure `load` takes a
// lookup fn so tests never race on process env.

use server::rate_limit::{Budget, RateLimitConfig};

mod rate_limit_policy {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key: &str| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    /// story: e20s02 — the three write paths' rows per the planning
    /// policy table.
    #[test]
    fn rows_match_policy_table_defaults() {
        let cfg = RateLimitConfig::load(&env(&[("JJHUB_RATE_LIMIT", "on")]));
        assert_eq!(
            cfg.push_user,
            Budget {
                per_window: 30,
                window_secs: 3600,
                burst: 10
            }
        );
        assert_eq!(cfg.push_ip, cfg.push_user);
        assert_eq!(
            cfg.create_user,
            Budget {
                per_window: 10,
                window_secs: 3600,
                burst: 3
            }
        );
        assert_eq!(cfg.create_ip, cfg.create_user);
        assert_eq!(
            cfg.token_user,
            Budget {
                per_window: 10,
                window_secs: 3600,
                burst: 5
            }
        );
    }

    /// story: e20s02 — every row has its own env knob.
    #[test]
    fn every_row_has_an_env_override() {
        let cfg = RateLimitConfig::load(&env(&[
            ("JJHUB_RATE_LIMIT", "on"),
            ("JJHUB_RATE_PUSH_USER", "1/60:1"),
            ("JJHUB_RATE_PUSH_IP", "2/60:1"),
            ("JJHUB_RATE_CREATE_USER", "3/60:1"),
            ("JJHUB_RATE_CREATE_IP", "4/60:1"),
            ("JJHUB_RATE_TOKEN_USER", "5/60:1"),
        ]));
        assert_eq!(cfg.push_user.per_window, 1);
        assert_eq!(cfg.push_ip.per_window, 2);
        assert_eq!(cfg.create_user.per_window, 3);
        assert_eq!(cfg.create_ip.per_window, 4);
        assert_eq!(cfg.token_user.per_window, 5);
    }

    /// story: e20s02 — a malformed row is an operator error: fail loud,
    /// never silently unlimited.
    #[test]
    #[should_panic(expected = "JJHUB_RATE_PUSH_USER")]
    fn malformed_row_fails_loud() {
        let _ = RateLimitConfig::load(&env(&[
            ("JJHUB_RATE_LIMIT", "on"),
            ("JJHUB_RATE_PUSH_USER", "lots"),
        ]));
    }
}
