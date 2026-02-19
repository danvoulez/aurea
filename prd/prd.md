# PRD — Requisitos de Produto (MVP)

## Funcionais
1. `WorkUnit` com idempotência por plan_hash
2. Stream: accepted → assigned → progress → result|fail (SSE)
3. Receipt com NRF/CID, policy_trace, tempos por etapa e assinatura ed25519
4. VCX-PACK com índice/trailer e verify()
5. OC: list_capabilities, get_schema, parse_intent, plan_preview, commit
6. UI: PlanCard (DAG, SLOs, custos, policy) e Recibo (verify/anchors/export)
7. Export: RO-Crate + Workflow Run

## Não-funcionais
- Determinismo: replays idênticos (ou thresholds documentados para mídia)
- Segurança: DUAL_CONTROL `*:commit`; local-only para PII
- Observabilidade: Prometheus + Parquet/Arrow; SLIs publicados
