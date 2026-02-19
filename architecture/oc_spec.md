# OC — Especificação LLM-friendly (v1.1)

## Objetos
- Intent{schema_id:string, v:u32, payload:object}
- PlanPreview{
  dag:{nodes[],edges[]},
  plan_hash:string,        # blake3(dag_canon)
  policy_trace:[{rule,ok}],
  slos:{ttft_ms:u32,ttr_ms:u32},
  costs?:{estimate:string,currency?:string},
  warnings?:[string]
}
- CommitRequest{plan_hash:string, confirm_phrase?:string}

## Regras de Conversação
1) NL → parse_intent → Intent válido (Schema/DoR).
2) plan_preview(Intent) → PlanPreview (SEM executar).
3) Confirmação humana → commit(CommitRequest).

## Auto-reparo (N=2)
- Slots ausentes/ambíguos retornam `repair_request:{missing:[path],hints:[...]}`.
- O operador só pede CAMPOS faltantes; mantém contexto do restante.
- Após N=2, retornar `SCHEMA_INVALID`.

## Idempotência
- idem_key = plan_hash (se ausente).
- Repetir commit com mesmo plan_hash → HTTP 200 + recibo anterior.

## Policy no :propose
- Avaliar PII/local-only, janelas, quotas e DUAL_CONTROL.
- Se DUAL_CONTROL: exigir `confirm_phrase` no commit.

## Contratos p/ LLM (extensões)
- JSON Schema com `x-llm: {examples:[], defaults:{}, minmax:{}, redact:["pii.*"]}`
- `x-ui: {widget:"select|numeric|code", unit:"ms|MB|%"}`

## Códigos de erro (OC)
- SCHEMA_INVALID: campos faltantes/fora de domínio
- POLICY_BLOCKED: política negou (ver policy_trace)
- DUAL_CONTROL_REQUIRED: requer confirm_phrase
- PLAN_CONFLICT: plan_hash mudou entre preview e commit


## Limites
- Tamanho máximo de `Intent.payload`: 256 KiB (MVP). Excedentes: 413 (Payload Too Large).
