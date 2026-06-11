// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! Tools that call back into the client mid-execution.
//!
//! `test_sampling` requests `sampling/createMessage`; the three elicitation
//! tools request `elicitation/create` with the exact schemas their scenarios
//! prescribe (the base username/email form, SEP-1034's all-primitive
//! defaults, SEP-1330's five enum variants). Each checks the client's
//! advertised capability first and returns a protocol error when the client
//! cannot answer — the scenarios' "if the client doesn't support X, return
//! an error" clause.

use std::collections::BTreeMap;

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    BooleanSchema, CallToolResult, ConstTitle, Content, CreateElicitationRequestParams,
    CreateMessageRequestParams, ElicitationSchema, EnumSchema, ErrorData, IntegerSchema,
    LegacyEnumSchema, MultiSelectEnumSchema, NumberSchema, PrimitiveSchema, SamplingMessage,
    SingleSelectEnumSchema, StringSchema, StringTypeConst, TitledItems,
    TitledMultiSelectEnumSchema, TitledSingleSelectEnumSchema,
};
use rmcp::service::RequestContext;
use rmcp::{RoleServer, tool, tool_router};

use crate::server::EverythingServer;

/// Arguments for `test_sampling`.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SamplingArgs {
    /// The prompt to send to the LLM
    pub prompt: String,
}

/// Arguments for `test_elicitation`.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ElicitationArgs {
    /// The message to show the user
    pub message: String,
}

#[tool_router(router = tool_router_interactive, vis = "pub(crate)")]
impl EverythingServer {
    /// `tools-call-sampling`: forwards the prompt to the client's LLM via
    /// `sampling/createMessage` (`maxTokens: 100` per the scenario).
    ///
    /// # Errors
    ///
    /// Errors when the client did not advertise the `sampling` capability,
    /// or when the sampling request itself fails.
    #[tool(description = "Requests LLM sampling from the client for conformance testing")]
    pub async fn test_sampling(
        &self,
        Parameters(SamplingArgs { prompt }): Parameters<SamplingArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let supported = context
            .peer
            .peer_info()
            .is_some_and(|info| info.capabilities.sampling.is_some());
        if !supported {
            return Err(ErrorData::invalid_request(
                "client does not support sampling (no sampling capability advertised)",
                None,
            ));
        }
        let result = context
            .peer
            .create_message(CreateMessageRequestParams::new(
                vec![SamplingMessage::user_text(prompt)],
                100,
            ))
            .await
            .map_err(|error| {
                ErrorData::internal_error(
                    "sampling/createMessage failed",
                    Some(serde_json::json!({ "error": error.to_string() })),
                )
            })?;
        let text = result
            .message
            .content
            .into_vec()
            .into_iter()
            .find_map(|content| content.as_text().map(|t| t.text.clone()))
            .unwrap_or_else(|| "(non-text response)".to_owned());
        Ok(CallToolResult::success(vec![Content::text(format!(
            "LLM response: {text}"
        ))]))
    }

    /// `tools-call-elicitation`: requests user input with the scenario's
    /// username/email schema (both required).
    ///
    /// # Errors
    ///
    /// Errors when the client did not advertise the `elicitation`
    /// capability, or when the elicitation request itself fails.
    #[tool(description = "Requests user input from the client for conformance testing")]
    pub async fn test_elicitation(
        &self,
        Parameters(ElicitationArgs { message }): Parameters<ElicitationArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let schema = ElicitationSchema::builder()
            .required_property(
                "username",
                PrimitiveSchema::String(StringSchema::new().description("User's response")),
            )
            .required_property(
                "email",
                PrimitiveSchema::String(StringSchema::new().description("User's email address")),
            )
            .build()
            .map_err(invalid_schema)?;
        elicit(&context, message, schema, "User response").await
    }

