# Local development-conversation E2E

This is a Docker-only acceptance test using a synthetic development
conversation. It is a client equivalent to the Codex Desktop knowledge flow:
it ingests a bounded conversation artifact, retrieves cited evidence, compiles
a token-bounded context package, checks tenant isolation, and confirms that
retrieved content remains untrusted.

The fixture deliberately includes a prompt-injection attempt. Passing the test
means the text is returned only as data; it does not change authorization or
policy. Do not put real conversations, API keys, or customer data in this
directory.

Build the image, then run the isolated acceptance stack:

```bash
docker build -t hangar-server-operational-test .
docker compose -f hangar-ia-e2e/compose.yaml up --abort-on-container-exit --exit-code-from e2e
docker compose -f hangar-ia-e2e/compose.yaml down -v
```

The result prints citation count, context-token count, full-corpus byte count,
and the isolation/trust-boundary checks. It exercises the native API used by
the CLI and MCP adapter; configuring the same local server in Codex Desktop is
documented in [`docs/integrations.md`](../docs/integrations.md).

## Benchmark controlado de qualidade semântica

`semantic-benchmark-corpus.json` é um corpus sintético e versionado de 12
documentos e 12 consultas para decisão de lançamento; não substitua-o por
conteúdo de clientes. O avaliador mede a API HTTP canônica, logo também cobre o
caminho que CLI e MCP usam.

Execute uma vez por perfil, em servidores novos com volumes distintos. O
processo cria uma organização descartável por execução; não use um token de
bootstrap compartilhado com produção.

```bash
# servidor padrão, HANGAR_EMBEDDING_PROFILE=hashing-v1
HANGAR_URL=http://127.0.0.1:8080 HANGAR_BOOTSTRAP_TOKEN=... \
  python hangar-ia-e2e/semantic_benchmark.py run \
  --expected-profile hashing-v1 --deployment-profile solo \
  --artifact-manifest-sha256 not-applicable \
  --output tmp/evaluation/hashing-v1.json

# servidor com modelo previamente instalado e verificado, sem rede no runtime
HANGAR_URL=http://127.0.0.1:8080 HANGAR_BOOTSTRAP_TOKEN=... \
  python hangar-ia-e2e/semantic_benchmark.py run \
  --expected-profile local-multilingual-v1 --deployment-profile solo \
  --artifact-manifest-sha256 "$(sha256sum /var/lib/hangar/models/local-multilingual-v1/hangar-local-model-manifest.json | cut -d' ' -f1)" \
  --output tmp/evaluation/local-multilingual-v1.json

python hangar-ia-e2e/semantic_benchmark.py compare \
  --baseline tmp/evaluation/hashing-v1.json \
  --semantic tmp/evaluation/local-multilingual-v1.json \
  --output tmp/evaluation/hangar-v1-launch-report.md
```

O comparador mede BM25 com `hashing-v1`, recuperação semântica isolada e o
ranking híbrido final. Ele aplica os portões de `docs/evaluation.md` à busca
híbrida e encerra com código 2 se o lançamento for `NO-GO`. P95 é registrado,
mas não tem portão universal: ele só vira SLO após registrar o hardware e o
perfil de implantação.

Cada execução também valida memória curta e longa pela API: uma sessão privada
aceita observação e resumo, recusa outro principal, expira por TTL e não entra
na recuperação durável. A promoção cria uma memória `proposed` com proveniência
da sessão; ela só se torna consumível depois de `validated` e `published` por
um owner.
