# SPDX-License-Identifier: MIT
# Copyright 2026 Tom F. (https://github.com/tomtom215)

"""Reads back what the OpenTelemetry Collector exported for CI's `otlp` job.

Usage: python3 -I check.py <the collector's file exporter output>

The collector (conformance/otlp/collector.yaml) received the spans
`mcp-conformance watch --otlp` sent while replaying
corpus/violations/tool-006-name-charset.jsonl plus one `ping` exchange (the
`spans_are_accepted_by_a_real_collector` test), and wrote them back out as
OTLP JSON. This checks that what came through the collector carries the MCP
semantic conventions' span names and attributes, the CLIENT span kind, and the
finding as a span event — that is, that the collector parsed every field, not
merely answered 200. Exits non-zero, naming what is wrong.
"""

import json
import re
import sys


def attributes(item):
    """An OTLP attribute list as a dict of key to (type, value)."""
    found = {}
    for attribute in item.get("attributes", []):
        ((kind, value),) = attribute["value"].items()
        found[attribute["key"]] = (kind, value)
    return found


def main(path):
    problems = []
    spans = []
    resources = []
    with open(path, encoding="utf-8") as lines:
        for line in lines:
            if not line.strip():
                continue
            for resource_spans in json.loads(line)["resourceSpans"]:
                resources.append(attributes(resource_spans["resource"]))
                for scope_spans in resource_spans["scopeSpans"]:
                    spans.extend(scope_spans["spans"])

    def check(condition, problem):
        if not condition:
            problems.append(problem)

    for resource in resources:
        check(
            resource.get("service.name") == ("stringValue", "mcp-conformance"),
            f"resource service.name: {resource}",
        )
    names = sorted(span["name"] for span in spans)
    check(
        names == sorted(["initialize", "notifications/initialized", "tools/list", "ping"]),
        f"span names: {names}",
    )
    check(len({span["traceId"] for span in spans}) == 1, "one trace id for the session")
    check(len({span["spanId"] for span in spans}) == len(spans), "distinct span ids")
    for span in spans:
        name = span["name"]
        check(re.fullmatch(r"[0-9a-f]{32}", span["traceId"]) is not None, f"{name}: traceId")
        check(re.fullmatch(r"[0-9a-f]{16}", span["spanId"]) is not None, f"{name}: spanId")
        check(span.get("kind") == 3, f"{name}: kind {span.get('kind')}, not SPAN_KIND_CLIENT (3)")
        check(
            int(span["endTimeUnixNano"]) >= int(span["startTimeUnixNano"]) > 0,
            f"{name}: times",
        )
        found = attributes(span)
        check(found.get("mcp.method.name") == ("stringValue", name), f"{name}: mcp.method.name")
        check(found.get("network.transport") == ("stringValue", "pipe"), f"{name}: network.transport")
        check(
            found.get("mcp.protocol.version") == ("stringValue", "2025-11-25"),
            f"{name}: mcp.protocol.version",
        )
        if name.startswith("notifications/"):
            check("jsonrpc.request.id" not in found, f"{name}: a notification has no request id")
        else:
            check(found.get("jsonrpc.request.id", ("", ""))[0] == "stringValue", f"{name}: jsonrpc.request.id")
        if name != "tools/list":
            check(not span.get("events"), f"{name}: unexpected events")

    listing = [span for span in spans if span["name"] == "tools/list"]
    events = listing[0].get("events", []) if listing else []
    findings = {}
    for event in events:
        check(event["name"] == "mcp_conformance.finding", f"event name {event['name']}")
        found = attributes(event)
        requirement = found.get("mcp_conformance.requirement.id", ("", ""))[1]
        findings[requirement] = found
    check(sorted(findings) == ["TOOL-006", "TOOL-007"], f"findings on tools/list: {sorted(findings)}")
    for requirement, found in findings.items():
        check(found.get("mcp_conformance.outcome") == ("stringValue", "warn"), f"{requirement}: outcome")
        check(
            found.get("mcp_conformance.requirement.level", ("", ""))[1].startswith("SHOULD"),
            f"{requirement}: level",
        )
        check(
            found.get("mcp_conformance.spec.url", ("", ""))[1].startswith(
                "https://modelcontextprotocol.io/specification/2025-11-25/server/tools"
            ),
            f"{requirement}: spec URL",
        )
        check(found.get("mcp_conformance.trace.seq") == ("intValue", "5"), f"{requirement}: seq")

    if problems:
        for problem in problems:
            print(f"check.py: {problem}", file=sys.stderr)
        return 1
    print(f"check.py: {len(spans)} spans and {len(findings)} findings came through the collector as sent")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
