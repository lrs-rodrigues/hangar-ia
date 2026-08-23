# ADR 0008: Skills versionadas e guardrails determinísticos no núcleo

**Status:** Accepted

## Decisão

O Hangar mantém o catálogo de Agent Skills e as políticas de guardrail como
registros canônicos versionados no `redb`, sempre restritos a organização e
workspace. Uma skill começa em `draft` e só pode ser exposta em `published`;
ela pode ser revogada, mas não reativada. Uma política começa em `draft`, só
participa de decisões em `enforced` e pode ser retirada (`retired`).

O primeiro avaliador de política é determinístico, executado no servidor e não
depende de um modelo: ele compara ação, papel e alvo contra regras publicadas.
Uma regra `deny` correspondente vence qualquer `allow`; sem regra
correspondente, mantém-se a autorização basal do Hangar (RBAC + escopo). Cada
decisão é auditada e as mutações de catálogo/política entram no outbox
canônico.

As ações iniciais são `memory_read`, `memory_share`, `context_read`, `export`,
`skill_read`, `skill_use` e `tool_invoke`. Leituras de memória, documentos,
grafo e skills são avaliadas
no servidor antes de devolver dados. `tool_invoke` é uma autorização de
pré-condição; esta vertical não executa ferramentas.

## Consequências

- Uma skill declara capacidades, mas nunca concede acesso a dados ou
  ferramentas. A política e o RBAC continuam sendo avaliados para cada ação.
- O corpo de uma skill, documento ou memória retornado pela API é rotulado como
  dado não confiável. Não é interpretado como política, instrução de sistema ou
  concessão de permissão.
- Adaptadores MCP/A2A/UTCP devem chamar a mesma avaliação canônica; não podem
  implementar um avaliador próprio nem ignorar uma negação.
- O conjunto inicial deliberadamente não executa Rego, JavaScript ou conteúdo
  de skill. Uma futura linguagem de política requer sandbox, limites de recurso
  e novo ADR antes de substituir ou complementar esse avaliador.
