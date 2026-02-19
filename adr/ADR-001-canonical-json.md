# ADR-001 — Canonical JSON & NRF/CID
Status: Accepted

- Decisão: bytes canônicos e `cid = blake3(nrf_bytes)`
- Consequência: recibos estáveis e idempotência por plano
- Vetores canônicos de teste: `qa/fixtures/canon/jcs_vectors_ok.json`, `qa/fixtures/canon/jcs_vectors_err.json`, `qa/fixtures/canon/profile_null_strip.json`, `qa/fixtures/canon/profile_num_norm.json`