    /// `elicitation-sep1034-defaults`: every primitive type carrying a
    /// default — string, integer, number, enum, boolean.
    ///
    /// # Errors
    ///
    /// Errors when the client did not advertise the `elicitation`
    /// capability, or when the elicitation request itself fails.
    #[tool(description = "Elicitation with SEP-1034 default values for all primitive types")]
    pub async fn test_elicitation_sep1034_defaults(
        &self,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let status = EnumSchema::builder(vec![
            "active".to_owned(),
            "inactive".to_owned(),
            "pending".to_owned(),
        ])
        .with_default("active")
        .map_err(invalid_schema)?
        .build();
        let schema = ElicitationSchema::builder()
            .property(
                "name",
                PrimitiveSchema::String(StringSchema::new().with_default("John Doe")),
            )
            .property(
                "age",
                PrimitiveSchema::Integer(IntegerSchema::new().with_default(30)),
            )
            .property(
                "score",
                PrimitiveSchema::Number(NumberSchema::new().with_default(95.5)),
            )
            .property("status", PrimitiveSchema::Enum(status))
            .property(
                "verified",
                PrimitiveSchema::Boolean(BooleanSchema::new().with_default(true)),
            )
            .build()
            .map_err(invalid_schema)?;
        elicit(
            &context,
            "Please confirm or adjust the prefilled values".to_owned(),
            schema,
            "Elicitation completed",
        )
        .await
    }

    /// `elicitation-sep1330-enums`: all five enum schema variants in one
    /// request — untitled/titled single-select, the deprecated
    /// `enumNames` form, untitled/titled multi-select.
    ///
    /// # Errors
    ///
    /// Errors when the client did not advertise the `elicitation`
    /// capability, or when the elicitation request itself fails.
    #[tool(description = "Elicitation with SEP-1330 enum schema variants")]
    pub async fn test_elicitation_sep1330_enums(
        &self,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let schema = sep1330_schema();
        elicit(
            &context,
            "Please choose from the enum variants".to_owned(),
            schema,
            "Elicitation completed",
        )
        .await
    }
}

/// The SEP-1330 schema: all five enum variants. A named constructor so the
/// exact wire shape — including the legacy `enumNames` field, which rmcp's
/// own client-side untagged deserialization silently drops — is unit-tested
/// against serialization, not a lossy round-trip.
pub(crate) fn sep1330_schema() -> ElicitationSchema {
    let mut properties = single_select_variants();
    properties.append(&mut multi_select_variants());
    ElicitationSchema::new(properties)
}

/// SEP-1330 variants 1–3: the single-select shapes.
fn single_select_variants() -> BTreeMap<String, PrimitiveSchema> {
    {
        let mut properties = BTreeMap::new();
        properties.insert(
            "untitledSingle".to_owned(),
            PrimitiveSchema::Enum(
                EnumSchema::builder(vec![
                    "option1".to_owned(),
                    "option2".to_owned(),
                    "option3".to_owned(),
                ])
                .build(),
            ),
        );
        properties.insert(
            "titledSingle".to_owned(),
            PrimitiveSchema::Enum(EnumSchema::Single(SingleSelectEnumSchema::Titled(
                TitledSingleSelectEnumSchema::new(vec![
                    ConstTitle::new("value1", "First Option"),
                    ConstTitle::new("value2", "Second Option"),
                    ConstTitle::new("value3", "Third Option"),
                ]),
            ))),
        );
        properties.insert(
            "legacyEnum".to_owned(),
            PrimitiveSchema::Enum(EnumSchema::Legacy(LegacyEnumSchema {
                type_: StringTypeConst,
                title: None,
                description: None,
                enum_: vec!["opt1".into(), "opt2".into(), "opt3".into()],
                enum_names: Some(vec![
                    "Option One".into(),
                    "Option Two".into(),
                    "Option Three".into(),
                ]),
            })),
        );
        properties
    }
}

