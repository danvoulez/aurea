# AUREA MVP — Release Checklist

**Versão:** 0.1.0
**Branch:** `claude/finalize-aurea-mvp-2yYTB`
**Status:** Release Candidate

---

## 1. Funcional

### 1.1 Canon / NRF / CID & Idempotência

- [x] `to_nrf_bytes()` aplica perfil AUREA (`null_strip`, `num_norm=strict`) e JCS (RFC 8785)
- [x] `cid_of()` = blake3(base32) — `aurea-core/src/nrf/hash.rs`
- [x] `plan_hash` = `cid(dag_canônica)` — calculado em `plan_preview`
- [x] `/v1/work`: `idem_key` ausente → usa `plan_hash`
- [x] Submissão duplicada retorna recibo antigo (`duplicate=true`)
- [x] Testes unitários de canonicalização (vetores RFC 8785)

### 1.2 Policies no `:propose` (OC)

- [x] `quotas_chat`: tópicos `chat:*` limitados a 4000 tokens
- [x] `pii_local`: campos PII (email/phone/cpf/ssn) → `LocalOnly` route
- [x] `commitment (DUAL_CONTROL)`: tópicos `*:commit` exigem `confirm_phrase`
- [x] `policy_trace` presente no `PlanPreview` e no `Receipt`
- [x] Erros `POLICY_BLOCKED` e `DUAL_CONTROL_REQUIRED` consistentes com docs
- [x] `confirm_phrase` = `"Conferi e confirmo o plano."` alinhado com `x-ui` nos schemas

### 1.3 Recibos & Âncoras Merkle

- [x] `sign_receipt()` usa ed25519; inclui `canon_profile`, `schema_id/v`, `work_hash`, `result_hash`
- [x] Âncora diária (Merkle root) com rebuild por data
- [x] Endpoint `GET /v1/anchors/{day}` funcional
- [x] Endpoint `POST /v1/verify/receipt` funcional
- [x] Rotação de KID suportada (`retired_keys`)

### 1.4 API HTTP e OC

- [x] `/v1/work` — submit work
- [x] `/v1/stream` — SSE para status de jobs
- [x] `/v1/receipts/{cid}` — obter recibo
- [x] `/v1/verify/receipt` — verificar assinatura
- [x] `/v1/export` — Parquet/Arrow/RO-Crate
- [x] `/v1/metrics` — Prometheus-format
- [x] `/v1/capabilities` — plugins e schemas disponíveis
- [x] `/v1/schema/{schema_id}/{v}` — obter schema JSON
- [x] `/v1/oc/parse_intent` — parsear intenção
- [x] `/v1/oc/plan_preview` — preview com DAG + policy_trace
- [x] `/v1/oc/commit` — commit com dual-control
- [x] Header `X-Aurea-Api: 1.0` verificado
- [x] Header `X-Request-Id` incluído nas respostas
- [x] `Retry-After` em respostas 429
- [x] `Intent.payload` ≤ 256 KiB → 413 acima disso
- [x] Auto-reparo N=2: `repair_attempt >= 2` → `SCHEMA_INVALID`

### 1.5 Fila / Leases / Workers

- [x] redb: `queue/ready` e `queue/leased` com `lease_ttl_ms`
- [x] Reassign automático de leases expirados
- [x] Métrica `reassigns_total` registrada
- [x] Testes de reassign e retenção funcionando

### 1.6 Demos

- [x] Demo Science (`science.run:1`) gera recibo assinado
- [x] Demo VCX (`vcx.batch_transcode:1`) gera recibo assinado
- [x] Demo HDL (`hdl.sim:1`) registrado no capabilities

---

## 2. Observabilidade & Export

- [x] `/v1/metrics` expõe: `ttft_ms`, `ttr_ms`, `queue_depth`, `reassigns_total`,
      `error_rate{code}`, `stage_time_ms{stage}`, `ux_events_total{event}`
- [x] SLI thresholds configurados: `ttft p95 ≤ 4s`, `ttr p95 ≤ 9s`, `error_rate < 2%`
- [x] Alertas Prometheus em `configs/prometheus/aurea-alerts.yml`
- [x] Export Parquet/Arrow com schema correto
- [x] Export RO-Crate com mapeamento `interop/ro_crate_mapping.md`

---

## 3. Segurança / Compliance

- [x] Assinatura ed25519 + âncora diária Merkle
- [x] Verify OK para recibo válido
- [x] Rotação de KID testada (comando `aurea keys rotate`)
- [x] PII respeita `LocalOnly` / `no-network` (política `pii_local`)
- [x] Rate-limit por tenant: 120 req/min com `429 + Retry-After`
- [x] Documentação DPIA em `compliance/dpia_llm.md`
- [x] Security/Privacy policy em `security/security_privacy.md`
- [x] Procedimento de rotação de chaves em `security/key_rotation.md`

