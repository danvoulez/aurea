# Arquitetura — Visão Geral

- Binário único (Axum + Leptos + redb + ed25519)
- Policies no `:propose` (pré-fila), idempotência por plano
- Leases + reassign; storage redb; métricas Prometheus
- Artefatos via VCX-PACK; export RO-Crate; âncoras Merkle diárias