/// SEP-1330 variants 4–5: the multi-select shapes.
fn multi_select_variants() -> BTreeMap<String, PrimitiveSchema> {
    {
        let mut properties = BTreeMap::new();
        properties.insert(
            "untitledMulti".to_owned(),
            PrimitiveSchema::Enum(
                EnumSchema::builder(vec![
                    "option1".to_owned(),
                    "option2".to_owned(),
                    "option3".to_owned(),
                ])
                .multiselect()
                .build(),
            ),
        );
        properties.insert(
            "titledMulti".to_owned(),
            PrimitiveSchema::Enum(EnumSchema::Multi(MultiSelectEnumSchema::Titled(
                TitledMultiSelectEnumSchema::new(TitledItems::new(vec![
                    ConstTitle::new("value1", "First Choice"),
                    ConstTitle::new("value2", "Second Choice"),
                    ConstTitle::new("value3", "Third Choice"),
                ])),
            ))),
        );
        properties
    }
}

/// Shared elicitation flow: capability check, raw `elicitation/create`,
/// result formatting (`<prefix>: action=…, content=…`).
async fn elicit(
    context: &RequestContext<RoleServer>,
    message: String,
    schema: ElicitationSchema,
    prefix: &str,
) -> Result<CallToolResult, ErrorData> {
    let supported = context
        .peer
        .peer_info()
        .is_some_and(|info| info.capabilities.elicitation.is_some());
    if !supported {
        return Err(ErrorData::invalid_request(
            "client does not support elicitation (no elicitation capability advertised)",
            None,
        ));
    }
    let result = context
        .peer
        .create_elicitation(CreateElicitationRequestParams::FormElicitationParams {
            meta: None,
            message,
            requested_schema: schema,
        })
        .await
        .map_err(|error| {
            ErrorData::internal_error(
                "elicitation/create failed",
                Some(serde_json::json!({ "error": error.to_string() })),
            )
        })?;
    let action = serde_json::to_value(result.action)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| "unknown".to_owned());
    let reply = result
        .content
        .map_or_else(|| "null".to_owned(), |value| value.to_string());
    Ok(CallToolResult::success(vec![Content::text(format!(
        "{prefix}: action={action}, content={reply}"
    ))]))
}

/// Maps schema-builder validation failures into protocol errors.
fn invalid_schema(message: impl AsRef<str>) -> ErrorData {
    ErrorData::internal_error(
        "elicitation schema construction failed",
        Some(serde_json::json!({ "message": message.as_ref() })),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn invalid_schema_carries_the_failure_payload() {
        let error = invalid_schema("boom");
        assert_eq!(error.code, rmcp::model::ErrorCode::INTERNAL_ERROR);
        assert_eq!(error.message, "elicitation schema construction failed");
        assert_eq!(error.data, Some(serde_json::json!({ "message": "boom" })));
    }

    /// The true wire shape, asserted at serialization: the duplex round-trip
    /// cannot check `enumNames` because rmcp's client-side untagged
    /// `EnumSchema` deserialization matches the legacy form as `Untitled`
    /// first and silently drops the field (upstream-filing candidate).
    #[test]
    fn sep1330_serializes_all_five_variants_to_the_wire() {
        let schema = serde_json::to_value(sep1330_schema()).unwrap();
        let props = &schema["properties"];
        assert_eq!(props["untitledSingle"]["type"], "string");
        assert_eq!(
            props["untitledSingle"]["enum"],
            serde_json::json!(["option1", "option2", "option3"])
        );
        assert_eq!(
            props["titledSingle"]["oneOf"][0],
            serde_json::json!({"const": "value1", "title": "First Option"})
        );
        assert_eq!(
            props["legacyEnum"]["enum"],
            serde_json::json!(["opt1", "opt2", "opt3"])
        );
        assert_eq!(
            props["legacyEnum"]["enumNames"],
            serde_json::json!(["Option One", "Option Two", "Option Three"])
        );
        assert_eq!(props["untitledMulti"]["type"], "array");
        assert_eq!(
            props["untitledMulti"]["items"]["enum"],
            serde_json::json!(["option1", "option2", "option3"])
        );
        assert_eq!(props["titledMulti"]["type"], "array");
        assert_eq!(
            props["titledMulti"]["items"]["anyOf"][0],
            serde_json::json!({"const": "value1", "title": "First Choice"})
        );
    }
}
