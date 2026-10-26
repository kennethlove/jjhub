// story: e20s01
//! Rate-limit configuration: master switch off by default, env
//! parsing for the policy row, window/burst defaults. Pure `load`
//! takes a lookup fn so tests never race on process env.

use server::rate_limit::{Budget, RateLimitConfig};

mod rate_limit_config {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key: &str| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    /// story: e20s01 — JJHUB_RATE_LIMIT unset = limiter off; test and
    /// dev constructors structurally never see it.
    #[test]
    fn master_switch_off_by_default() {
        let cfg = RateLimitConfig::load(env(&[]));
        assert!(!cfg.enabled, "unset JJHUB_RATE_LIMIT must mean off");
    }

    /// story: e20s01 — login per-IP row defaults per the policy table:
    /// 20/hour, burst 5; window default 3600s.
    #[test]
    fn login_ip_defaults_per_policy_table() {
        let cfg = RateLimitConfig::load(env(&[("JJHUB_RATE_LIMIT", "on")]));
        assert!(cfg.enabled);
        assert_eq!(cfg.window_secs, 3600);
        assert_eq!(
            cfg.login_ip,
            Budget {
                per_window: 20,
                window_secs: 3600,
                burst: 5
            }
        );
    }

    /// story: e20s01 — explicit env overrides both budget and window;
    /// format N/window[:burst].
    #[test]
    fn explicit_budget_and_window_parse() {
        let cfg = RateLimitConfig::load(env(&[
            ("JJHUB_RATE_LIMIT", "1"),
            ("JJHUB_RATE_LIMIT_WINDOW", "1800"),
            ("JJHUB_RATE_LOGIN_IP", "10/900:3"),
        ]));
        assert!(cfg.enabled);
        assert_eq!(cfg.window_secs, 1800);
        assert_eq!(
            cfg.login_ip,
            Budget {
                per_window: 10,
                window_secs: 900,
                burst: 3
            }
        );
    }

    /// story: e20s01 — a malformed budget is an operator error: fail
    /// loud at load, never silently unlimited.
    #[test]
    #[should_panic(expected = "JJHUB_RATE_LOGIN_IP")]
    fn malformed_budget_fails_loud() {
        let _ = RateLimitConfig::load(env(&[
            ("JJHUB_RATE_LIMIT", "on"),
            ("JJHUB_RATE_LOGIN_IP", "many/hour"),
        ]));
    }
}
