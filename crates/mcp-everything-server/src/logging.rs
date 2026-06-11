// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Logging-level policy: `logging/setLevel` state and its filter.
//!
//! The `logging-set-level` scenario requires the server to accept a level
//! and *"filter subsequent log notifications based on level"*. rmcp's
//! `LoggingLevel` doesn't implement `Ord`, so the spec's severity order
//! (`debug` lowest → `emergency` highest) is encoded here explicitly and
//! exhaustively tested.

use rmcp::model::LoggingLevel;

/// Spec severity order, `debug` = 0 (least severe) … `emergency` = 7.
const fn severity(level: LoggingLevel) -> u8 {
    match level {
        LoggingLevel::Debug => 0,
        LoggingLevel::Info => 1,
        LoggingLevel::Notice => 2,
        LoggingLevel::Warning => 3,
        LoggingLevel::Error => 4,
        LoggingLevel::Critical => 5,
        LoggingLevel::Alert => 6,
        LoggingLevel::Emergency => 7,
    }
}

/// Whether a message at `candidate` severity passes a `threshold` set via
/// `logging/setLevel`: everything at or above the threshold flows.
#[must_use]
pub const fn permits(threshold: LoggingLevel, candidate: LoggingLevel) -> bool {
    severity(candidate) >= severity(threshold)
}

/// The level before any `logging/setLevel`: `debug`, i.e. everything flows —
/// a conformance server must not hide messages the client never asked to
/// filter.
#[must_use]
pub const fn default_level() -> LoggingLevel {
    LoggingLevel::Debug
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [LoggingLevel; 8] = [
        LoggingLevel::Debug,
        LoggingLevel::Info,
        LoggingLevel::Notice,
        LoggingLevel::Warning,
        LoggingLevel::Error,
        LoggingLevel::Critical,
        LoggingLevel::Alert,
        LoggingLevel::Emergency,
    ];

    #[test]
    fn severity_is_strictly_increasing_in_spec_order() {
        for window in ALL.windows(2) {
            assert!(
                severity(window[0]) < severity(window[1]),
                "{:?} must rank below {:?}",
                window[0],
                window[1]
            );
        }
    }

    #[test]
    fn threshold_admits_itself_and_everything_above() {
        for (i, threshold) in ALL.iter().enumerate() {
            for (j, candidate) in ALL.iter().enumerate() {
                assert_eq!(
                    permits(*threshold, *candidate),
                    j >= i,
                    "threshold {threshold:?} vs candidate {candidate:?}"
                );
            }
        }
    }

    #[test]
    fn default_level_filters_nothing() {
        for candidate in ALL {
            assert!(permits(default_level(), candidate));
        }
    }
}
