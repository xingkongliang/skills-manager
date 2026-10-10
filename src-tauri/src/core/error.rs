use serde::Serialize;
use std::fmt;

/// Structured error type for Tauri commands.
///
/// Serialized as `{"kind": "Database", "message": "..."}` so the frontend
/// can branch on `kind` while still showing a human-readable `message`.
///
/// `details` carries machine-readable specifics for the few kinds where the
/// caller has to do more than print the message. It is omitted from the wire
/// format when absent, so every existing consumer is unaffected.
#[derive(Debug, Serialize)]
pub struct AppError {
    pub kind: ErrorKind,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<ErrorDetails>,
    /// What the caller has to do about it, when the message alone does not say.
    ///
    /// Set only where a caller has to *act*, not merely report: a GUI user should
    /// not have to recognise a CLI flag name to know which field to fill in, and
    /// a private repository needs a different hint than a repository that is not
    /// there. Both cases are matched on English text today, which breaks the
    /// moment a message is reworded. Omitted when absent, so nothing that does
    /// not set it changes shape.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<ErrorReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorReason {
    /// The repository root is not a skill directory, so the caller has to say
    /// which directory inside it is.
    SubpathRequired,
    /// Git refused the operation for want of credentials — a private repository,
    /// or one the current login cannot read.
    AuthFailed,
}

/// The paths a deployment refused to write, because they belong to someone
/// else (#363). Nothing at them was touched. Carried only by
/// `ErrorKind::TargetConflict`, which is what says how to read them — the ways
/// out are documented once, in the `manage-skills` skill, not repeated here on
/// every error.
#[derive(Debug, Serialize)]
pub struct ErrorDetails {
    pub conflicts: Vec<TargetConflictDetail>,
}

#[derive(Debug, Serialize)]
pub struct TargetConflictDetail {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    Database,
    Io,
    Network,
    Git,
    NotFound,
    InvalidInput,
    Cancelled,
    Internal,
    /// A write was refused because the target is not ours to replace. Always
    /// carries `ErrorDetails::TargetConflict`.
    TargetConflict,
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl AppError {
    pub fn not_found(msg: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::NotFound,
            message: msg.into(),
            details: None,
            reason: None,
        }
    }

    pub fn invalid_input(msg: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::InvalidInput,
            message: msg.into(),
            details: None,
            reason: None,
        }
    }

    /// The caller has to say which directory inside the repository is the skill.
    ///
    /// The message stays CLI-shaped for the CLI's own output; what the GUI does
    /// about it comes from [`ErrorReason::SubpathRequired`], so no caller has to
    /// recognise a flag name to know which field to put the answer in.
    pub fn subpath_required(msg: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::InvalidInput,
            message: msg.into(),
            details: None,
            reason: Some(ErrorReason::SubpathRequired),
        }
    }

    #[allow(dead_code)]
    pub fn cancelled(msg: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Cancelled,
            message: msg.into(),
            details: None,
            reason: None,
        }
    }

    /// Convert an `anyhow::Error` originating from database operations.
    pub fn db(e: impl fmt::Display) -> Self {
        Self {
            kind: ErrorKind::Database,
            message: e.to_string(),
            details: None,
            reason: None,
        }
    }

    /// Convert an `anyhow::Error` originating from git operations.
    pub fn git(e: impl fmt::Display) -> Self {
        Self {
            kind: ErrorKind::Git,
            message: e.to_string(),
            details: None,
            reason: None,
        }
    }

    /// Classify a git operation error into cancellation, network, authentication
    /// or generic git error.
    ///
    /// The buckets are matched on English text, which is as fragile as it sounds
    /// — but that is what git hands us, and [`ErrorReason`] carries the one
    /// distinction a caller must *act* on out to the frontend so it never has to
    /// repeat the matching.
    pub fn classify_git_error(e: impl fmt::Display) -> Self {
        let message = e.to_string();
        let lower = message.to_ascii_lowercase();
        if lower.contains("cancelled") || lower.contains("canceled") {
            Self {
                kind: ErrorKind::Cancelled,
                message,
                details: None,
                reason: None,
            }
        } else if lower.contains("authentication failed")
            || lower.contains("could not read username")
            || lower.contains("permission denied (publickey)")
            || lower.contains("terminal prompts disabled")
        {
            Self {
                kind: ErrorKind::Git,
                message,
                details: None,
                reason: Some(ErrorReason::AuthFailed),
            }
        } else if lower.contains("connection refused")
            || lower.contains("could not resolve host")
            || lower.contains("failed to connect")
            || lower.contains("connection timed out")
            || lower.contains("network is unreachable")
        {
            Self {
                kind: ErrorKind::Network,
                message,
                details: None,
                reason: None,
            }
        } else {
            Self {
                kind: ErrorKind::Git,
                message,
                details: None,
                reason: None,
            }
        }
    }

    /// Convert an `anyhow::Error` originating from network operations.
    pub fn network(e: impl fmt::Display) -> Self {
        Self {
            kind: ErrorKind::Network,
            message: e.to_string(),
            details: None,
            reason: None,
        }
    }

    /// Convert an `anyhow::Error` originating from IO operations.
    pub fn io(e: impl fmt::Display) -> Self {
        Self {
            kind: ErrorKind::Io,
            message: e.to_string(),
            details: None,
            reason: None,
        }
    }

    /// A deployment refusal, with the paths that stopped it. Callers that
    /// drive this programmatically (the CLI, and the frontend toast) need the
    /// paths, not a sentence containing them.
    pub fn target_conflict(
        message: impl Into<String>,
        conflicts: Vec<TargetConflictDetail>,
    ) -> Self {
        Self {
            kind: ErrorKind::TargetConflict,
            message: message.into(),
            details: Some(ErrorDetails { conflicts }),
            reason: None,
        }
    }

    pub fn internal(e: impl fmt::Display) -> Self {
        Self {
            kind: ErrorKind::Internal,
            message: e.to_string(),
            details: None,
            reason: None,
        }
    }
}

