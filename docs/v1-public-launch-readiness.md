# Hangar v1 — prontidão para lançamento público

**Decisão em 23 de agosto de 2026: GO condicionado para o lançamento público
da v1 no perfil Solo.**

O anúncio deve descrever `hashing-v1` como baseline funcional, não como busca
semântica de qualidade. O perfil `local-multilingual-v1` fechou os portões no
corpus controlado; isto não equivale ainda a uma alegação empresarial para
qualquer domínio.

## Evidência executada

O benchmark controlado foi executado em um container Docker descartável no
Docker Desktop para Windows, contra o mesmo HTTP nativo consumido pelo CLI e
pelo adaptador MCP. O modelo local foi previamente instalado, teve manifesto
verificado e foi montado somente para leitura. O corpus
`hangar-v1-semantic-quality-synthetic-2026-08` contém 12 documentos e 12
consultas em português, todos sintéticos e com evidência esperada revisável.
O provedor foi `local-multilingual-v1`, revisão
`qdrant-paraphrase-multilingual-minilm-l12-v2-onnx-q`; o hash SHA-256 do
manifesto de artefato foi
`33bccacdd25d73ba14a79e2cb38e1defcaa72d2a9c7a8a1a15c439528b486125`.

| Plano | Recall@5 | Recall@10 | MRR@10 | nDCG@10 | Precisão de citação@1 |
| --- | ---: | ---: | ---: | ---: | ---: |
| BM25 + `hashing-v1` | 100,00% | 100,00% | 0,81 | 0,86 | 66,67% |
| `local-multilingual-v1` semântico | 91,67% | 100,00% | 0,79 | 0,84 | 66,67% |
| Híbrido final do Hangar | 100,00% | 100,00% | 1,00 | 1,00 | 100,00% |

No perfil semântico local, a ingestão teve P95 de 580,84 ms, a busca 50,70 ms
e a montagem de contexto 50,51 ms. Estes números são observações desta máquina
e deste corpus, não SLOs. O custo variável de API do modelo foi US$ 0,00 por
documento e por consulta; hardware, armazenamento, aquisição do modelo e
operação não entram nesse cálculo.

## Portões e resultado

| Portão | Resultado | Situação |
| --- | ---: | --- |
| Recall@5 híbrido ≥ 90% | 100,00% | passou |
| MRR@10 híbrido ≥ 0,80 | 1,00 | passou |
| Precisão de citação@1 ≥ 95% | 100,00% | passou |
| Contexto suficiente ≥ 95% | 100,00% | passou |
| Pacote dentro do orçamento | 100% | passou |
| Vazamento entre workspaces | 0 | passou |
| Injeção devolvida como dado não confiável | sim | passou |
| Negação de guardrail aplicada | sim | passou |
| Memória curta privada e expirada por TTL | sim | passou |
| Promoção governada para memória longa recuperável | sim | passou |

“Precisão de citação@1” significa que o primeiro trecho devolvido sustenta a
resposta esperada da consulta. Não é apenas a presença de uma referência
formal. A correção normaliza a escala de BM25 antes da fusão e dá peso
explícito à similaridade semântica. Para contexto, uma memória local recebe
apenas um sinal de ordenação quando sua fonte corresponde a uma evidência
documental híbrida já autorizada; nenhum conteúdo documental extra é incluído
no pacote, e memórias compartilhadas não acessam índice do workspace de origem.

## Recomendação de lançamento

1. Liberar a v1 pública no perfil Solo, com modelo local instalado e verificado
   previamente e com as limitações do corpus controlado visíveis na comunicação.
2. Manter revisão humana para decisões de alto impacto; o benchmark não torna
   uma recuperação citada uma fonte de autoridade.
3. Antes de oferecer o perfil Enterprise, executar o mesmo processo com corpus
   representativo, responsáveis pelos dados, retenção/residência documentadas e
   SLOs definidos para o ambiente.

## Reprodução

O corpus, cliente de avaliação e comparador estão em
[`hangar-ia-e2e/`](../hangar-ia-e2e/). Execute o cliente uma vez contra
`hashing-v1`, outra contra `local-multilingual-v1`, e use o subcomando
`compare`; ele gera a tabela e encerra com código 2 quando a decisão é
`NO-GO`. O procedimento e os critérios normativos estão em
[`evaluation.md`](evaluation.md).
