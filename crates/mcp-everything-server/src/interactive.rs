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

// SEP-2577 forward-deprecates Roots, Sampling and Logging. They remain fully
// functional and REQUIRED on the `2025-11-25` surface this crate implements
// and the official suite exercises, so rmcp 3.x's deprecation attributes fire
// on correct code — here, Sampling. Scoped to this module, never the crate:
// a blanket allow would also hide a deprecation that genuinely matters. The
// honest cost is that a *different* future deprecation in this module would
// be silenced too. Retires when the `2025-11-25` surface does.
#![allow(deprecated)]
use std::collections::BTreeMap;

use rmcp::handler::server::tool::{InputResponses, RequestState};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    BooleanSchema, CallToolResponse, CallToolResult, ConstTitle, ContentBlock,
    CreateMessageRequestParams, ElicitRequestParams, ElicitationSchema, EnumSchema, ErrorData,
    IntegerSchema, LegacyEnumSchema, MultiSelectEnumSchema, NumberSchema,
    PrimitiveSchemaDefinition, SamplingMessage, SingleSelectEnumSchema, StringSchema, TitledItems,
    TitledMultiSelectEnumSchema, TitledSingleSelectEnumSchema,
};
use rmcp::service::RequestContext;
use rmcp::{RoleServer, tool, tool_router};

use crate::server::{EverythingServer, ServedRevision};

