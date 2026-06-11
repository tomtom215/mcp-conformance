// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Resource catalog of the everything server.
//!
//! The suite's `resources-*` scenarios define it exactly: two direct
//! resources (`test://static-text`, `test://static-binary`), one template
//! (`test://template/{id}/data`) substituting its `{id}`, and
//! subscribe/unsubscribe bookkeeping (held in
//! [`crate::server::EverythingServer`]; this module is the pure catalog).

use rmcp::model::{
    AnnotateAble as _, RawResource, RawResourceTemplate, ReadResourceResult, Resource,
    ResourceContents, ResourceTemplate,
};

use crate::fixtures::TINY_PNG_BASE64;

/// URI of the static text resource (`resources-read-text`).
pub const STATIC_TEXT_URI: &str = "test://static-text";
/// URI of the static binary resource (`resources-read-binary`).
pub const STATIC_BINARY_URI: &str = "test://static-binary";
/// RFC 6570 template the `resources-templates-read` scenario substitutes.
pub const TEMPLATE_URI: &str = "test://template/{id}/data";

/// Every direct resource, as `resources/list` reports it.
#[must_use]
pub fn catalog() -> Vec<Resource> {
    vec![
        RawResource {
            uri: STATIC_TEXT_URI.into(),
            name: "static-text".into(),
            title: None,
            description: Some("A static text resource for conformance testing".into()),
            mime_type: Some("text/plain".into()),
            size: None,
            icons: None,
            meta: None,
        }
        .no_annotation(),
        RawResource {
            uri: STATIC_BINARY_URI.into(),
            name: "static-binary".into(),
            title: None,
            description: Some("A static binary resource for conformance testing".into()),
            mime_type: Some("image/png".into()),
            size: None,
            icons: None,
            meta: None,
        }
        .no_annotation(),
    ]
}

/// Every resource template, as `resources/templates/list` reports it.
#[must_use]
pub fn templates() -> Vec<ResourceTemplate> {
    vec![
        RawResourceTemplate {
            uri_template: TEMPLATE_URI.into(),
            name: "template-data".into(),
            title: None,
            description: Some("Parameterized resource template for conformance testing".into()),
            mime_type: Some("application/json".into()),
            icons: None,
        }
        .no_annotation(),
    ]
}

/// Resolves a URI to its contents; `None` is "no such resource".
///
/// Template reads substitute `{id}` exactly as the scenario specifies:
/// `test://template/123/data` answers with JSON naming id `123`.
#[must_use]
pub fn read(uri: &str) -> Option<ReadResourceResult> {
    if uri == STATIC_TEXT_URI {
        return Some(ReadResourceResult::new(vec![
            ResourceContents::TextResourceContents {
                uri: uri.into(),
                mime_type: Some("text/plain".into()),
                text: "This is the content of the static text resource.".into(),
                meta: None,
            },
        ]));
    }
    if uri == STATIC_BINARY_URI {
        return Some(ReadResourceResult::new(vec![
            ResourceContents::BlobResourceContents {
                uri: uri.into(),
                mime_type: Some("image/png".into()),
                blob: TINY_PNG_BASE64.into(),
                meta: None,
            },
        ]));
    }
    let id = template_id(uri)?;
    Some(ReadResourceResult::new(vec![
        ResourceContents::TextResourceContents {
            uri: uri.into(),
            mime_type: Some("application/json".into()),
            text: format!(r#"{{"id":"{id}","templateTest":true,"data":"Data for ID: {id}"}}"#),
            meta: None,
        },
    ]))
}

/// Extracts `{id}` from `test://template/{id}/data` instantiations; `None`
/// for anything that is not exactly one non-empty segment in that shape.
fn template_id(uri: &str) -> Option<&str> {
    let id = uri
        .strip_prefix("test://template/")?
        .strip_suffix("/data")?;
    if id.is_empty() || id.contains('/') {
        return None;
    }
    Some(id)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn template_substitution_matches_the_scenario_example() {
        let result = read("test://template/123/data").unwrap();
        let ResourceContents::TextResourceContents {
            text, mime_type, ..
        } = &result.contents[0]
        else {
            panic!("template read must be text");
        };
        assert_eq!(
            text,
            r#"{"id":"123","templateTest":true,"data":"Data for ID: 123"}"#
        );
        assert_eq!(mime_type.as_deref(), Some("application/json"));
    }

    #[test]
    fn template_rejects_malformed_instantiations() {
        for uri in [
            "test://template//data",
            "test://template/a/b/data",
            "test://template/data",
            "test://other/123/data",
            "test://template/123/",
        ] {
            assert!(read(uri).is_none(), "{uri} must not resolve");
        }
    }

    #[test]
    fn static_reads_carry_their_scenario_contents() {
        let text = read(STATIC_TEXT_URI).unwrap();
        let ResourceContents::TextResourceContents { text, .. } = &text.contents[0] else {
            panic!("static text must be text contents");
        };
        assert_eq!(text, "This is the content of the static text resource.");

        let binary = read(STATIC_BINARY_URI).unwrap();
        let ResourceContents::BlobResourceContents {
            blob, mime_type, ..
        } = &binary.contents[0]
        else {
            panic!("static binary must be blob contents");
        };
        assert_eq!(blob, TINY_PNG_BASE64);
        assert_eq!(mime_type.as_deref(), Some("image/png"));
    }

    #[test]
    fn catalog_lists_direct_resources_with_descriptions() {
        let resources = catalog();
        assert_eq!(resources.len(), 2);
        assert!(resources.iter().all(|r| r.description.is_some()));
        let templates = templates();
        assert_eq!(templates.len(), 1);
        assert_eq!(templates[0].uri_template, TEMPLATE_URI);
    }
}
