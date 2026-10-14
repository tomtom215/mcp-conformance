// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! [`Report`]'s and [`ExtensionReport`]'s JSON form: their fields, with the
//! verdict they imply.
//!
//! Written by hand rather than derived because the verdict is a function of the
//! totals, not a field: storing it would let the two disagree. The member order
//! is the report's declaration order with `verdict` after the revision fields,
//! where a reader of the JSON looks first.

use serde::ser::{Serialize, SerializeStruct, Serializer};

use super::{ExtensionReport, Report};

impl Serialize for Report {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut report = serializer.serialize_struct("Report", 8)?;
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
        if let Some(wire_schema) = &self.wire_schema {
            report.serialize_field("wire_schema", wire_schema)?;
        } else {
            report.skip_field("wire_schema")?;
        }
        if self.extensions.is_empty() {
            report.skip_field("extensions")?;
        } else {
            report.serialize_field("extensions", &self.extensions)?;
        }
        report.end()
    }
}

impl Serialize for ExtensionReport {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut extension = serializer.serialize_struct("ExtensionReport", 4)?;
        extension.serialize_field("extension", &self.extension)?;
        extension.serialize_field("verdict", &self.verdict())?;
        extension.serialize_field("totals", &self.totals)?;
        extension.serialize_field("requirements", &self.requirements)?;
        extension.end()
    }
}
