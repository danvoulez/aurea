# QA & Estratégia de Testes

- Unit: canon/hash, cid, validação de schema
- Integração: leases/reassign, assinatura/verify, OC propose/commit idempotente
- E2E: fala→PlanCard→confirmar→stream→Recibo
- Determinismo: KATs por domínio (ciência/HDL bit-a-bit; mídia com thresholds)
- Chaos (staging): latência/falha seedada; SLOs mantidos
- Canon JSON (baseline): rodar vetores em `qa/fixtures/canon/` para casos `ok`, `err`, `null_strip` e `num_norm`


## Usabilidade (guiado)
- Cenário 1: VCX: preencher slots faltantes (auto-reparo) → confirmar → verificar recibo
- Cenário 2: POLICY_BLOCKED → ajustar janela/local-only → confirmar
- Cenário 3: Repetir plano (idempotência) → confirmar recibo reaproveitado

## Determinismo
- Ciência/HDL: byte-a-byte
- Mídia: PSNR ≥ 45 dB OU SSIM ≥ 0.995 (registrar no recibo)