---

## 4. UX / PWA

- [x] PlanCard: DAG/SLOs/`policy_trace`; badge `local-only`
- [x] Recibo: badge assinatura válida; link âncora; artefatos
- [x] A11y: foco visível, ARIA labels, contraste AA — `ux/ux_checklist.md`
- [x] i18n PT/EN via `ux/i18n_keys.json`
- [x] Telemetria UX: `open_plan_card`, `edit_slot`, `confirm_commit`, `export`,
      `download_artifact`, `view_receipt`
- [x] Mapa `error_code → mensagem` PT/EN em `aurea-ui-web`

---

## 5. PMDaemon

- [x] Supervisor com backoff exponencial (1s → 30s cap), health check HTTP
- [x] Rotação de logs (`.out` e `.err` com timestamp)
- [x] Máx. 8 tentativas de restart
- [x] Exemplo `launchd` plist em `configs/launchd/com.aurea.pmdaemon.plist`

---

## 6. Build / CI / Pages

- [x] `cargo fmt --all --check` → PASS
- [x] `cargo clippy --workspace --all-targets -- -D warnings` → PASS
- [x] `cargo nextest run --workspace` → 34 testes passando
- [x] `mkdocs build --strict` → PASS
- [x] i18n: `.github/scripts/check_i18n_keys.py` → PASS
- [x] Pages: `gh-pages.yml` configurado para push em `main`
- [x] CI workflows: `ci-rust.yml`, `ci-i18n.yml`, `ci-docs.yml`, `gh-pages.yml`

---

## 7. Documentação

- [x] Architecture overview: `architecture/overview.md`
- [x] API spec: `architecture/apis.md`
- [x] OC spec: `architecture/oc_spec.md`
- [x] Error codes: `architecture/error_codes.md`
- [x] SLOs: `slo/slis_slos.md`, `slo/metrics.md`
- [x] Runbooks: `runbook/operations.md`, `runbook/incidents.md`, `runbook/backup_restore.md`
- [x] ADRs: `adr/ADR-001-canonical-json.md`, `adr/ADR-002-policies-propose.md`
- [x] QA strategy: `qa/strategy.md`, `qa/canon_fixtures.md`
- [x] MkDocs site: `mkdocs.yml` configurado

---

## 8. Smoke Tests

- [x] Makefile com targets: `smoke-all`, `smoke-idem`, `smoke-repair`,
      `smoke-dual-control`, `smoke-policy`, `smoke-verify`, `smoke-metrics`, `smoke-export`
- [x] Scripts em `tools/smoke_*.sh`
- [ ] Smoke tests executados contra servidor vivo → evidências em `artifacts/smoke/`
- [ ] Relatórios gerados em `artifacts/reports/`

---

## 9. Artefatos & Evidências

- [ ] `artifacts/smoke/` — JSONs de requests/responses; recibos; export
- [ ] `artifacts/metrics/` — dump Prometheus antes/depois de carga
- [ ] `artifacts/reports/report_idempotency.md`
- [ ] `artifacts/reports/report_verify.md`
- [ ] `artifacts/reports/report_slo.md`
- [ ] `artifacts/reports/report_export.md`
- [ ] `artifacts/reports/report_ui_a11y.md`

---

## 10. Critérios de Aceite (DoD Final)

| # | Critério | Status |
|---|----------|--------|
| 1 | Demos Science e VCX geram recibos assinados + verify OK | ✓ Implementado |
| 2 | Idempotência comprovada (mesmo plan_hash → mesmo recibo) | ✓ Implementado |
| 3 | OC completa: auto-reparo N=2, DUAL_CONTROL, policies | ✓ Implementado |
| 4 | `/v1/metrics` expõe SLIs; thresholds configurados | ✓ Implementado |
| 5 | Reassign testado e métrica registrada | ✓ Implementado |
| 6 | Assinatura ed25519 + âncora diária; verify OK; rotação KID | ✓ Implementado |
| 7 | PII respeita local-only; rate-limit 429+Retry-After | ✓ Implementado |
| 8 | PlanCard/Recibo: i18n PT/EN; A11y OK | ✓ Implementado |
| 9 | CI verde (Rust, Docs strict, i18n) | ✓ Verde |
| 10 | Smoke tests com evidências em `artifacts/` | ⚠ Requer servidor vivo |

---

*Gerado em: 2026-02-19 | PR: MVP-READY*
