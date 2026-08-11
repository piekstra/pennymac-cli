//! Catalog of the portal's mutating endpoints — everything this CLI
//! deliberately does *not* do.
//!
//! Two jobs, one source of truth:
//!
//! 1. `pmac writes` prints it, so the gap between "what the portal can do" and
//!    "what this read-only CLI does" is inspectable rather than tribal.
//! 2. `pmac api` refuses to POST to any path listed here, so the raw escape
//!    hatch cannot move money or change the account by accident.
//!
//! These paths were read from the mypennymac front end's own XHR calls and its
//! JavaScript; see `docs/api.md`. **None have been called** — the paths are
//! transcribed, not exercised, and any future implementation must verify them
//! before trusting them. On a mortgage, the write surface moves real money.

/// A portal capability this CLI does not implement.
pub struct Capability {
    /// HTTP method the portal's front end uses.
    pub method: &'static str,
    /// Path under the portal host.
    pub path: &'static str,
    /// Grouping for the `writes` table.
    pub category: Category,
    /// What calling it would do.
    pub description: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// Moves money, now or on a schedule. The highest-stakes group.
    Money,
    /// Adds, edits, or removes a stored bank account.
    PaymentMethod,
    /// Changes escrow, autopay, or paperless settings.
    Settings,
    /// Changes the login or profile itself.
    Account,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Money => "money",
            Category::PaymentMethod => "payment-method",
            Category::Settings => "settings",
            Category::Account => "account",
        }
    }
}

/// Every mutating endpoint observed in the portal's front end.
pub const CAPABILITIES: &[Capability] = &[
    // ---- money ----
    Capability {
        method: "POST",
        path: "/api/payment/submit_payment",
        category: Category::Money,
        description: "Submit a one-time mortgage payment (ACH).",
    },
    Capability {
        method: "POST",
        path: "/api/payment/submit_onetime_payment",
        category: Category::Money,
        description: "Submit a one-time payment from the make-a-payment flow.",
    },
    Capability {
        method: "POST",
        path: "/api/payment/cancel_payment",
        category: Category::Money,
        description: "Cancel a submitted payment that has not yet processed.",
    },
    Capability {
        method: "POST",
        path: "/api/payment/setup_autopay",
        category: Category::Money,
        description: "Create or update recurring automatic payments (autopay).",
    },
    Capability {
        method: "POST",
        path: "/api/payment/cancel_autopay",
        category: Category::Money,
        description: "Cancel recurring automatic payments.",
    },
    Capability {
        method: "POST",
        path: "/api/payment/submit_curtailment",
        category: Category::Money,
        description: "Submit an additional principal (curtailment) payment.",
    },
    // ---- payment methods ----
    Capability {
        method: "POST",
        path: "/api/payment/add_bank_account",
        category: Category::PaymentMethod,
        description: "Save a bank account (routing + account number) for payments.",
    },
    Capability {
        method: "POST",
        path: "/api/payment/delete_bank_account",
        category: Category::PaymentMethod,
        description: "Delete a saved bank account. Breaks any autopay using it.",
    },
    // ---- settings ----
    Capability {
        method: "POST",
        path: "/api/loan/submit_escrow_analysis_shortage",
        category: Category::Settings,
        description: "Elect how to pay an escrow shortage from an analysis.",
    },
    Capability {
        method: "POST",
        path: "/api/documents/set_paperless",
        category: Category::Settings,
        description: "Change paperless / e-statement delivery preferences.",
    },
    Capability {
        method: "POST",
        path: "/api/messages/send_message",
        category: Category::Settings,
        description: "Send a secure message to servicing.",
    },
    Capability {
        method: "POST",
        path: "/api/messages/mark_read",
        category: Category::Settings,
        description: "Mark message-center items as read.",
    },
    // ---- account ----
    Capability {
        method: "POST",
        path: "/api/account/update_profile",
        category: Category::Account,
        description: "Change the name, phone, or email on the account.",
    },
    Capability {
        method: "POST",
        path: "/users/mfa/set_mfa",
        category: Category::Account,
        description: "Change the second-factor method on the identity login.",
    },
    Capability {
        method: "POST",
        path: "/users/validate",
        category: Category::Account,
        description: "The login endpoint (`auth login` uses it deliberately; \
                      listed so `api` won't post credentials to it blindly).",
    },
];

/// Whether a path is a known mutating endpoint.
///
/// Compared case-insensitively and ignoring a trailing slash: portals spell the
/// same route with mixed casing across their code, so a guard that matched only
/// one casing would be trivially bypassed by typing the other.
pub fn is_write(path: &str) -> bool {
    let normalized = normalize(path);
    CAPABILITIES.iter().any(|c| normalize(c.path) == normalized)
}

fn normalize(path: &str) -> String {
    let path = path.split(['?', '#']).next().unwrap_or(path);
    path.trim_end_matches('/').to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_write_paths_are_recognized() {
        assert!(is_write("/api/payment/submit_payment"));
        assert!(is_write("/api/payment/setup_autopay"));
        assert!(is_write("/api/account/update_profile"));
    }

    #[test]
    fn the_guard_is_case_and_slash_insensitive() {
        assert!(is_write("/API/Payment/Submit_Payment"));
        assert!(is_write("/api/payment/submit_payment/"));
        assert!(is_write("/api/payment/submit_payment?x=1"));
    }

    #[test]
    fn read_paths_are_not_writes() {
        for path in [
            "/api/loan/loans",
            "/api/loan/get_loan_activity",
            "/api/documents/get_docs",
            "/api/payment/get_payment_info",
            "/api/account/request_token",
        ] {
            assert!(!is_write(path), "{path} must not be treated as a write");
        }
    }

    #[test]
    fn a_read_path_that_merely_starts_like_a_write_is_allowed() {
        assert!(!is_write("/api/payment/submit_payment_preview"));
    }

    #[test]
    fn every_capability_is_well_formed_and_unique() {
        let mut seen = Vec::new();
        for c in CAPABILITIES {
            assert!(c.path.starts_with('/'), "{} needs a leading slash", c.path);
            assert!(!c.description.is_empty(), "{} needs a description", c.path);
            assert_eq!(c.method, "POST", "{} — unexpected method", c.path);
            seen.push(normalize(c.path));
        }
        let before = seen.len();
        seen.sort();
        seen.dedup();
        assert_eq!(before, seen.len(), "duplicate capability path");
    }
}
