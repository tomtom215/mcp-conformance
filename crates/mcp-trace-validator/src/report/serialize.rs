// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! [`Report`]'s JSON form: its fields, with the verdict they imply.
//!
//! Written by hand rather than derived because the verdict is a function of the
//! totals, not a field: storing it would let the two disagree. The member order
//! is the report's declaration order with `verdict` after the revision fields,
//! where a reader of the JSON looks first.

use serde::ser::{Serialize, SerializeStruct, Serializer};

use super::Report;

impl Serialize for Report {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut report = serializer.serialize_struct("Report", 6)?;
        report.serialize_field("revision", &self.revision)?;
        if let Some(mismatch) = &self.revision_mismatch {
            report.serialize_field("revision_mismatch", mismatch)?;
        } else {
            report.skip_field("revision_mismatch")?;
        }
        if let Some(source) = &self.revision_source {
            report.serialize_field("revision_source", source)?;
        } else {
            report.skip_field("revision_source")?;
        }
        report.serialize_field("verdict", &self.verdict())?;
        report.serialize_field("totals", &self.totals)?;
        report.serialize_field("requirements", &self.requirements)?;
        report.end()
    }
}
