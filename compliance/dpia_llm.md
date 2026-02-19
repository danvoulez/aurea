# DPIA — LLM & Conversacional (MVP)

**Base legal**: execução de contrato e legítimo interesse, limitado ao escopo do tenant.  
**Dados tratados**: metadados de planos (PlanPreview, policy_trace), recibos (hashes/CIDs), artefatos (VCX-PACK).  
**Minimização**: `x-llm.redact` em schemas; `local-only` e `no-network` para PII.  
**Conservação**: retenção configurável por domínio; GC com verificação amostral.  
**Direitos**: acesso/retificação/eliminação conforme políticas internas do tenant.  
**Controles**: assinatura ed25519; âncoras Merkle; idempotência por plan_hash; auditoria via receipts.  
**Contato**: Owner/PM do tenant.
