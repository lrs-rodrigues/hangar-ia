# Hangar AI

<p align="center"><strong>Contexto gobernado y portable para agentes de IA.</strong></p>

<p align="center"><a href="README.md">English</a> · <a href="README.pt-BR.md">Português (Brasil)</a></p>

Hangar AI es una plataforma open source de conocimiento y memoria para agentes de IA. Ofrece documentos gobernados, memoria duradera, sesiones privadas y contexto con presupuesto explícito, sin copiar conversaciones enteras a cada prompt ni encerrar el conocimiento en un proveedor de modelos.

Es **embedded-first**: un binario Rust (o contenedor) y un volumen persistente. No requiere PostgreSQL, Redis, base de vectores, base de grafos, cola ni almacenamiento de objetos.

> **Alpha:** preparada para evaluación local y pilotos controlados. Aún no hay OIDC ni topología multi-node; no use Hangar como única autoridad para decisiones de alto impacto.

## Qué incluye

- Contexto portable por HTTP, gRPC, CLI y MCP sobre la misma API.
- Sesiones privadas que expiran y memoria duradera con promoción y publicación explícitas.
- Búsqueda BM25, vector local, evidencia de grafo y ranking híbrido con citas, procedencia y etiqueta de contenido no confiable.
- Aislamiento por organización/workspace, guardrails deterministas, auditoría, outbox, cuotas, métricas y backup/restauración verificados.
- Perfil offline `hashing-v1` y perfil semántico local opcional y verificado.

## Cómo está construido

El almacenamiento canónico controla alcance, procedencia, ciclo de vida y política. Los índices de texto, vector y grafo son proyecciones reconstruibles; nunca son fuente de autorización. Todo contenido recuperado es **dato no confiable**.

```text
Clientes MCP / CLI / HTTP / gRPC → adaptadores delgados → autorización,
política, ciclo de vida y auditoría → ingestión, contexto y recuperación →
redb, blobs de archivos, Tantivy, USearch y proyección de grafo.
```

## Inicio rápido

```bash
docker build -t hangar-ai .
docker run --rm -p 8080:8080 \
  -e HANGAR_BOOTSTRAP_TOKEN=change-me \
  -v hangar-data:/var/lib/hangar \
  hangar-ai

curl http://127.0.0.1:8080/readyz
```

Para ejecutar desde el código fuente, instale Rust 1.88. En Windows instale también Visual Studio Build Tools con Desktop development with C++.

```bash
cargo run -p hangar-server -- --data-dir ./data --bootstrap-token change-me
```

Consulte la [guía de API](docs/api.md), las [integraciones](docs/integrations.md) y el [entorno Docker local](deploy/local/README.md). Para el perfil semántico local, aprovisione y verifique el modelo antes de iniciar el servidor; los pesos nunca se descargan durante la ingestión o las consultas.

## Benchmark controlado

El repositorio incluye un corpus sintético y versionado. En el perfil híbrido local, las 12 consultas en portugués obtuvieron Recall@5 de 100%, MRR@10 de 1,00, precisión de cita@1 de 100%, contexto suficiente de 100% y cero fugas entre workspaces. Esto demuestra el corpus controlado, no una promesa para cualquier dominio.

```bash
python hangar-ia-e2e/semantic_benchmark.py --help
```

Consulte la [metodología](docs/evaluation.md) y el [informe de preparación v1](docs/v1-public-launch-readiness.md).

## Estructura, operación y licencia

El código está en `packages/`; `deploy/local/` contiene Compose para host único; `hangar-ia-e2e/` contiene pruebas y benchmarks sintéticos; `docs/` reúne API, operaciones, seguridad y arquitectura.

El perfil embedded permite un único escritor activo; no es una implantación replicada. Detenga el servidor antes del backup y restaure siempre en otra ubicación, verificando el resultado. Proteja el volumen y los backups como datos sensibles. Trate documentos, memorias y skills recuperados como datos, nunca como autoridad de política o herramienta.

Licencia [Apache-2.0](LICENSE). Lea [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md), [RELEASE.md](RELEASE.md) y [AGENTS.md](AGENTS.md) antes de contribuir.
