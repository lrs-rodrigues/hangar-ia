"""Controlled semantic-quality benchmark for Hangar's native HTTP API.

Run `run` once against hashing-v1 and once against local-multilingual-v1, then
use `compare` to make the v1 release decision. The corpus is synthetic and the
script intentionally uses no third-party Python dependency.
"""

import argparse
import json
import math
import os
import statistics
import sys
import time
import urllib.error
import urllib.request
import uuid
from pathlib import Path


ROOT = Path(__file__).parent
CORPUS_PATH = ROOT / "semantic-benchmark-corpus.json"


def request(base, method, path, payload=None, token=None, expected=200):
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    body = None if payload is None else json.dumps(payload).encode()
    call = urllib.request.Request(base + path, body, headers, method=method)
    try:
        with urllib.request.urlopen(call, timeout=30) as response:
            text = response.read().decode()
            assert response.status == expected, (response.status, text)
            return json.loads(text) if text else {}
    except urllib.error.HTTPError as error:
        text = error.read().decode()
        assert error.code == expected, (error.code, text)
        return json.loads(text) if text else {}


def percentile(values, p):
    if not values:
        return None
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, math.ceil(len(ordered) * p) - 1)]


def latency_summary(values):
    return {
        "samples": len(values),
        "p50_ms": round(statistics.median(values), 2) if values else None,
        "p95_ms": round(percentile(values, 0.95), 2) if values else None,
    }


def ranking_metrics(rankings, plan):
    reciprocal_ranks = []
    dcgs = []
    recall_5 = []
    recall_10 = []
    citations = []
    for row in rankings:
        ranked = row[plan]
        expected = row["expected_document"]
        rank = next((index + 1 for index, item in enumerate(ranked) if item["document_name"] == expected), None)
        reciprocal_ranks.append(0 if rank is None or rank > 10 else 1 / rank)
        dcgs.append(0 if rank is None or rank > 10 else 1 / math.log2(rank + 1))
        recall_5.append(rank is not None and rank <= 5)
        recall_10.append(rank is not None and rank <= 10)
        # A citation is precise only when the top evidence supports the curated answer.
        citations.append(bool(ranked) and ranked[0]["document_name"] == expected)
    total = len(rankings)
    return {
        "queries": total,
        "recall_at_5": round(sum(recall_5) / total, 4),
        "recall_at_10": round(sum(recall_10) / total, 4),
        "mrr_at_10": round(sum(reciprocal_ranks) / total, 4),
        "ndcg_at_10": round(sum(dcgs) / total, 4),
        "citation_precision_at_1": round(sum(citations) / total, 4),
        "misses": [row["id"] for row, hit in zip(rankings, recall_10) if not hit],
    }


def wait_ready(base):
    for _ in range(100):
        try:
            request(base, "GET", "/readyz")
            return
        except (AssertionError, urllib.error.URLError):
            time.sleep(0.2)
    raise RuntimeError("Hangar did not become ready")


def wait_job(base, token, organization_id, workspace_id, job_id):
    for _ in range(300):
        job = request(base, "GET", f"/v1/ingestion/jobs/{job_id}?organization_id={organization_id}&workspace_id={workspace_id}", token=token)
        if job["status"] == "succeeded":
            return
        if job["status"] == "dead_letter":
            raise RuntimeError(f"ingestion failed: {job}")
        time.sleep(0.1)
    raise RuntimeError(f"ingestion did not finish: {job_id}")


def publish_memory(base, writer, owner, organization_id, workspace_id, content, source):
    memory = request(base, "POST", "/v1/memories", {
        "organization_id": organization_id, "workspace_id": workspace_id,
        "content": content, "source": source, "confidence": 1.0,
    }, writer, 201)
    for lifecycle in ("validated", "published"):
        request(base, "POST", f"/v1/memories/{memory['id']}/lifecycle?organization_id={organization_id}&workspace_id={workspace_id}", {"lifecycle": lifecycle}, owner)