mod capability;
mod mrtr;
use capability::Required;
use mrtr::{Ask, Interaction, Round};

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
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        capability::require(&context, self.revision(), Required::Sampling)?;
        let round = Round {
            tool: "test_sampling",
            state,
            responses,
        };
        let answer = match mrtr::ask(
            &context,
            self.revision(),
            round,
            Ask::Sample(CreateMessageRequestParams::new(
                vec![SamplingMessage::user_text(prompt)],
                100,
            )),
        )
        .await?
        {
            Interaction::Answered(answer) => answer,
            Interaction::Deferred(deferred) => return Ok(deferred.into()),
        };
        Ok(CallToolResult::success(vec![ContentBlock::text(format!(
            "LLM response: {}",
            sampled_text(&answer)
        ))])
        .into())
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
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let round = Round {
            tool: "test_elicitation",
            state,
            responses,
        };
        let schema = ElicitationSchema::builder()
            .required_property(
                "username",
                PrimitiveSchemaDefinition::String(
                    StringSchema::new().description("User's response"),
                ),
            )
            .required_property(
                "email",
                PrimitiveSchemaDefinition::String(
                    StringSchema::new().description("User's email address"),
                ),
            )
            .build()
            .map_err(invalid_schema)?;
        elicit(
            &context,
            self.revision(),
            round,
            message,
            schema,
            "User response",
        )
        .await
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
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let round = Round {
            tool: "test_elicitation_sep1034_defaults",
            state,
            responses,
        };
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
                PrimitiveSchemaDefinition::String(StringSchema::new().with_default("John Doe")),
            )
            .property(
                "age",
                PrimitiveSchemaDefinition::Integer(IntegerSchema::new().with_default(30)),
            )
            .property(
                "score",
                PrimitiveSchemaDefinition::Number(NumberSchema::new().with_default(95.5)),
            )
            .property("status", PrimitiveSchemaDefinition::Enum(status))
            .property(
                "verified",
                PrimitiveSchemaDefinition::Boolean(BooleanSchema::new().with_default(true)),
            )
            .build()
            .map_err(invalid_schema)?;
        elicit(
            &context,
            self.revision(),
            round,
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
        RequestState(state): RequestState,
        InputResponses(responses): InputResponses,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let round = Round {
            tool: "test_elicitation_sep1330_enums",
            state,
            responses,
        };
        let schema = sep1330_schema();
        elicit(
            &context,
            self.revision(),
            round,
            "Please choose from the enum variants".to_owned(),
            schema,
            "Elicitation completed",
        )
        .await
    }

    /// `test_url_elicitation` — the full URL-mode round trip (register 2.10
    /// parity: the TypeScript everything server exercises URL mode; no suite
    /// scenario does). Sends a `mode: "url"` `elicitation/create`; on
    /// consent (`accept`), immediately delivers
    /// `notifications/elicitation/complete` for the issued id — the
    /// out-of-band interaction, compressed to its wire shape — so a client's
    /// pending-id handling is exercised end to end.
    ///
    /// # Errors
    ///
    /// Errors when the client did not advertise the `elicitation`
    /// capability, or when the elicitation request itself fails.
    #[tool(description = "URL-mode elicitation round trip for conformance testing")]
    pub async fn test_url_elicitation(
        &self,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        // Server-unique id: URL elicitations are completed *by id*, and a
        // process-wide counter keeps concurrent calls distinct.
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        capability::require(&context, self.revision(), Required::Elicitation)?;
        let elicitation_id = format!(
            "url-elic-{}",
            NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let result = context
            .peer
            .create_elicitation(ElicitRequestParams::UrlElicitationParams {
                meta: None,
                message: "Complete the interaction in your browser".to_owned(),
                url: "https://mcp.example/interact".to_owned(),
                elicitation_id: elicitation_id.clone(),
            })
            .await
            .map_err(|error| {
                ErrorData::internal_error(
                    "elicitation/create failed",
                    Some(serde_json::json!({ "error": error.to_string() })),
                )
            })?;
        if result.action == rmcp::model::ElicitationAction::Accept {
            // Consent recorded: the out-of-band interaction "finishes" now,
            // and the spec's completion notification closes the loop.
            //
            // rmcp 3.x deleted the typed API for this notification —
            // `notifications/elicitation/complete` and the URL-mode
            // `elicitationId` are removed in `2026-07-28` (register 1.5d Minor
            // #11) — but they are still part of `2025-11-25`, the revision
            // this server implements, and rmcp still lists `V_2025_11_25` as
            // supported. Dropping the capability would let the SDK's forward
            // migration silently shrink our protocol surface, so it is sent
            // through the generic seam instead. The wire shape is pinned
            // byte-for-byte against 1.7.0's typed emission by
            // `url_elicitation_completion_matches_the_1_7_wire_shape` in
            // tests/roundtrip.rs: method `notifications/elicitation/complete`,
            // params `{"elicitationId": "<id>"}` (rmcp 1.7.0 serialized the
            // param struct `rename_all = "camelCase"`). Register 3.16.
            context
                .peer
                .send_notification(rmcp::model::ServerNotification::CustomNotification(
                    rmcp::model::CustomNotification::new(
                        "notifications/elicitation/complete",
                        Some(serde_json::json!({ "elicitationId": elicitation_id.clone() })),
                    ),
                ))
                .await
                .map_err(|error| {
                    ErrorData::internal_error(
                        "notifications/elicitation/complete failed",
                        Some(serde_json::json!({ "error": error.to_string() })),
                    )
                })?;
        }
        let action = serde_json::to_value(result.action)
            .ok()
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| "unknown".to_owned());
        Ok(CallToolResult::success(vec![ContentBlock::text(format!(
            "URL elicitation {elicitation_id}: action={action}"
        ))]))
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
fn single_select_variants() -> BTreeMap<String, PrimitiveSchemaDefinition> {
    {
        let mut properties = BTreeMap::new();
        properties.insert(
            "untitledSingle".to_owned(),
            PrimitiveSchemaDefinition::Enum(
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
            PrimitiveSchemaDefinition::Enum(EnumSchema::Single(SingleSelectEnumSchema::Titled(
                TitledSingleSelectEnumSchema::new(vec![
                    ConstTitle::new("value1", "First Option"),
                    ConstTitle::new("value2", "Second Option"),
                    ConstTitle::new("value3", "Third Option"),
                ]),
            ))),
        );
        properties.insert(
            "legacyEnum".to_owned(),
            PrimitiveSchemaDefinition::Enum(EnumSchema::Legacy({
                // `#[non_exhaustive]` in rmcp 3.x. `new` takes the enum
                // values; `enum_names` is the field SEP-1330 exercises and
                // the one register 3.8 tracks, so it is set explicitly.
                let mut schema =
                    LegacyEnumSchema::new(vec!["opt1".into(), "opt2".into(), "opt3".into()]);
                schema.enum_names = Some(vec![
                    "Option One".into(),
                    "Option Two".into(),
                    "Option Three".into(),
                ]);
                schema
            })),
        );
        properties
    }
}

/// SEP-1330 variants 4–5: the multi-select shapes.
fn multi_select_variants() -> BTreeMap<String, PrimitiveSchemaDefinition> {
    {
        let mut properties = BTreeMap::new();
        properties.insert(
            "untitledMulti".to_owned(),
            PrimitiveSchemaDefinition::Enum(
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
            PrimitiveSchemaDefinition::Enum(EnumSchema::Multi(MultiSelectEnumSchema::Titled(
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

/// Shared elicitation flow: capability check, the revision's round trip,
/// result formatting (`<prefix>: action=…, content=…`).
async fn elicit(
    context: &RequestContext<RoleServer>,
    revision: ServedRevision,
    round: Round,
    message: String,
    schema: ElicitationSchema,
    prefix: &str,
) -> Result<CallToolResponse, ErrorData> {
    capability::require(context, revision, Required::Elicitation)?;
    let answer = match mrtr::ask(
        context,
        revision,
        round,
        Ask::Elicit(ElicitRequestParams::FormElicitationParams {
            meta: None,
            message,
            requested_schema: schema,
        }),
    )
    .await?
    {
        Interaction::Answered(answer) => answer,
        Interaction::Deferred(deferred) => return Ok(deferred.into()),
    };
    Ok(CallToolResult::success(vec![ContentBlock::text(elicited_text(prefix, &answer))]).into())
}

/// `<prefix>: action=…, content=…` from an elicitation result.
///
/// Reads the client's answer as JSON rather than as a typed `ElicitResult`,
/// because that is the only shape an MRTR retry has: `inputResponses` is a map
/// of opaque JSON. The legacy path serializes its typed result through the
/// same function, so the two eras cannot drift into different wording — which
/// they would if each formatted from its own type.
fn elicited_text(prefix: &str, answer: &serde_json::Value) -> String {
    let action = answer
        .get("action")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let content = answer
        .get("content")
        .filter(|content| !content.is_null())
        .map_or_else(|| "null".to_owned(), ToString::to_string);
    format!("{prefix}: action={action}, content={content}")
}

/// The first text block of a sampling result, however it arrived.
fn sampled_text(answer: &serde_json::Value) -> String {
    answer
        .pointer("/content/text")
        .or_else(|| answer.pointer("/content/0/text"))
        .and_then(serde_json::Value::as_str)
        .map_or_else(|| "(non-text response)".to_owned(), ToOwned::to_owned)
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
mod tests;
