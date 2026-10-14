// SPDX-License-Identifier: MIT
// Copyright 2026 Tom F. (https://github.com/tomtom215)

//! The surface beyond the suite's catalogue, through a real rmcp client at
//! `2025-11-25`: every listing's second page, reached by following
//! `nextCursor`, and what is on it — icons, annotations, `resource_link`
//! content, an audio prompt. The `2026-07-28` wire form of the same flow is
//! in `stateless_http.rs`.
//!
//! In-process over `tokio::io::duplex`, as `roundtrip.rs` is: the same codec
//! the stdio transport uses, no sockets.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use mcp_everything_server::EverythingServer;
use rmcp::ServiceExt as _;
use rmcp::model::{
    CallToolRequestParams, ContentBlock, ErrorCode, GetPromptRequestParams, PaginatedRequestParams,
};
use rmcp::service::{RoleClient, RunningService};

async fn connect() -> RunningService<RoleClient, ()> {
    let (server_io, client_io) = tokio::io::duplex(64 * 1024);
    tokio::spawn(async move {
        if let Ok(server) = EverythingServer::new().serve(server_io).await {
            let _ = server.waiting().await;
        }
    });
    ().serve(client_io).await.expect("client initialize")
}

fn at(cursor: &str) -> PaginatedRequestParams {
    PaginatedRequestParams::default().with_cursor(Some(cursor.to_owned()))
}

#[tokio::test]
async fn a_paging_client_reaches_every_second_page() {
    let client = connect().await;
    // rmcp's own paging helpers follow `nextCursor` until it is absent, so
    // these succeeding is the cursor round trip, done by the SDK.
    let tools = client.list_all_tools().await.expect("every tools page");
    let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_ref()).collect();
    for name in [
        "test_simple_text",
        "test_resource_link",
        "test_parameter_headers",
    ] {
        assert!(names.contains(&name), "{name} missing from {names:?}");
    }
    let prompts = client.list_all_prompts().await.expect("every prompts page");
    assert_eq!(prompts.len(), 5);
    let resources = client
        .list_all_resources()
        .await
        .expect("every resources page");
    assert_eq!(resources.len(), 3);
    let templates = client
        .list_all_resource_templates()
        .await
        .expect("every templates page");
    assert_eq!(templates.len(), 2);
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn a_one_page_client_sees_the_suites_catalogue_and_a_next_cursor() {
    let client = connect().await;
    let first = client.list_tools(None).await.expect("tools/list");
    assert!(
        first
            .tools
            .iter()
            .all(|tool| tool.name != "test_resource_link"),
        "the second page's tools stay off the first"
    );
    let cursor = first.next_cursor.expect("a next page is named");
    let second = client
        .list_tools(Some(at(&cursor)))
        .await
        .expect("page two");
    assert_eq!(second.next_cursor, None, "two pages, no more");
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn an_unissued_cursor_draws_invalid_params_at_2025_11_25() {
    // PAGE-003: "Invalid cursors SHOULD result in an error with code -32602
    // (Invalid params)." Enforced in the handler at this revision, where the
    // stateless envelope rules do not run.
    let client = connect().await;
    let outcome = client.list_prompts(Some(at("fabricated"))).await;
    let Err(rmcp::ServiceError::McpError(error)) = outcome else {
        panic!("expected an MCP error, got {outcome:?}");
    };
    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn the_server_names_itself_with_an_icon() {
    let client = connect().await;
    let info = client.peer_info().expect("initialize result");
    let icons = info
        .server_info
        .as_ref()
        .and_then(|server| server.icons.clone())
        .expect("server icons");
    assert!(icons[0].src.starts_with("data:image/png;base64,"));
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn the_resource_link_tool_links_a_resource_that_reads_back() {
    let client = connect().await;
    let result = client
        .call_tool(CallToolRequestParams::new("test_resource_link"))
        .await
        .expect("tools/call");
    let Some(ContentBlock::ResourceLink(link)) = result.content.get(1) else {
        panic!("second block must be a resource_link: {:?}", result.content);
    };
    assert!(link.annotations.is_some() && link.icons.is_some());
    let read = client
        .read_resource(rmcp::model::ReadResourceRequestParams::new(&link.uri))
        .await
        .expect("the linked resource reads");
    assert_eq!(read.contents.len(), 1);
    client.cancel().await.expect("clean shutdown");
}

#[tokio::test]
async fn the_audio_prompt_carries_base64_audio_with_its_mime_type() {
    let client = connect().await;
    let prompt = client
        .get_prompt(GetPromptRequestParams::new("test_prompt_with_audio"))
        .await
        .expect("prompts/get");
    let ContentBlock::Audio(audio) = &prompt.messages[0].content else {
        panic!("first message must be audio: {:?}", prompt.messages);
    };
    assert_eq!(audio.mime_type, "audio/wav");
    assert_eq!(audio.data, mcp_everything_server::fixtures::TINY_WAV_BASE64);
    assert!(audio.annotations.is_some());
    client.cancel().await.expect("clean shutdown");
}
