//! Claude plan usage limits (5-hour session + weekly) for the Agent header.
//!
//! Not part of ACP: read from Anthropic's OAuth usage endpoint with the token
//! Claude Code already stored (macOS Keychain, else `~/.claude/.credentials.json`).
//! Access tokens expire (~8h); we refresh via `platform.claude.com` and write the
//! rotated pair back so Claude Code keeps working. Best effort — any failure
//! just hides the bars.

#[cfg(target_os = "macos")]
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local};
use serde_json::{json, Value};

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
/// Claude Code's public OAuth client (same id the CLI uses).
const OAUTH_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const REFRESH_URL: &str = "https://platform.claude.com/v1/oauth/token";
/// Refresh this many ms before `expiresAt` so a mid-poll 401 is rare.
const REFRESH_SKEW_MS: u128 = 60_000;

/// One rolling window.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    /// 0–100.
    pub percent: f32,
    /// ISO-8601 reset time as sent by the API.
    pub resets_at: Option<String>,
}

/// Share of weekly plan usage by product surface (not model).
#[derive(Clone, Debug, PartialEq)]
pub struct BreakdownRow {
    pub key: String,
    pub display_name: String,
    /// 0–100 share of the weekly window.
    pub percent: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlanLimits {
    pub session: Option<Window>,
    pub week: Option<Window>,
    /// Model-scoped weekly cap when the plan exposes it (often Max); `None` on Pro.
    pub week_opus: Option<Window>,
    pub week_sonnet: Option<Window>,
    pub breakdown: Vec<BreakdownRow>,
    /// Display label from Claude OAuth creds (`pro` → `Claude Pro`), if known.
    pub subscription: Option<String>,
}

impl PlanLimits {
    pub fn is_empty(&self) -> bool {
        self.session.is_none()
            && self.week.is_none()
            && self.week_opus.is_none()
            && self.week_sonnet.is_none()
            && self.breakdown.is_empty()
    }
}

/// `claudeAiOauth.subscriptionType` → short UI label.
pub fn format_subscription(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "" => "Claude".into(),
        "pro" => "Claude Pro".into(),
        "max" => "Claude Max".into(),
        "free" => "Claude Free".into(),
        "team" => "Claude Team".into(),
        "enterprise" => "Claude Enterprise".into(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(c) => format!("Claude {}{}", c.to_ascii_uppercase(), chars.as_str()),
                None => "Claude".into(),
            }
        }
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
            week_opus: next.week_opus.or(p.week_opus),
            week_sonnet: next.week_sonnet.or(p.week_sonnet),
            breakdown: if next.breakdown.is_empty() {
                p.breakdown
            } else {
                next.breakdown
            },
            subscription: next.subscription.or(p.subscription),
        },
        None => next,
    })
}

/// Blocking: call from a background task.
pub fn fetch() -> Result<PlanLimits, String> {
    let (token, subscription) = access_token_fresh()?;
    fetch_with_token(&token, subscription)
}

fn fetch_with_token(token: &str, subscription: Option<String>) -> Result<PlanLimits, String> {
    fetch_with_token_inner(token, subscription, true)
}

fn fetch_with_token_inner(
    token: &str,
    subscription: Option<String>,
    allow_refresh: bool,
) -> Result<PlanLimits, String> {
    let mut response = ureq::get(USAGE_URL)
        .header("Authorization", &format!("Bearer {token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
        .call()
        .map_err(|e| format!("usage request failed: {e}"))?;
    let status = response.status().as_u16();
    // Stale clock / Claude Code rotated under us — refresh once and retry.
    if status == 401 && allow_refresh {
        let (token, refreshed_sub) =
            refresh_oauth().map_err(|e| format!("usage 401; refresh: {e}"))?;
        return fetch_with_token_inner(&token, refreshed_sub.or(subscription), false);
    }
    if !(200..300).contains(&status) {
        return Err(format!("usage request failed: status {status}"));
    }
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("usage response: {e}"))?;
    let body: Value = serde_json::from_str(&text).map_err(|e| format!("usage json: {e}"))?;
    let mut limits = parse(&body);
    limits.subscription = subscription;
    Ok(limits)
}

fn access_token_fresh() -> Result<(String, Option<String>), String> {
    let oauth = oauth_creds().ok_or("no Claude Code login found")?;
    let subscription = oauth
        .get("subscriptionType")
        .and_then(Value::as_str)
        .map(format_subscription);
    let expired = oauth
        .get("expiresAt")
        .and_then(Value::as_u64)
        .map(|exp| now_ms() + REFRESH_SKEW_MS >= u128::from(exp))
        .unwrap_or(false);
    if expired {
        return refresh_oauth().or_else(|e| {
            // Fall through to the stored token if refresh fails — usage may
            // still work until Anthropic rejects it (401 path retries).
            log::debug!("agent plan limits: refresh skipped: {e}");
            let token = oauth
                .get("accessToken")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("Claude OAuth token missing ({e})"))?
                .to_owned();
            Ok((token, subscription))
        });
    }
    let token = oauth
        .get("accessToken")
        .and_then(Value::as_str)
        .ok_or("Claude OAuth token missing")?
        .to_owned();
    Ok((token, subscription))
}