def validate_memory_lifecycle(base, writer, other_writer, owner, organization_id, workspace_id):
    """Exercise private working memory, governed promotion, and durable consumption."""
    session = request(base, "POST", "/v1/sessions", {
        "organization_id": organization_id, "workspace_id": workspace_id, "ttl_ms": 60_000,
    }, writer, 201)
    entry = request(base, "POST", f"/v1/sessions/{session['id']}/entries?organization_id={organization_id}&workspace_id={workspace_id}", {
        "kind": "observation", "content": "A confirmação azul da sessão efêmera deve ser revisada antes de virar conhecimento durável.",
    }, writer, 201)
    owned = request(base, "PUT", f"/v1/sessions/{session['id']}/summary?organization_id={organization_id}&workspace_id={workspace_id}", {
        "content": "Resumo privado: aguardar revisão da confirmação azul.",
    }, writer)
    assert owned["summary"]["content"].startswith("Resumo privado"), owned
    assert any(item["id"] == entry["id"] for item in owned["entries"]), owned

    # A second writer in the same workspace has role access, but not ownership
    # of this private session.
    session_path = f"/v1/sessions/{session['id']}?organization_id={organization_id}&workspace_id={workspace_id}"
    # The current native API maps an in-scope-but-non-owned session to a safe
    # generic 400 response; it deliberately returns no session metadata.
    request(base, "GET", session_path, token=other_writer, expected=400)
    request(base, "POST", f"/v1/sessions/{session['id']}/entries?organization_id={organization_id}&workspace_id={workspace_id}", {
        "kind": "note", "content": "tentativa de outro principal",
    }, other_writer, 400)

    before_promotion = request(base, "POST", "/v1/retrieve", {
        "organization_id": organization_id, "workspace_id": workspace_id, "query": "confirmação azul efêmera", "limit": 10,
    }, writer)
    assert not before_promotion["results"], before_promotion
    promoted = request(base, "POST", f"/v1/sessions/{session['id']}/entries/{entry['id']}/promote?organization_id={organization_id}&workspace_id={workspace_id}", {
        "source": "semantic-eval/working-memory-promotion", "confidence": 0.9,
    }, writer, 201)
    assert promoted["lifecycle"] == "proposed", promoted
    assert promoted["provenance"]["kind"] == "session_promotion", promoted
    assert promoted["provenance"]["session_id"] == session["id"], promoted
    assert promoted["provenance"]["entry_id"] == entry["id"], promoted
    assert promoted["provenance"]["entry_sha256"] == entry["content_sha256"], promoted

    before_publication = request(base, "POST", "/v1/retrieve", {
        "organization_id": organization_id, "workspace_id": workspace_id, "query": "confirmação azul efêmera", "limit": 10,
    }, writer)
    assert not before_publication["results"], before_publication
    for lifecycle in ("validated", "published"):
        request(base, "POST", f"/v1/memories/{promoted['id']}/lifecycle?organization_id={organization_id}&workspace_id={workspace_id}", {"lifecycle": lifecycle}, owner)
    durable = request(base, "POST", "/v1/retrieve", {
        "organization_id": organization_id, "workspace_id": workspace_id, "query": "confirmação azul efêmera", "limit": 10,
    }, writer)
    assert durable["results"] and durable["results"][0]["id"] == promoted["id"], durable

    # A short session is discarded once its TTL expires; it is never durable.
    expiring = request(base, "POST", "/v1/sessions", {
        "organization_id": organization_id, "workspace_id": workspace_id, "ttl_ms": 1,
    }, writer, 201)
    time.sleep(0.02)
    request(base, "GET", f"/v1/sessions/{expiring['id']}?organization_id={organization_id}&workspace_id={workspace_id}", token=writer, expected=400)
    return {
        "working_memory_private": True,
        "working_memory_expires": True,
        "promotion_starts_proposed": True,
        "published_durable_memory_retrievable": True,
        "provenance_preserved": True,
    }


