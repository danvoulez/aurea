# Segurança & Privacidade

- Assinaturas ed25519; `kid` rotativo; `/v1/verify/receipt`
- PII/local-only com roteamento automático; `no-network` quando exigido
- DUAL_CONTROL com frase de confirmação e carimbo no `policy_trace`
- Retenção e GC de packs/recibos


## Modelo de Ameaças (resumo)
- Integridade de recibos: ed25519 + Merkle diário.
- Replay/duplicidade: idem_key(plan_hash).
- Exfiltração de PII: policy local-only + no-network.

## Gestão de Chaves
- Geração on-boot; `kid=YYYYMMDD-HHMM`.
- Rotação: semanal (staging), mensal (prod) ou a cada N recibos.
- Procedimento e rollback em `security/key_rotation.md`.


## STRIDE (resumo)
| Ameaça | Risco | Mitigação |
|---|---|---|
| Spoofing | Impersonação de worker | chaves/leases; rotas outbound; verify assinatura |
| Tampering | Alteração de recibo/pack | ed25519 + Merkle + verify() |
| Repudiation | Negar execução | recibos assinados, anchors diárias |
| Information Disclosure | Exfiltração/PII | policy local-only + no-network |
| Denial of Service | Saturação da fila | quotas, rate limit, reassign |
| Elevation of Privilege | Execução não autorizada | policies por tópico + DUAL_CONTROL |
