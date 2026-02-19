# Project Charter — AUREA

**Visão**: Plataforma 100% Rust para execução determinística com recibos assinados, artefatos portáveis (VCX-PACK) e UX conversacional (OC).  
**Problema**: Drift de ambiente, dados sensíveis e falta de trilha verificável.  
**Solução**: Bus tipado, policies antes da fila, recibos/âncoras, packs verificáveis e PlanCard.

**Métricas (12m)**: ≥3 domínios ativos; TTFT p95 ≤ 4s; TTR p95 ≤ 9s; ≥10 replays auditados/mês; ≥60% execuções com RO-Crate.  
**Escopo MVP**: PR-01..PR-10. **Fora**: QUIC/HSM/DOI externos/dashboards UI.
