# Skills e guardrails

Esta vertical entrega o controle de catálogo e de decisão que fica entre uma
identidade autenticada e qualquer contexto portável. Ela não executa modelos ou
ferramentas: mantém a decisão governada no núcleo e deixa protocolos apenas
transportarem o contrato.

## Modelo de catálogo

Uma `AgentSkill` é uma versão imutável em intenção de uma habilidade descrita
por conteúdo, hash, autor, escopo e manifesto de capacidades. O manifesto pode
declarar nomes de ferramentas e ações de contexto, mas é somente metadado; não
concede acesso. A evolução é:

```text
draft → published → revoked
   └────────────────→ revoked
```

Um novo registro com o mesmo nome recebe a próxima versão no mesmo
organization/workspace. A revogação é terminal para preservar a trilha de
auditoria. Leitores recebem somente skills publicadas.

Uma `GuardrailPolicy` também é uma versão por nome e escopo:

```text
draft → enforced → retired
   └─────────────→ retired
```

Somente políticas `enforced` influenciam solicitações. Cada regra contém ID
único, ação, efeito, papéis opcionais e alvos opcionais. Lista de papéis ou
alvos vazia significa “qualquer”; um alvo pode ser `*`. A avaliação é estável:

1. autentica a chave e valida organização/workspace/RBAC;
2. seleciona apenas políticas reforçadas daquele workspace;
3. coleta regras que correspondem à ação, papel e alvo;
4. nega se houver qualquer `deny`; caso contrário permite, preservando o RBAC
   basal quando não houver correspondência.

O retorno contém os IDs de políticas e regras avaliadas, nunca o conteúdo de
memória, documento ou skill usado como suposta regra. Decisões permitidas e
negadas são auditadas; criações e mudanças de lifecycle também entram no outbox
canônico.

## Pontos de aplicação

| Ação | Ponto nativo protegido | Alvo inicial |
| --- | --- | --- |
| `memory_read` | leitura e busca de memória; leitura de sessão | `memory`, `working-session` |
| `memory_share` | proposta e revisão de compartilhamento de memória | `memory-share` |
| `context_read` | RAG de documentos e Graph-RAG | `documents`, `graph` |
| `export` | exportação de workspace autorizada | `workspace-export` |
| `skill_read` | catálogo e detalhe de skill publicada | `catalog`, UUID da skill |
| `skill_use` | pré-autorização de uso de skill | nome da skill |
| `tool_invoke` | endpoint de avaliação para chamadores/adaptadores | identificador de ferramenta |

Não existe endpoint de execução de ferramenta no servidor nesta fase. Um
adaptador que deseje invocar uma ferramenta deve fazer a pré-avaliação canônica
e respeitar uma negação; uma aprovação tampouco substitui a autorização do
provedor da ferramenta.

## Limites de confiança

Todo conteúdo recuperado recebe o tratamento de `untrusted_data`. Em especial,
um documento indexado ou o `content` de uma skill não pode criar regras,
modificar ACL, elevar papel nem provocar chamada de ferramenta. Apenas o fluxo
autenticado de criação/publicação de políticas altera o avaliador. Isso mantém
prompt injection e memory poisoning fora da superfície de autoridade.

## Limites deliberados

O avaliador inicial não executa Rego, JavaScript ou DSL arbitrária, não suporta
condições temporais/PII/classificação e não substitui OIDC. Esses recursos
exigem um novo contrato versionado, sandbox e limites de CPU/memória antes de
serem adicionados. O comportamento atual é suficiente para políticas
previsíveis, revisáveis e operáveis no perfil de um único container.
