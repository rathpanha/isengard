//! Claude plan usage limits (5-hour session + weekly) for the Agent header.
//!
//! Not part of ACP: read from Anthropic's OAuth usage endpoint with the token
//! Claude Code already stored (macOS Keychain, else `~/.claude/.credentials.json`).
//! The token stays in this process and is only sent to `api.anthropic.com`.
//! Best effort — the endpoint is undocumented, so any failure just hides the bars.

#[cfg(target_os = "macos")]
use std::process::Command;

use chrono::{DateTime, Local};
use serde_json::Value;

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";

/// One rolling window.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    /// 0–100.
    pub percent: f32,
    /// ISO-8601 reset time as sent by the API.
    pub resets_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlanLimits {
    pub session: Option<Window>,
    pub week: Option<Window>,
}

impl PlanLimits {
    pub fn is_empty(&self) -> bool {
        self.session.is_none() && self.week.is_none()
    }
}

/// API `resets_at` (RFC3339 / ISO-8601) → local wall clock for tooltips.
///
/// Unparseable strings pass through unchanged.
pub fn format_resets_local(iso: &str) -> String {
    DateTime::parse_from_rfc3339(iso)
        .map(|dt| {
            dt.with_timezone(&Local)
                .format("%a %b %d, %Y %-I:%M %p")
                .to_string()
        })
        .unwrap_or_else(|_| iso.to_string())
}

/// Fold a fresh fetch into the last good snapshot.
///
/// Anthropic's OAuth usage endpoint sometimes returns HTTP 200 with
/// `five_hour` / `seven_day` null (outages, plan quirks). Treating that as
/// authoritative would blank the bars after they had already shown — keep
/// prior windows and only fill in what the payload actually has.
pub fn merge(prev: Option<PlanLimits>, next: PlanLimits) -> Option<PlanLimits> {
    if next.is_empty() {
        return prev;
    }
    Some(match prev {
        Some(p) => PlanLimits {
            session: next.session.or(p.session),
            week: next.week.or(p.week),
        },
        None => next,
    })
}

/// Blocking: call from a background task.
pub fn fetch() -> Result<PlanLimits, String> {
    let token = access_token().ok_or("no Claude Code login found")?;
    let mut response = ureq::get(USAGE_URL)
        .header("Authorization", &format!("Bearer {token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
        .call()
        .map_err(|e| format!("usage request failed: {e}"))?;
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("usage response: {e}"))?;
    let body: Value = serde_json::from_str(&text).map_err(|e| format!("usage json: {e}"))?;
    Ok(parse(&body))
}

fn parse(body: &Value) -> PlanLimits {
    let window = |key: &str| {
        let w = body.get(key)?;
        Some(Window {
            percent: w.get("utilization")?.as_f64()? as f32,
            resets_at: w
                .get("resets_at")
                .and_then(Value::as_str)
                .map(str::to_owned),
        })
    };
    PlanLimits {
        session: window("five_hour"),
        week: window("seven_day"),
    }
}

fn access_token() -> Option<String> {
    let raw = keychain_credentials().or_else(|| {
        let path = dirs::home_dir()?.join(".claude").join(".credentials.json");
        std::fs::read_to_string(path).ok()
    })?;
    let json: Value = serde_json::from_str(raw.trim()).ok()?;
    json.get("claudeAiOauth")?
        .get("accessToken")?
        .as_str()
        .map(str::to_owned)
}

#[cfg(target_os = "macos")]
fn keychain_credentials() -> Option<String> {
    let out = Command::new("security")
        .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(not(target_os = "macos"))]
fn keychain_credentials() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::{PlanLimits, Window, format_resets_local, merge, parse};
    use serde_json::json;

    #[test]
    fn parses_both_windows() {
        let l = parse(&json!({
            "five_hour": {"utilization": 42.5, "resets_at": "2026-10-21T15:00:00Z"},
            "seven_day": {"utilization": 7.0, "resets_at": null},
        }));
        assert_eq!(
            l.session,
            Some(Window {
                percent: 42.5,
                resets_at: Some("2026-10-21T15:00:00Z".into())
            })
        );
        assert_eq!(l.week.map(|w| w.percent), Some(7.0));
    }

    #[test]
    fn missing_windows_are_none() {
        let l = parse(&json!({"five_hour": null}));
        assert!(l.session.is_none() && l.week.is_none());
    }

    #[test]
    fn formats_rfc3339_as_local_wall_clock() {
        let s = format_resets_local("2026-10-22T08:10:00.071913+00:00");
        assert!(!s.contains("T08:"), "{s}");
        assert!(s.contains("2026"), "{s}");
        assert!(s.contains("Oct"), "{s}");
        assert!(s.ends_with("AM") || s.ends_with("PM"), "{s}");
    }

    #[test]
    fn bad_timestamp_passthrough() {
        assert_eq!(format_resets_local("later"), "later");
    }

    #[test]
    fn empty_fetch_keeps_previous() {
        let prev = Some(PlanLimits {
            session: Some(Window {
                percent: 18.0,
                resets_at: None,
            }),
            week: Some(Window {
                percent: 13.0,
                resets_at: None,
            }),
        });
        let empty = PlanLimits {
            session: None,
            week: None,
        };
        assert_eq!(merge(prev.clone(), empty), prev);
    }

    #[test]
    fn partial_fetch_keeps_missing_window() {
        let prev = Some(PlanLimits {
            session: Some(Window {
                percent: 18.0,
                resets_at: None,
            }),
            week: Some(Window {
                percent: 13.0,
                resets_at: None,
            }),
        });
        let only_week = PlanLimits {
            session: None,
            week: Some(Window {
                percent: 20.0,
                resets_at: Some("later".into()),
            }),
        };
        let merged = merge(prev, only_week).unwrap();
        assert_eq!(merged.session.map(|w| w.percent), Some(18.0));
        assert_eq!(merged.week.map(|w| w.percent), Some(20.0));
    }
}