/// Exchange `refreshToken` → new access (+ rotated refresh). Persists to the
/// same store Claude Code uses. Refresh tokens rotate; skipping write-back
/// invalidates the CLI login.
fn refresh_oauth() -> Result<(String, Option<String>), String> {
    let (mut root, source) = credentials_root().ok_or("no Claude Code login found")?;
    let oauth = root
        .get_mut("claudeAiOauth")
        .ok_or("credentials missing claudeAiOauth")?;
    let refresh = oauth
        .get("refreshToken")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or("Claude OAuth refresh token missing")?
        .to_owned();
    let subscription = oauth
        .get("subscriptionType")
        .and_then(Value::as_str)
        .map(format_subscription);

    let body = json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh,
        "client_id": OAUTH_CLIENT_ID,
    })
    .to_string();
    let mut response = ureq::post(REFRESH_URL)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .header("User-Agent", "claude-code/2.0")
        .send(body.as_bytes())
        .map_err(|e| format!("oauth refresh failed: {e}"))?;
    if !(200..300).contains(&response.status().as_u16()) {
        let status = response.status();
        let _ = response.body_mut().read_to_string();
        return Err(format!("oauth refresh status {status}"));
    }
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("oauth refresh body: {e}"))?;
    let tok: Value = serde_json::from_str(&text).map_err(|e| format!("oauth refresh json: {e}"))?;
    let access = tok
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or("oauth refresh missing access_token")?
        .to_owned();
    let new_refresh = tok
        .get("refresh_token")
        .and_then(Value::as_str)
        .unwrap_or(refresh.as_str())
        .to_owned();
    let now = now_ms();
    if let Some(secs) = tok.get("expires_in").and_then(Value::as_u64) {
        oauth
            .as_object_mut()
            .ok_or("claudeAiOauth not an object")?
            .insert(
                "expiresAt".into(),
                json!((now + u128::from(secs) * 1000) as u64),
            );
    }
    if let Some(secs) = tok.get("refresh_token_expires_in").and_then(Value::as_u64) {
        oauth.as_object_mut().unwrap().insert(
            "refreshTokenExpiresAt".into(),
            json!((now + u128::from(secs) * 1000) as u64),
        );
    }
    {
        let obj = oauth.as_object_mut().unwrap();
        obj.insert("accessToken".into(), json!(access));
        obj.insert("refreshToken".into(), json!(new_refresh));
    }
    persist_credentials(&root, source)?;
    Ok((access, subscription))
}

#[derive(Clone, Copy)]
enum CredSource {
    File,
    #[cfg(target_os = "macos")]
    Keychain,
}

fn credentials_root() -> Option<(Value, CredSource)> {
    #[cfg(target_os = "macos")]
    if let Some(raw) = keychain_credentials()
        && let Ok(json) = serde_json::from_str::<Value>(raw.trim())
        && json.get("claudeAiOauth").is_some()
    {
        return Some((json, CredSource::Keychain));
    }
    let path = dirs::home_dir()?.join(".claude").join(".credentials.json");
    let raw = std::fs::read_to_string(path).ok()?;
    let json: Value = serde_json::from_str(raw.trim()).ok()?;
    json.get("claudeAiOauth")?;
    Some((json, CredSource::File))
}

fn oauth_creds() -> Option<Value> {
    credentials_root().map(|(root, _)| root.get("claudeAiOauth").cloned())?
}

