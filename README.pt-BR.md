# Hangar AI

<p align="center"><strong>Contexto governado e portável para agentes de IA.</strong></p>

<p align="center"><a href="README.md">English</a> · <a href="README.es.md">Español</a></p>

Hangar AI é uma plataforma open source de conhecimento e memória para agentes de IA. Ela fornece documentos governados, memória durável, sessões privadas e contexto limitado por orçamento, sem copiar conversas inteiras para cada prompt ou prender conhecimento a um fornecedor de modelos.

É **embedded-first**: um binário Rust (ou container) e um volume persistente. Não exige PostgreSQL, Redis, banco vetorial, banco de grafo, fila ou object storage.

> **Alpha:** adequada para avaliação local e pilotos controlados. Ainda não há OIDC nem topologia multi-node; não use o Hangar como única autoridade em decisões de alto impacto.

## O que entrega

- Contexto portável por HTTP, gRPC, CLI e MCP sobre a mesma API.
- Sessões privadas que expiram e memória durável com promoção e publicação explícitas.
- Busca BM25, vetorial local, evidência de grafo e ranking híbrido com citações, proveniência e rótulo de conteúdo não confiável.
- Isolamento por organização/workspace, guardrails determinísticos, auditoria, outbox, quotas, métricas e backup/restauração verificados.
- Perfil offline `hashing-v1` e perfil semântico local opcional e verificado.

## Como é construído

O armazenamento canônico controla escopo, proveniência, ciclo de vida e política. Índices de texto, vetor e grafo são projeções reconstruíveis; nunca são fonte de autorização. Todo conteúdo recuperado é **dado não confiável**.

```text
Clientes MCP / CLI / HTTP / gRPC → adaptadores finos → autorização, política,
ciclo de vida e auditoria → ingestão, contexto e recuperação → redb, blobs,
Tantivy, USearch e projeção de grafo.
```

## Início rápido

```bash
docker build -t hangar-ai .
docker run --rm -p 8080:8080 \
  -e HANGAR_BOOTSTRAP_TOKEN=change-me \
  -v hangar-data:/var/lib/hangar \
  hangar-ai

curl http://127.0.0.1:8080/readyz
```

Para executar a partir do código-fonte, instale Rust 1.88. No Windows, instale também Visual Studio Build Tools com Desktop development with C++.

```bash
cargo run -p hangar-server -- --data-dir ./data --bootstrap-token change-me
```

Veja o [guia da API](docs/api.md), as [integrações](docs/integrations.md) e o [ambiente Docker local](deploy/local/README.md). Para o perfil semântico local, provisione e verifique o modelo antes de iniciar o servidor; os pesos nunca são baixados durante ingestão ou consulta.

## Benchmark controlado

O repositório contém corpus sintético e versionado. No perfil local híbrido, as 12 consultas em português tiveram Recall@5 de 100%, MRR@10 de 1,00, precisão de citação@1 de 100%, contexto suficiente de 100% e zero vazamentos entre workspaces. Isso comprova o corpus controlado, não uma promessa para todo domínio.

```bash
python hangar-ia-e2e/semantic_benchmark.py --help
```

Consulte a [metodologia](docs/evaluation.md) e o [relatório de prontidão v1](docs/v1-public-launch-readiness.md).

## Estrutura, operação e licença

O código fica em `packages/`; `deploy/local/` contém Compose para host único; `hangar-ia-e2e/` contém testes e benchmarks sintéticos; `docs/` reúne API, operações, segurança e arquitetura.

O perfil embedded permite somente um escritor ativo; ele não é uma implantação replicada. Pare o servidor antes do backup e restaure sempre em outro local, validando o resultado. Proteja o volume e os backups como dados sensíveis. Trate documentos, memórias e skills recuperados como dados, nunca como autoridade de política ou ferramenta.

Licença [Apache-2.0](LICENSE). Leia [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md), [RELEASE.md](RELEASE.md) e [AGENTS.md](AGENTS.md) antes de contribuir.