impl std::error::Error for AppError {}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self {
            kind: ErrorKind::Io,
            message: e.to_string(),
            details: None,
            reason: None,
        }
    }
}

impl From<tokio::task::JoinError> for AppError {
    fn from(e: tokio::task::JoinError) -> Self {
        Self {
            kind: ErrorKind::Internal,
            message: e.to_string(),
            details: None,
            reason: None,
        }
    }
}

impl From<tauri::Error> for AppError {
    fn from(e: tauri::Error) -> Self {
        Self {
            kind: ErrorKind::Internal,
            message: e.to_string(),
            details: None,
            reason: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_git_error_detects_cancelled() {
        let err = AppError::classify_git_error("Installation cancelled by user");
        assert!(matches!(err.kind, ErrorKind::Cancelled));
    }

    #[test]
    fn classify_git_error_detects_canceled_american_spelling() {
        let err = AppError::classify_git_error("Operation was canceled");
        assert!(matches!(err.kind, ErrorKind::Cancelled));
    }

    #[test]
    fn classify_git_error_regular_git_error() {
        let err = AppError::classify_git_error("Failed to push to remote");
        assert!(matches!(err.kind, ErrorKind::Git));
    }

    #[test]
    fn classify_git_error_case_insensitive() {
        let err = AppError::classify_git_error("CANCELLED by system");
        assert!(matches!(err.kind, ErrorKind::Cancelled));
    }

    #[test]
    fn classify_git_error_detects_connection_refused() {
        let err = AppError::classify_git_error("fatal: unable to access 'https://gitea.example.com/user/repo.git/': Failed to connect to gitea.example.com port 443: Connection refused");
        assert!(matches!(err.kind, ErrorKind::Network));
    }

    #[test]
    fn classify_git_error_detects_could_not_resolve_host() {
        let err = AppError::classify_git_error(
            "fatal: unable to access: Could not resolve host: example.com",
        );
        assert!(matches!(err.kind, ErrorKind::Network));
    }

    /// The frontend branches on `reason`, so its spelling is a wire contract
    /// like any other: renaming the variant silently turns a specific hint into
    /// generic advice. Locked here because nothing else exercises the JSON.
    #[test]
    fn actionable_reasons_serialize_as_the_frontend_expects() {
        let wire = |err: AppError| serde_json::to_value(err).unwrap();

        let auth = AppError::classify_git_error(
            "fatal: Authentication failed for 'https://github.com/acme/private.git/'",
        );
        assert_eq!(auth.reason, Some(ErrorReason::AuthFailed));
        assert_eq!(
            wire(auth)["reason"],
            "auth_failed",
            "src/lib/error.ts matches on this exact string"
        );

        assert_eq!(
            wire(AppError::subpath_required("no SKILL.md at the root"))["reason"],
            "subpath_required"
        );

        // Every other error must keep the old two-field shape, or every
        // consumer that reads `kind` breaks.
        let plain = wire(AppError::not_found("gone"));
        assert_eq!(plain["kind"], "not_found");
        assert!(
            plain.get("reason").is_none(),
            "reason is omitted when absent, not serialized as null"
        );
    }

    /// A private repository fails at the *first* network call, well before a
    /// clone is attempted, so the classifier has to be on that one too.
    #[test]
    fn classify_git_error_distinguishes_credentials_from_an_absent_repository() {
        for message in [
            "Authentication failed: no credentials available",
            "could not read Username for 'https://github.com': terminal prompts disabled",
            "git@github.com: Permission denied (publickey).",
        ] {
            assert_eq!(
                AppError::classify_git_error(message).reason,
                Some(ErrorReason::AuthFailed),
                "{message} should read as needing credentials"
            );
        }
        assert_eq!(
            AppError::classify_git_error("repository not found").reason,
            None,
            "a missing repository is a different problem with a different hint"
        );
    }

    #[test]
    fn constructors_set_correct_kinds() {
        assert!(matches!(AppError::not_found("x").kind, ErrorKind::NotFound));
        assert!(matches!(
            AppError::invalid_input("x").kind,
            ErrorKind::InvalidInput
        ));
        assert!(matches!(
            AppError::cancelled("x").kind,
            ErrorKind::Cancelled
        ));
        assert!(matches!(AppError::db("x").kind, ErrorKind::Database));
        assert!(matches!(AppError::git("x").kind, ErrorKind::Git));
        assert!(matches!(AppError::network("x").kind, ErrorKind::Network));
        assert!(matches!(AppError::io("x").kind, ErrorKind::Io));
        assert!(matches!(AppError::internal("x").kind, ErrorKind::Internal));
    }

    #[test]
    fn display_shows_message() {
        let err = AppError::not_found("Skill not found");
        assert_eq!(format!("{}", err), "Skill not found");
    }

    #[test]
    fn serializes_to_json_with_kind_and_message() {
        let err = AppError::not_found("missing");
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["kind"], "not_found");
        assert_eq!(json["message"], "missing");
    }

    #[test]
    fn error_kind_serializes_snake_case() {
        let err = AppError::invalid_input("bad");
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["kind"], "invalid_input");
    }

    #[test]
    fn from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file gone");
        let app_err: AppError = io_err.into();
        assert!(matches!(app_err.kind, ErrorKind::Io));
        assert!(app_err.message.contains("file gone"));
    }
}