fn persist_credentials(root: &Value, source: CredSource) -> Result<(), String> {
    let text = serde_json::to_string(root).map_err(|e| format!("credentials serialize: {e}"))?;
    match source {
        CredSource::File => {
            let path = dirs::home_dir()
                .ok_or("no home dir")?
                .join(".claude")
                .join(".credentials.json");
            // Atomic-ish replace so a crash mid-write does not truncate login.
            let tmp = path.with_extension("json.tmp");
            std::fs::write(&tmp, &text).map_err(|e| format!("credentials write: {e}"))?;
            std::fs::rename(&tmp, &path).map_err(|e| format!("credentials rename: {e}"))?;
            Ok(())
        }
        #[cfg(target_os = "macos")]
        CredSource::Keychain => {
            // Delete + add keeps the same service name Claude Code reads.
            let _ = Command::new("security")
                .args([
                    "delete-generic-password",
                    "-s",
                    "Claude Code-credentials",
                ])
                .output();
            let status = Command::new("security")
                .args([
                    "add-generic-password",
                    "-a",
                    &std::env::var("USER").unwrap_or_else(|_| "claude".into()),
                    "-s",
                    "Claude Code-credentials",
                    "-w",
                    &text,
                    "-U",
                ])
                .status()
                .map_err(|e| format!("keychain write: {e}"))?;
            if status.success() {
                Ok(())
            } else {
                Err("keychain write failed".into())
            }
        }
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
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
    let breakdown = body
        .get("seven_day_breakdown")
        .and_then(|b| b.get("rows"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    Some(BreakdownRow {
                        key: row.get("key")?.as_str()?.to_owned(),
                        display_name: row.get("display_name")?.as_str()?.to_owned(),
                        percent: row.get("percent")?.as_f64()? as f32,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    PlanLimits {
        session: window("five_hour"),
        week: window("seven_day"),
        week_opus: window("seven_day_opus"),
        week_sonnet: window("seven_day_sonnet"),
        breakdown,
        subscription: None,
    }
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

#[cfg(test)]
mod tests {
    use super::{
        BreakdownRow, PlanLimits, Window, format_resets_local, format_subscription, merge, parse,
    };
    use serde_json::json;

    fn empty_limits() -> PlanLimits {
        PlanLimits {
            session: None,
            week: None,
            week_opus: None,
            week_sonnet: None,
            breakdown: vec![],
            subscription: None,
        }
    }

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
        assert!(l.week_opus.is_none() && l.week_sonnet.is_none());
        assert!(l.breakdown.is_empty());
        assert!(l.subscription.is_none());
    }

    #[test]
    fn missing_windows_are_none() {
        let l = parse(&json!({"five_hour": null}));
        assert!(l.is_empty());
    }

    #[test]
    fn parses_model_windows_and_breakdown() {
        let l = parse(&json!({
            "five_hour": {"utilization": 6.0, "resets_at": "2026-10-22T13:00:00Z"},
            "seven_day": {"utilization": 13.0, "resets_at": "2026-10-28T03:00:00Z"},
            "seven_day_opus": {"utilization": 40.0, "resets_at": "2026-10-28T03:00:00Z"},
            "seven_day_sonnet": null,
            "seven_day_breakdown": {
                "rows": [
                    {"key": "claude_code", "display_name": "Claude Code", "percent": 100},
                    {"key": "chat", "display_name": "Chats", "percent": 0}
                ]
            }
        }));
        assert_eq!(l.week_opus.map(|w| w.percent), Some(40.0));
        assert!(l.week_sonnet.is_none());
        assert_eq!(
            l.breakdown,
            vec![
                BreakdownRow {
                    key: "claude_code".into(),
                    display_name: "Claude Code".into(),
                    percent: 100.0,
                },
                BreakdownRow {
                    key: "chat".into(),
                    display_name: "Chats".into(),
                    percent: 0.0,
                },
            ]
        );
    }

    #[test]
    fn formats_subscription_labels() {
        assert_eq!(format_subscription("pro"), "Claude Pro");
        assert_eq!(format_subscription("MAX"), "Claude Max");
        assert_eq!(format_subscription("team"), "Claude Team");
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
            week_opus: None,
            week_sonnet: None,
            breakdown: vec![BreakdownRow {
                key: "claude_code".into(),
                display_name: "Claude Code".into(),
                percent: 100.0,
            }],
            subscription: Some("Claude Pro".into()),
        });
        assert_eq!(merge(prev.clone(), empty_limits()), prev);
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
            week_opus: Some(Window {
                percent: 50.0,
                resets_at: None,
            }),
            week_sonnet: None,
            breakdown: vec![],
            subscription: Some("Claude Pro".into()),
        });
        let only_week = PlanLimits {
            session: None,
            week: Some(Window {
                percent: 20.0,
                resets_at: Some("later".into()),
            }),
            week_opus: None,
            week_sonnet: None,
            breakdown: vec![BreakdownRow {
                key: "chat".into(),
                display_name: "Chats".into(),
                percent: 10.0,
            }],
            subscription: None,
        };
        let merged = merge(prev, only_week).unwrap();
        assert_eq!(merged.session.map(|w| w.percent), Some(18.0));
        assert_eq!(merged.week.map(|w| w.percent), Some(20.0));
        assert_eq!(merged.week_opus.map(|w| w.percent), Some(50.0));
        assert_eq!(merged.breakdown.len(), 1);
        assert_eq!(merged.breakdown[0].key, "chat");
        assert_eq!(merged.subscription.as_deref(), Some("Claude Pro"));
    }
}