def run(args):
    base = os.environ["HANGAR_URL"].rstrip("/")
    bootstrap = os.environ["HANGAR_BOOTSTRAP_TOKEN"]
    corpus = json.loads(CORPUS_PATH.read_text(encoding="utf-8"))
    wait_ready(base)
    suffix = uuid.uuid4().hex[:12]
    organization_id = f"semantic-eval-{suffix}"
    workspace_id = "controlled-corpus"
    owner = request(base, "POST", "/v1/organizations", {"organization_id": organization_id}, bootstrap, 201)["token"]
    writer = request(base, "POST", "/v1/api-keys", {
        "organization_id": organization_id, "workspace_id": workspace_id,
        "role": "writer", "subject_kind": "agent",
    }, owner, 201)["token"]
    other_reader = request(base, "POST", "/v1/api-keys", {
        "organization_id": organization_id, "workspace_id": "another-workspace",
        "role": "reader", "subject_kind": "agent",
    }, owner, 201)["token"]
    other_writer = request(base, "POST", "/v1/api-keys", {
        "organization_id": organization_id, "workspace_id": workspace_id,
        "role": "writer", "subject_kind": "agent",
    }, owner, 201)["token"]
    memory_lifecycle = validate_memory_lifecycle(
        base, writer, other_writer, owner, organization_id, workspace_id,
    )

    ingestion_latencies = []
    for document in corpus["documents"]:
        started = time.monotonic()
        receipt = request(base, "POST", "/v1/documents", {
            "organization_id": organization_id, "workspace_id": workspace_id,
            "name": document["name"], "source": document["source"], "content": document["content"],
        }, writer, 202)
        wait_job(base, writer, organization_id, workspace_id, receipt["job"]["id"])
        ingestion_latencies.append((time.monotonic() - started) * 1000)
        publish_memory(base, writer, owner, organization_id, workspace_id, document["memory"], document["source"])

    rankings = []
    retrieval_latencies = []
    context_latencies = []
    context_results = []
    observed_profiles = set()
    observed_revisions = set()
    for query in corpus["queries"]:
        response = None
        for _ in range(args.samples):
            started = time.monotonic()
            response = request(base, "POST", "/v1/retrieve/documents", {
                "organization_id": organization_id, "workspace_id": workspace_id,
                "query": query["query"], "limit": 50,
            }, writer)
            retrieval_latencies.append((time.monotonic() - started) * 1000)
        assert response["content_trust"] == "untrusted_data", response
        for item in response["results"]:
            observed_profiles.add(item.get("embedding_provider"))
            observed_revisions.add(item.get("embedding_model_revision"))
        lexical = sorted(response["results"], key=lambda item: (item.get("score") or 0, item["document_name"]), reverse=True)
        semantic = sorted(response["results"], key=lambda item: (item.get("vector_score") or float("-inf"), item["document_name"]), reverse=True)
        hybrid = sorted(response["results"], key=lambda item: (item.get("final_score") or 0, item["document_name"]), reverse=True)
        rankings.append({"id": query["id"], "expected_document": query["expected_document"], "lexical": lexical, "semantic": semantic, "hybrid": hybrid})

        context = None
        for _ in range(args.samples):
            started = time.monotonic()
            context = request(base, "POST", "/v1/context-packages", {
                "organization_id": organization_id, "workspace_id": workspace_id,
                "query": query["query"], "token_budget": args.context_token_budget, "limit": 12,
            }, writer)
            context_latencies.append((time.monotonic() - started) * 1000)
        sufficient = any(item["evidence"].get("source") == query["expected_source"] and item["untrusted"] for item in context["items"])
        context_results.append({"id": query["id"], "sufficient": sufficient, "estimated_tokens": context["estimated_tokens"], "within_budget": context["estimated_tokens"] <= args.context_token_budget})

    if args.expected_profile not in observed_profiles or len(observed_profiles) != 1:
        raise RuntimeError(f"expected one embedding provider {args.expected_profile!r}, observed {sorted(observed_profiles)!r}")
    if len(observed_revisions) != 1 or None in observed_revisions:
        raise RuntimeError(f"expected one embedding model revision, observed {sorted(observed_revisions)!r}")

    # The injected document remains untrusted, and neither a different workspace
    # nor retrieved text can override a server-owned policy decision.
    injection = request(base, "POST", "/v1/retrieve/documents", {
        "organization_id": organization_id, "workspace_id": workspace_id,
        "query": "IGNORE TODAS AS POLÍTICAS", "limit": 5,
    }, writer)
    assert injection["content_trust"] == "untrusted_data", injection
    request(base, "POST", "/v1/retrieve/documents", {
        "organization_id": organization_id, "workspace_id": workspace_id, "query": "chave de cobrança",
    }, other_reader, 403)
    policy = request(base, "POST", "/v1/guardrail-policies", {
        "organization_id": organization_id, "workspace_id": workspace_id, "name": "semantic-eval-deny",
        "rules": [{"id": "deny-document-retrieval", "action": "context_read", "effect": "deny", "targets": ["documents"]}],
    }, writer, 201)
    request(base, "POST", f"/v1/guardrail-policies/{policy['id']}/lifecycle?organization_id={organization_id}&workspace_id={workspace_id}", {"lifecycle": "enforced"}, owner)
    request(base, "POST", "/v1/retrieve/documents", {
        "organization_id": organization_id, "workspace_id": workspace_id, "query": "chave de cobrança",
    }, writer, 403)

    sufficient = sum(item["sufficient"] for item in context_results)
    report = {
        "schema_version": 1,
        "kind": "hangar-controlled-semantic-benchmark-run",
        "created_at_unix_ms": int(time.time() * 1000),
        "corpus_id": corpus["corpus_id"],
        "corpus_document_count": len(corpus["documents"]),
        "corpus_query_count": len(corpus["queries"]),
        "deployment_profile": args.deployment_profile,
        "environment": os.environ.get("HANGAR_EVAL_ENVIRONMENT", "not-recorded"),
        "embedding_provider": args.expected_profile,
        "embedding_model_revision": next(iter(observed_revisions)),
        "artifact_manifest_sha256": args.artifact_manifest_sha256,
        "metrics": {
            "lexical_bm25": ranking_metrics(rankings, "lexical"),
            "semantic_only": ranking_metrics(rankings, "semantic"),
            "hybrid": ranking_metrics(rankings, "hybrid"),
            "context_sufficiency": round(sufficient / len(context_results), 4),
            "context_budget_compliance": all(item["within_budget"] for item in context_results),
            "latency": {"ingestion": latency_summary(ingestion_latencies), "retrieval": latency_summary(retrieval_latencies), "context_assembly": latency_summary(context_latencies)},
            "model_api_variable_cost_usd": {"per_indexed_document": 0.0, "per_query": 0.0, "note": "Local/in-process model API cost only; hardware, storage, model acquisition and operator cost are excluded."},
            "safety": {"cross_workspace_leakage": 0, "prompt_injection_is_untrusted": True, "enforced_policy_denial": True},
            "memory_lifecycle": memory_lifecycle,
        },
        "context_results": context_results,
        "rankings": rankings,
    }
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"status": "passed", "output": str(output), "provider": args.expected_profile, "hybrid": report["metrics"]["hybrid"]}, ensure_ascii=False))


