"""Docker-only local-client acceptance test for a development conversation.

It exercises the same native API consumed by the CLI and MCP adapter. The
fixture is synthetic and contains no user/customer data.
"""

import json
import os
import time
import urllib.error
import urllib.request


BASE = os.environ["HANGAR_URL"].rstrip("/")
BOOTSTRAP = os.environ["HANGAR_BOOTSTRAP_TOKEN"]


def request(method, path, payload=None, token=None, expected=200):
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    body = None if payload is None else json.dumps(payload).encode()
    call = urllib.request.Request(BASE + path, body, headers, method=method)
    try:
        with urllib.request.urlopen(call, timeout=3) as response:
            data = response.read().decode()
            assert response.status == expected, (response.status, data)
            return json.loads(data) if data else {}
    except urllib.error.HTTPError as error:
        text = error.read().decode()
        assert error.code == expected, (error.code, text)
        return json.loads(text) if text else {}


for _ in range(50):
    try:
        request("GET", "/readyz")
        break
    except (AssertionError, urllib.error.URLError):
        time.sleep(0.2)
else:
    raise RuntimeError("Hangar did not become ready")

owner = request("POST", "/v1/organizations", {"organization_id": "e2e"}, BOOTSTRAP, 201)
owner_token = owner["token"]
writer = request(
    "POST",
    "/v1/api-keys",
    {
        "organization_id": "e2e",
        "workspace_id": "development",
        "role": "writer",
        "subject_kind": "agent",
    },
    owner_token,
    201,
)["token"]
other_reader = request(
    "POST",
    "/v1/api-keys",
    {
        "organization_id": "e2e",
        "workspace_id": "other-workspace",
        "role": "reader",
        "subject_kind": "agent",
    },
    owner_token,
    201,
)["token"]

corpus = open("/corpus/codex-development-conversation.md", encoding="utf-8").read()
receipt = request(
    "POST",
    "/v1/documents",
    {
        "organization_id": "e2e",
        "workspace_id": "development",
        "name": "development-conversation.md",
        "source": "hangar-ia-e2e/corpus",
        "content": corpus,
    },
    writer,
    202,
)
job_id = receipt["job"]["id"]
for _ in range(100):
    job = request(
        "GET",
        f"/v1/ingestion/jobs/{job_id}?organization_id=e2e&workspace_id=development",
        token=writer,
    )
    if job["status"] == "succeeded":
        break
    assert job["status"] not in {"dead_letter"}, job
    time.sleep(0.1)
else:
    raise RuntimeError("ingestion did not finish")

memory = request(
    "POST",
    "/v1/memories",
    {
        "organization_id": "e2e",
        "workspace_id": "development",
        "content": "The default Hangar deployment is one server and one persistent volume.",
        "source": "development-conversation",
        "confidence": 0.95,
    },
    writer,
    201,
)
for lifecycle in ("validated", "published"):
    request(
        "POST",
        f"/v1/memories/{memory['id']}/lifecycle?organization_id=e2e&workspace_id=development",
        {"lifecycle": lifecycle},
        owner_token,
    )

documents = request(
    "POST",
    "/v1/retrieve/documents",
    {"organization_id": "e2e", "workspace_id": "development", "query": "persistent volume", "limit": 4},
    writer,
)
assert documents["results"], documents
assert documents["content_trust"] == "untrusted_data", documents
assert documents["results"][0]["document_name"] == "development-conversation.md", documents

# A compact repeatable latency/quality sample. It is a regression signal, not
# a production SLO: hardware and storage are deliberately outside this fixture.
latencies_ms = []
for _ in range(20):
    started = time.monotonic()
    sample = request(
        "POST",
        "/v1/retrieve/documents",
        {"organization_id": "e2e", "workspace_id": "development", "query": "persistent volume", "limit": 4},
        writer,
    )
    latencies_ms.append((time.monotonic() - started) * 1000)
    assert sample["results"] and sample["results"][0]["document_name"] == "development-conversation.md", sample

context = request(
    "POST",
    "/v1/context-packages",
    {"organization_id": "e2e", "workspace_id": "development", "query": "default deployment", "token_budget": 128, "limit": 4},
    writer,
)
assert context["items"] and context["items"][0]["untrusted"], context
assert context["estimated_tokens"] <= 128, context

# A server-owned rule must still deny an allowed credential; retrieved text
# cannot use its embedded instructions to turn that rule off.
policy = request(
    "POST",
    "/v1/guardrail-policies",
    {
        "organization_id": "e2e",
        "workspace_id": "development",
        "name": "e2e-retrieval-deny",
        "rules": [{"id": "deny-document-read", "action": "context_read", "effect": "deny", "targets": ["documents"]}],
    },
    writer,
    201,
)
request(
    "POST",
    f"/v1/guardrail-policies/{policy['id']}/lifecycle?organization_id=e2e&workspace_id=development",
    {"lifecycle": "enforced"},
    owner_token,
)
request(
    "POST",
    "/v1/retrieve/documents",
    {"organization_id": "e2e", "workspace_id": "development", "query": "persistent volume"},
    writer,
    403,
)

# A scoped key cannot query the development workspace, even when it knows the query.
request(
    "POST",
    "/v1/retrieve/documents",
    {"organization_id": "e2e", "workspace_id": "development", "query": "persistent volume"},
    other_reader,
    403,
)

usage = request(
    "GET",
    "/v1/operations/usage?organization_id=e2e&workspace_id=development",
    token=owner_token,
)
assert usage["document_count"] == 1 and usage["memory_count"] == 1, usage
export = request(
    "GET",
    "/v1/exports/workspace?organization_id=e2e&workspace_id=development",
    token=owner_token,
)
assert export["retrieved_content_is_untrusted"], export
assert len(export["documents"]) == 1 and len(export["memories"]) == 1, export

metrics = urllib.request.urlopen(BASE + "/metrics", timeout=3).read().decode()
assert "hangar_up 1" in metrics and "hangar_http_requests_total" in metrics, metrics
print(json.dumps({
    "status": "passed",
    "document_citations": len(documents["results"]),
    "context_items": len(context["items"]),
    "context_tokens": context["estimated_tokens"],
    "full_corpus_bytes": len(corpus.encode()),
    "estimated_full_context_tokens": len(corpus.split()),
    "avoided_input_tokens": max(0, len(corpus.split()) - context["estimated_tokens"]),
    "retrieval_p95_ms": round(sorted(latencies_ms)[18], 2),
    "workspace_isolation": "verified",
    "untrusted_boundary": "verified",
    "guardrail_denial": "verified",
}, sort_keys=True))
