// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Resource catalog of the everything server.
//!
//! The suite's `resources-*` scenarios define it exactly: two direct
//! resources (`test://static-text`, `test://static-binary`), one template
//! (`test://template/{id}/data`) substituting its `{id}`, and
//! subscribe/unsubscribe bookkeeping (held in
//! [`crate::server::EverythingServer`]; this module is the pure catalog).
//!
//! Both listings are served in two pages ([`crate::pagination`]). The first is
//! the suite's catalogue, unchanged; the second carries what the suite does
//! not name — one resource and one template, each with a title, icons and
//! the full `Annotations` shape (audience, priority, `lastModified`).

use rmcp::model::{ReadResourceResult, Resource, ResourceContents, ResourceTemplate};

use crate::fixtures::{self, TINY_PNG_BASE64};

/// URI of the static text resource (`resources-read-text`).
pub const STATIC_TEXT_URI: &str = "test://static-text";
/// URI of the static binary resource (`resources-read-binary`).
pub const STATIC_BINARY_URI: &str = "test://static-binary";
/// RFC 6570 template the `resources-templates-read` scenario substitutes.
pub const TEMPLATE_URI: &str = "test://template/{id}/data";
/// URI of the annotated resource on `resources/list`'s second page — also the
/// target of `test_resource_link`'s link.
pub const ANNOTATED_URI: &str = "test://annotated-text";
/// The template on `resources/templates/list`'s second page.
pub const ANNOTATED_TEMPLATE_URI: &str = "test://annotated/{id}";

/// The body of [`ANNOTATED_URI`]; its byte length is the listed `size`.
const ANNOTATED_TEXT: &str = "This resource carries a title, icons and annotations.";

/// Which page of `resources/list` a resource is listed on.
#[must_use]
pub fn page_of(resource: &Resource) -> usize {
    usize::from(resource.uri == ANNOTATED_URI)
}

/// Which page of `resources/templates/list` a template is listed on.
#[must_use]
pub fn template_page_of(template: &ResourceTemplate) -> usize {
    usize::from(template.uri_template == ANNOTATED_TEMPLATE_URI)
}

/// The resource the annotated listing entry and `test_resource_link` both
/// describe.
#[must_use]
pub fn annotated() -> Resource {
    Resource::new(ANNOTATED_URI, "annotated-text")
        .with_title("Annotated text")
        .with_description("A text resource carrying a title, icons and annotations")
        .with_mime_type("text/plain")
        .with_size(ANNOTATED_TEXT.len() as u64)
        .with_icons(fixtures::icons())
        .with_annotations(fixtures::annotations())
}

/// Every direct resource, as `resources/list` reports it across its pages.
#[must_use]
pub fn catalog() -> Vec<Resource> {
    // rmcp 3.x flattened `Annotated<RawResource>` into a `#[non_exhaustive]`
    // `Resource` carrying `annotations` directly, so these are built through
    // `new` + field assignment rather than a struct literal. The emitted JSON
    // is unchanged: every field left at its `new` default is
    // `skip_serializing_if = "Option::is_none"`, and the old `no_annotation()`
    // meant exactly today's `annotations: None`.
    vec![
        {
            let mut resource = Resource::new(STATIC_TEXT_URI, "static-text");
            resource.description = Some("A static text resource for conformance testing".into());
            resource.mime_type = Some("text/plain".into());
            resource
        },
        {
            let mut resource = Resource::new(STATIC_BINARY_URI, "static-binary");
            resource.description = Some("A static binary resource for conformance testing".into());
            resource.mime_type = Some("image/png".into());
            resource
        },
        annotated(),
    ]
}

/// Every resource template, as `resources/templates/list` reports it across
/// its pages.
#[must_use]
pub fn templates() -> Vec<ResourceTemplate> {
    vec![
        {
            let mut template = ResourceTemplate::new(TEMPLATE_URI, "template-data");
            template.description =
                Some("Parameterized resource template for conformance testing".into());
            template.mime_type = Some("application/json".into());
            template
        },
        ResourceTemplate::new(ANNOTATED_TEMPLATE_URI, "annotated-template")
            .with_title("Annotated template")
            .with_description("A resource template carrying a title, icons and annotations")
            .with_mime_type("text/plain")
            .with_icons(fixtures::icons())
            .with_annotations(fixtures::annotations()),
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
    if uri == ANNOTATED_URI {
        return Some(ReadResourceResult::new(vec![
            ResourceContents::TextResourceContents {
                uri: uri.into(),
                mime_type: Some("text/plain".into()),
                text: ANNOTATED_TEXT.into(),
                meta: None,
            },
        ]));
    }
    if let Some(id) = annotated_id(uri) {
        return Some(ReadResourceResult::new(vec![
            ResourceContents::TextResourceContents {
                uri: uri.into(),
                mime_type: Some("text/plain".into()),
                text: format!("Annotated resource {id}."),
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

/// Extracts `{id}` from `test://annotated/{id}` instantiations, under the
/// same one-non-empty-segment rule as [`template_id`].
fn annotated_id(uri: &str) -> Option<&str> {
    let id = uri.strip_prefix("test://annotated/")?;
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
        assert_eq!(resources.len(), 3);
        assert!(resources.iter().all(|r| r.description.is_some()));
        // The first page is exactly the suite's two fixtures, in order.
        let first: Vec<&str> = resources
            .iter()
            .filter(|r| page_of(r) == 0)
            .map(|r| r.uri.as_str())
            .collect();
        assert_eq!(first, [STATIC_TEXT_URI, STATIC_BINARY_URI]);
        let templates = templates();
        assert_eq!(templates.len(), 2);
        assert_eq!(templates[0].uri_template, TEMPLATE_URI);
        assert_eq!(template_page_of(&templates[0]), 0);
        assert_eq!(template_page_of(&templates[1]), 1);
    }

    #[test]
    fn the_second_page_carries_icons_and_annotations_and_reads_back() {
        let annotated = catalog().into_iter().find(|r| page_of(r) == 1).unwrap();
        let value = serde_json::to_value(&annotated).unwrap();
        assert_eq!(value["uri"], ANNOTATED_URI);
        assert_eq!(value["title"], "Annotated text");
        assert_eq!(value["icons"][0]["mimeType"], "image/png");
        assert_eq!(
            value["annotations"]["audience"],
            serde_json::json!(["user", "assistant"])
        );
        // The listed size is the size of what a read returns.
        let body = read(ANNOTATED_URI).unwrap();
        let ResourceContents::TextResourceContents { text, .. } = &body.contents[0] else {
            panic!("the annotated resource is text");
        };
        assert_eq!(value["size"], text.len());

        let template = &templates()[1];
        let value = serde_json::to_value(template).unwrap();
        assert_eq!(value["uriTemplate"], ANNOTATED_TEMPLATE_URI);
        assert_eq!(value["annotations"]["priority"], 0.5);
        assert!(
            read("test://annotated/7").is_some(),
            "the template resolves"
        );
        for uri in ["test://annotated/", "test://annotated/a/b"] {
            assert!(read(uri).is_none(), "{uri} must not resolve");
        }
    }
}