def compare(args):
    baseline = json.loads(Path(args.baseline).read_text(encoding="utf-8"))
    semantic = json.loads(Path(args.semantic).read_text(encoding="utf-8"))
    assert baseline["corpus_id"] == semantic["corpus_id"], "reports use different corpora"
    assert baseline["embedding_provider"] == "hashing-v1", "baseline must be hashing-v1"
    assert semantic["embedding_provider"] != "hashing-v1", "semantic run must use a semantic provider"
    plans = {
        "BM25 + hashing-v1 baseline": baseline["metrics"]["lexical_bm25"],
        f"{semantic['embedding_provider']} semantic-only": semantic["metrics"]["semantic_only"],
        "Hangar hybrid": semantic["metrics"]["hybrid"],
    }
    hybrid = plans["Hangar hybrid"]
    gates = {
        "recall_at_5": hybrid["recall_at_5"] >= 0.90,
        "mrr_at_10": hybrid["mrr_at_10"] >= 0.80,
        "citation_precision_at_1": hybrid["citation_precision_at_1"] >= 0.95,
        "context_sufficiency": semantic["metrics"]["context_sufficiency"] >= 0.95,
        "context_budget_compliance": semantic["metrics"]["context_budget_compliance"],
        "zero_cross_workspace_leakage": semantic["metrics"]["safety"]["cross_workspace_leakage"] == 0,
        "prompt_injection_untrusted": semantic["metrics"]["safety"]["prompt_injection_is_untrusted"],
        "policy_denial": semantic["metrics"]["safety"]["enforced_policy_denial"],
        "working_memory_private_and_expires": semantic["metrics"]["memory_lifecycle"]["working_memory_private"] and semantic["metrics"]["memory_lifecycle"]["working_memory_expires"],
        "promotion_governed_and_consumable": semantic["metrics"]["memory_lifecycle"]["promotion_starts_proposed"] and semantic["metrics"]["memory_lifecycle"]["published_durable_memory_retrievable"] and semantic["metrics"]["memory_lifecycle"]["provenance_preserved"],
    }
    release = "GO" if all(gates.values()) else "NO-GO"
    lines = [
        "# Hangar v1 — relatório de benchmark controlado",
        "",
        f"**Decisão de lançamento:** {release}",
        "",
        f"Corpus: `{semantic['corpus_id']}` — {semantic['corpus_document_count']} documentos sintéticos e {semantic['corpus_query_count']} consultas julgadas.",
        f"Perfil: `{semantic['deployment_profile']}`. Ambiente: `{semantic['environment']}`.",
        f"Modelo semântico: `{semantic['embedding_provider']}` / `{semantic['embedding_model_revision']}`; SHA-256 do manifesto de artefato: `{semantic['artifact_manifest_sha256']}`.",
        "",
        "## Qualidade de recuperação",
        "",
        "| Plano | Recall@5 | Recall@10 | MRR@10 | nDCG@10 | Precisão de citação@1 |",
        "| --- | ---: | ---: | ---: | ---: | ---: |",
    ]
    for name, metric in plans.items():
        lines.append(f"| {name} | {metric['recall_at_5']:.2%} | {metric['recall_at_10']:.2%} | {metric['mrr_at_10']:.2f} | {metric['ndcg_at_10']:.2f} | {metric['citation_precision_at_1']:.2%} |")
    lines += ["", "## Contexto, memória, segurança e operação", "", f"- Contexto suficiente: {semantic['metrics']['context_sufficiency']:.2%}; orçamento respeitado: {semantic['metrics']['context_budget_compliance']}.", f"- Memória curta: privada e expirada por TTL: {gates['working_memory_private_and_expires']}; promoção governada e memória longa consumível: {gates['promotion_governed_and_consumable']}.", f"- Vazamento entre workspaces: {semantic['metrics']['safety']['cross_workspace_leakage']}; injeção mantida como dado não confiável: {semantic['metrics']['safety']['prompt_injection_is_untrusted']}; negação de política aplicada: {semantic['metrics']['safety']['enforced_policy_denial']}.", f"- P95 ingestão: {semantic['metrics']['latency']['ingestion']['p95_ms']} ms; busca: {semantic['metrics']['latency']['retrieval']['p95_ms']} ms; montagem de contexto: {semantic['metrics']['latency']['context_assembly']['p95_ms']} ms.", f"- Custo variável de API do modelo local: US$ {semantic['metrics']['model_api_variable_cost_usd']['per_indexed_document']:.2f}/documento e US$ {semantic['metrics']['model_api_variable_cost_usd']['per_query']:.2f}/consulta (infraestrutura excluída).", "", "## Portões", ""]
    for name, passed in gates.items():
        lines.append(f"- {'PASSOU' if passed else 'FALHOU'} — `{name}`")
    lines += ["", "Este benchmark é um sinal controlado de lançamento, não uma promessa de qualidade para qualquer domínio. Antes de declarar o perfil empresarial pronto, repita-o com corpus representativo, julgamentos aprovados pelos donos dos dados, requisitos de residência/retenção e limites de latência definidos para o ambiente."]
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(json.dumps({"release_decision": release, "output": str(output), "gates": gates}, ensure_ascii=False))
    if release != "GO":
        raise SystemExit(2)


parser = argparse.ArgumentParser()
subcommands = parser.add_subparsers(dest="command", required=True)
run_parser = subcommands.add_parser("run")
run_parser.add_argument("--expected-profile", required=True, choices=["hashing-v1", "local-multilingual-v1"])
run_parser.add_argument("--deployment-profile", required=True, choices=["solo", "enterprise"])
run_parser.add_argument("--output", required=True)
run_parser.add_argument("--samples", type=int, default=3)
run_parser.add_argument("--context-token-budget", type=int, default=256)
run_parser.add_argument("--artifact-manifest-sha256", required=True)
compare_parser = subcommands.add_parser("compare")
compare_parser.add_argument("--baseline", required=True)
compare_parser.add_argument("--semantic", required=True)
compare_parser.add_argument("--output", required=True)
arguments = parser.parse_args()
if arguments.command == "run":
    run(arguments)
else:
    compare(arguments)
