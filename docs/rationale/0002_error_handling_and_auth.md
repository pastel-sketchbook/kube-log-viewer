# ADR-0002: Error Display and Azure Auth Recovery

**Date:** 2026-02-24
**Status:** Accepted
**Supersedes:** Error overlay from initial implementation

## Context

The initial implementation displayed K8s errors as a centered popup overlay in the log pane. This had two problems:

1. **Verbose errors overflowed the overlay.** Azure/kubelogin auth errors can be hundreds of characters across multiple lines (AADSTS error codes, token details, suggested commands). The fixed-size overlay couldn't contain them, spilling across the TUI.

2. **kubelogin stderr corrupts the TUI.** When exec-based credential plugins (e.g. `kubelogin get-token`) fail, their stderr goes directly to the terminal, bypassing ratatui. This garbles the display regardless of how we render our own error.

Additionally, the most common error on AKS clusters is expired Azure credentials, which requires running `az login` to re-authenticate via the browser.

## Decision: Inline Error Lines

**Remove the error overlay entirely.** Instead, append errors as `[ERROR] <message>` lines to `log_lines` -- the same `Vec<String>` used for pod log output.

**Rationale:**
- The log pane already handles arbitrary-length text with scrolling and wrapping.
- Users can search errors with the existing `/` search feature.
- No special rendering code to maintain -- errors are just log lines with a `[ERROR]` prefix.
- The `colorize_log_line()` function already highlights lines containing "ERROR" in red.

**What was removed:**
- `error_message: Option<String>` field from `App`
- `render_error_overlay()` and `centered_rect_absolute()` from `ui/logs.rs`
- All `self.error_message = None` / `self.error_message = Some(...)` state management

## Decision: Automatic `az login` on Auth Errors

When an `AppEvent::Error` is received and the message matches known Azure/kubelogin auth patterns, the app automatically:

1. Sets `az_login_in_progress = true` (prevents re-triggering)
2. Appends `[INFO] Azure credentials expired — opening browser for login…` to log lines
3. Spawns `az login` as a background process with `stdout(null)` and `stderr(piped)`
4. On completion, sends `AppEvent::AzLoginCompleted(Result<(), String>)`
5. On success: appends info line, calls `load_contexts()` to reload cluster data
6. On failure: appends `[ERROR] az login failed: <details>`

**Auth error detection** (`is_auth_error`) checks for (case-insensitive):
- `aadsts` — Azure AD / Entra ID error codes
- `az login` — Suggested remediation in error messages
- `kubelogin` — Exec credential plugin name
- `interactive_browser` — kubelogin auth mode reference
- `token` + `expir` — Generic token expiration
- `unauthorized` — HTTP 401 status text
- `401` + `auth`/`credential` — Numeric status with auth context

**Why `stdout(null)` and `stderr(piped)`:**
- `az login` prints JSON to stdout on success -- we don't need it.
- Piping stderr prevents it from leaking into the TUI (same problem as kubelogin).
- We read stderr on failure to extract the last line as an error message.

**Why not prompt the user first:**
- The user explicitly requested automatic browser opening.
- `az login` is non-destructive -- it opens a browser tab for interactive auth.
- The `az_login_in_progress` flag prevents duplicate spawns.

## Consequences

- Errors are no longer modal -- they don't block interaction.
- Multiple errors accumulate in the scrollback rather than overwriting each other.
- Azure auth recovery is automatic, reducing the need to quit and re-launch.
- The `az` CLI must be installed for auto-login to work (expected on AKS workstations).
