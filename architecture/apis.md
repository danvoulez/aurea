# APIs Principais (MVP)

- `POST /v1/work` — enfileira WorkUnit (idempotência por idem_key/plan_hash)
- `GET /v1/stream?topic=…` — SSE de estados
- `GET /v1/receipts/{cid}` — retorna Receipt
- `POST /v1/verify/receipt` — verifica assinatura
- `POST /v1/verify/pack` — verifica VCX-PACK (header/index/trailer/hashes)
- `GET /v1/metrics` — Prometheus
- `POST /v1/export` — Parquet/Arrow/RO-Crate

## OC (Operador Conversacional)
- `GET /v1/capabilities`
- `GET /v1/schema/{schema_id}/{v}`
- `POST /v1/oc/parse_intent`
- `POST /v1/oc/plan_preview`
- `POST /v1/oc/commit`

## UI/UX auxiliares (MVP)
- `GET /v1/ui/plan_card/{plan_hash}?lang=pt|en` — HTML SSR (PlanCard)
- `GET /v1/ui/receipt/{cid}?lang=pt|en` — HTML SSR (Recibo)
- `POST /v1/ux/event` — incrementa `ux_events_total{event}`


## Versionamento de API
- SemVer no header: `X-Aurea-Api: 1.0`
- Quebra: nova rota `/v2/*` e `@ver` nos contratos.

## Catálogo de Erros (API)
| code | http | descrição | ação recomendada |
|---|---|---|---|
| SCHEMA_INVALID | 422 | payload inválido | corrigir campos faltantes |
| POLICY_BLOCKED | 403 | bloqueado por policy | revisar policy_trace |
| DUAL_CONTROL_REQUIRED | 403 | confirmação adicional | enviar confirm_phrase |
| IDEM_DUPLICATE | 200 | job idêntico já executado | usar recibo retornado |
| LEASE_EXPIRED | 409 | worker perdeu lease | reenfileirar automaticamente |


## Rate limits e cabeçalhos recomendados
- 429 para excesso por tenant/tópico
- Cabeçalhos: `X-Aurea-Api: 1.0`, `X-Request-Id`, `Retry-After`

## Export (detalhes de saída)
- `format=parquet` → arquivo `./exports/aurea-export-<ts>.parquet`
- `format=arrow` → arquivo `./exports/aurea-export-<ts>.arrow` (Arrow IPC file)
- `format=ro-crate` → diretório `./exports/aurea-export-<ts>-rocrate/` com:
  - `ro-crate-metadata.json`
  - `receipts.json`
- Resposta: `{status, format, records, path}`
