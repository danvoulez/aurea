ÁUREA — Projeto Final
=====================

**Plataforma 100% Rust para Ciência Reprodutível, VCX, Protótipos de Silício e Operação Crítica**

* * *

0) Sumário executivo
--------------------

ÁUREA é uma plataforma unificada, tipada e verificável para execução de trabalhos críticos com **determinismo**, **recibos fortes** e **artefatos portáveis**. Implementada integralmente em Rust, combina um **bus de jobs** com **políticas auditáveis**, **plugins nativos/WASI**, empacotamento universal **VCX-PACK** e **UI PWA**. O desenho minimiza componentes e round-trips: **um binário** principal serve API+UI, gerencia fila/leases, assina recibos e expõe métricas; workers rodam **in-proc** por padrão e **out-of-proc**/QUIC quando necessário.

* * *

1) Objetivos e resultados
-------------------------

*   **Determinismo e reprodutibilidade**: replay bit-a-bit via seeds, clocks fixos, canonical JSON e KATs.
*   **Auditabilidade forte**: recibos com NRF/CID, `policy_trace`, tempos por etapa e assinaturas ed25519.
*   **Tipagem ponta-a-ponta**: contratos Rust → JSON Schema → validação de payloads/results → UI auto-gerada.
*   **Operação crítica**: quotas rígidas, PII/local-only, janelas, DUAL\_CONTROL, âncoras Merkle.
*   **Portabilidade de artefatos**: VCX-PACK com manifesto/índices/hashes/verify.
*   **TCO baixo**: tudo Rust, sem SaaS de cola; um binário opera API, fila e observabilidade.

* * *

2) Escopo
---------

Domínios de primeira classe:

1.  **Ciência reprodutível** (WASI/bin, KATs).
2.  **VCX** (imagem/áudio/vídeo, AV1, tempos por etapa).
3.  **HDL** (geração Rust→HDL, simulação determinística, VCD).
4.  **Operadores semânticos** (Text→JSON schema-driven; RAG tipado).

* * *

3) Arquitetura (enxuta)
-----------------------

### 3.1 Componentes

*   **aurea-bin** (único binário)
    *   **API/Bus**: HTTP/3 (QUIC) + HTTP/1.1 fallback; SSE para stream.
    *   **UI PWA** (Axum + Leptos): forms e viewers gerados por schema.
    *   **Fila/Leases**: `redb` ACID; reassign automático.
    *   **Policies**: funções puras compiladas; `policy_trace` no recibo.
    *   **Assinaturas**: ed25519 (chaves geridas pelo serviço; `kid` rotativo).
    *   **Observabilidade**: Prometheus + exports Arrow/Parquet; âncoras Merkle diárias.
*   **Plugins** (in-proc por padrão; WASI/out-of-proc via QUIC quando requerido):
    *   `exp_runner`, `exp_ingest`, `vcx_worker`, `hdl_codegen`, `hdl_sim`, `op_llm_json`, `op_rag`, `notify`.

### 3.2 Fluxo de alto nível

1.  Cliente envia `WorkUnit<T>` → API valida schema/idempotência → **accepted**.
2.  Scheduler aplica policies → enfileira → adquire lease → despacha handler (plugin).
3.  Handler emite **progress** (tempos por etapa), produz artefatos (VCX-PACK), retorna **result**.
4.  Serviço gera **Receipt** (hashes, trace, tempos), **assina** e ancora (Merkle opcional).
5.  Cliente acompanha por stream e consulta recibos via `/v1/receipts/{cid}`.

* * *

4) Contratos de dados
---------------------

### 4.1 Tipos principais (Rust → JSON Schema)

*   `WorkUnit<T>`; `WorkResult<R>`; `Receipt`; `Metrics`; `Signature`; `PolicyEntry`; `ArtifactRef`.
*   **NRF/CID**: `cid = blake3(canonical_json(value))`.
*   **Idempotência**: `(tenant, topic, idem_key)`; se ausente, `idem_key = blake3(payload_canon)`.

### 4.2 Versionamento e interoperabilidade

*   `@type`, `@id (uuid)`, `@ver`, `@world`.
*   `schema_id`/`schema_v` gravados no recibo; **semver estrito**; Schemas publicados pelo serviço.

* * *

5) API e protocolo
------------------

### 5.1 HTTP/3 (QUIC) e SSE

*   `POST /v1/work` → enfileira `WorkUnit<T>`; responde `{status, receipt_cid}`.
*   `GET /v1/stream?topic=...&tenant=...&id=...` (SSE) → eventos `accepted|assigned|progress|done|error`.
*   `GET /v1/receipts/{cid}` → `Receipt`.
*   `GET /v1/schema/{schema_id}/{v}` → JSON Schema.
*   `POST /v1/verify/pack` → valida VCX-PACK (hashes/manifest/entries).
*   `GET /v1/metrics` (Prometheus).
*   `POST /v1/export?format=parquet|arrow` → dump incremental.

### 5.2 QUIC (workers/out-of-proc)

Frames (bidi stream):

*   `worker.hello{worker_id, capacity, capabilities[]}`
*   `job.assign{work_unit}` / `job.progress{stage, t_ms}` / `job.result{result}` / `job.fail{err}`
*   `job.heartbeat{lease_id}`

Leases:

*   Tabelas `ready(topic)`, `leased(topic)` com `lease_ttl_ms`. Reassign em expiração.

* * *

6) Policies (compile-time, auditáveis)
--------------------------------------

Trait:

```rust
pub trait Policy {
  fn evaluate(&self, work: &WorkUnit<serde_json::Value>) -> Decision;
}
pub struct Decision { pub route: Route, pub budgets: Quotas, pub trace: Vec<PolicyEntry> }
```

Exemplos:

*   `quotas_chat`: `topic.starts_with("chat:")` → `tokens<=4000`.
*   `pii_local`: `pii_detected(payload)` → `route(LocalOnly)`.
*   `commitment`: `topic.ends_with(":commit")` → `require(DUAL_CONTROL)`.

`policy_trace` sempre gravado em `Receipt`.

* * *

7) VCX-PACK (artefatos)
-----------------------

*   **Formato**: manifesto + índice + trailer; entradas com `path`, `sha`, `size`.
*   **CID do pack**: `blake3(manifest_json)`.
*   **Lifecycle integrado**: runtime expõe `pack_begin/pack_add/pack_close` e preenche `receipt.artifacts[]`.
*   **Verify**: endpoint/CLI valida offsets, hashes e consistência.

* * *

8) Determinismo e reprodutibilidade
-----------------------------------

*   Seeds fixos, TZ=UTC, relógios simulados quando necessário.
*   Canonical JSON p/ hashing e idempotência.
*   **KATs**: ingest/compare outputs com packs de referência.
*   Isolamento de rede (policies) e versões travadas de libs.

* * *

9) Observabilidade e forense
----------------------------

*   Métricas: `ttft_ms`, `ttr_ms`, `queue_depth`, `reassigns`, `error_rate`, `stage_time_ms{stage}`.
*   Exports Parquet/Arrow + DataFusion para análises.
*   **Âncoras Merkle diárias** dos recibos (root publicado em log/arquivo).

* * *

10) UI PWA (Axum + Leptos)
--------------------------

*   **Auto-forms** a partir de JSON Schema dos contratos.
*   Lista/Detalhe de Jobs e **📜 Recibo** com `policy_trace`, hashes, tempos, artefatos (download/verify).
*   **Replay**: botão que reenvia `WorkUnit` original (ou simula via VCX-PACK/seed).

* * *

11) Segurança e conformidade
----------------------------

*   Assinaturas **ed25519** on-by-default; rotação de chaves (`kid`).
*   Políticas para PII/local-only e DUAL\_CONTROL em tópicos sensíveis.
*   Logs rotacionados; controle de retenção; snapshots periódicos.

* * *

12) Roadmap (90 dias)
---------------------

### M1 — Fundação 1-binário (D0–D15)

*   `aurea serve` (HTTP/3+SSE), redb, fila/leases, idempotência por hash, schemas publicados, UI base.

### M2 — Recibos fortes + VCX (D16–D35)

*   Assinatura ed25519; tempos por etapa; VCX-PACK integrado + verify; UI “📜 Recibo”.

### M3 — Domínios (D36–D65)

*   `exp_runner`/`exp_ingest` (WASI/bin) + KATs;
*   `vcx_worker` (resize→AV1→pack) com tempos;
*   `hdl_codegen`/`hdl_sim` (VCD, asserts) e packs.

### M4 — Operação crítica e SRE (D66–D90)

*   Policies duras (PII/local-only, DUAL\_CONTROL, janelas);
*   Métricas Prometheus, exports Parquet/Arrow; caos/staging; âncoras Merkle.

* * *

13) Critérios de aceite
-----------------------

*   **Determinismo (Ciência/HDL)**: replay reproduz artefatos byte-a-byte (hashes idênticos).
*   **VCX**: pipelines com tempos por etapa no recibo; `verify()` OK do pack.
*   **Semânticos**: Text→JSON válido no 1º ou 2º reparo; schema registrado no recibo.
*   **Operação crítica**: SLOs (TTFT p95 ≤ 4s; TTR p95 ≤ 9s); reassign no lease expirado; DUAL\_CONTROL efetivo.
*   **UI**: PWA instalável; stream estável; “📜 Recibo” completo.
*   **Assinaturas**: todos recibos assinados; rotação de `kid` validável.

* * *

14) Riscos & mitigações
-----------------------

*   **AV1 CPU**: presets ajustáveis; kernels wgpu para filtros; paralelização por GOP.
*   **Determinismo de libs**: trave de versões; seeds; KATs regulares; modo “no network”.
*   **QUIC atrás de proxies**: fallback WS/443; keep-alive agressivo.
*   **Crescimento de estado**: retenção configurável; snapshots Parquet/Arrow; âncoras Merkle.

* * *

15) Implantação e operação
--------------------------

*   **Single binary**: `aurea serve --db ./aurea.redb --listen 0.0.0.0:8080`.
*   **Chaves**: diretório `keys/` com `kid` atual e históricos; rotação diária/semanal.
*   **Config**: TOML com policies, quotas, janelas e roteamento de tópicos.
*   **Backup**: `redb` + snapshots de recibos/artefatos; exports periódicos Parquet/Arrow.

* * *

16) Testes e validação
----------------------

*   Unit/integration para contratos, hashing canônico, idempotência e leases.
*   KATs por domínio (ciência, VCX, HDL) com packs fixos.
*   Fuzz/proptest para HDL sob `feature=verify`.
*   Chaos em staging (latência/falhas controladas, reassign validado).

* * *

17) Governança e licença
------------------------

*   **Apache-2.0** para núcleo e plugins oficiais.
*   Plugins privados por domínio podem viver fora (respeitando contrato).
*   **Semver estrito**; `schema_id/v` carimbado em recibos; **sem versões mágicas**.

* * *

18) Entregáveis
---------------

*   Binário `aurea` (serve/worker/pack).
*   Crates: `core`, `storage`, `runtime`, `artifacts/vcx_pack`, `plugins/*`.
*   UI PWA integrada.
*   Documentação de API/Schema, guias de operação e de compliance.
*   Conjunto de KATs e packs de referência.

* * *

19) Exemplo de contrato e handler
---------------------------------

```rust
// Contrato
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct VcxSpec { pub ops: Vec<VcxOp> }

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[serde(tag="op", rename_all="snake_case")]
pub enum VcxOp { Resize{w:u32,h:u32}, EncodeAv1{preset:u8} }

// Handler
#[work(topic="vcx:graph")]
fn vcx_graph(spec: VcxSpec, ctx: Ctx) -> anyhow::Result<VcxResult> {
    let mut pack = ctx.pack_begin("vcx-output")?;
    let t0 = ctx.stage_start("resize");
    let img = resize(spec, ctx.inputs())?;
    ctx.stage_end("resize", t0);

    let t1 = ctx.stage_start("encode_av1");
    let av1 = encode_av1(img, spec)?;
    ctx.stage_end("encode_av1", t1);

    pack.add("out/video.ivf", &av1)?;
    let pack_cid = ctx.pack_close(pack)?;
    Ok(VcxResult{ pack_cid })
}
```

* * *

20) Plano de adoção
-------------------

1.  **Lab piloto** com ciência reprodutível e VCX simples.
2.  Expandir para HDL com VCD/netlists e KATs.
3.  Adicionar operadores semânticos com schemas de domínio.
4.  Ativar policies duras e âncoras em produção.

* * *

### Pronto para construção

O esqueleto inicial já está disponível e compilável. Próxima ação recomendada: implementar **M1** (idempotência por hash, leases redb, UI forms por schema) no binário único e habilitar **assinatura de recibos**. Isso libera fluxos reais de ciência/VCX/HDL com rastreabilidade completa.

## Prompt:
Se o operador for um LLM que conversa com o humano e confirma ações, conseguimos por mais complexidade "dentro" pra ficar com ux humana mais livre?

## Response:
Thought for 5s

Sim — dá para “entuchar” bastante complexidade dentro do operador e manter a UX humana leve, desde que o LLM trabalhe sob **contratos e fases**. Abaixo está o padrão que recomendo para o ÁUREA.

Padrão: Operador Conversacional (OC)
------------------------------------

Um **plugin** que conversa em linguagem natural e, por baixo, produz/valida `WorkUnit<T>` e `WorkResult<R>` contra **schemas**. Ele faz: entender intenção, preencher slots, planejar, simular (dry-run), pedir confirmação e só então **commit**.

### Fases (de fora parece “um chat”, por dentro é uma esteira)

1.  **NL → Intenção + Slots**
    *   Extração para `ActionDraft{action_id, slots, constraints, deadline?}`
    *   Validação imediata via JSON Schema do domínio (reduz alucinação).
2.  **Planejamento**
    *   Gera um **DAG** de operações (ex.: VCX → resize→encode AV1→pack).
    *   Estima **custos/tempos** (quotas) e políticas aplicáveis (PII/local-only, DUAL\_CONTROL, janela).
3.  **Dry-run / Pré-visualização**
    *   Gera **diff/preview** do que vai acontecer: artefatos esperados, SLOs, custo estimado, impactos.
    *   Produz um `Receipt` **ephemeral** (não assinado) com `policy_trace` e orçamentos.
4.  **Confirmação**
    *   O OC pede um “OK” explícito com **frase de compromisso** (ex.: `confirm: "DUAL_CONTROL"` quando o tópico termina com `:commit`).
    *   Idempotência por **hash do plano** (`idem_key = blake3(dag_canon)`).
5.  **Commit**
    *   Emite `WorkUnit` definitivo para o tópico de execução (`vcx:graph`, `science:run`, `hdl:sim`…), assina recibo real, ancora (Merkle opcional).
    *   Stream único: `accepted → assigned → progress{…} → result|fail`.
6.  **Pós-execução**
    *   OC agrega o `Receipt` e links de artefatos/VCX-PACK numa mensagem final, oferecendo **Replay** e **Rollback** (se aplicável).

> Para o usuário é “fale o que quer → veja um cartão com o plano → confirme”. Sem formulariões; sem idas e vindas.

* * *

Como isso “esconde” complexidade (com segurança)
------------------------------------------------

*   **Schema-driven**: o OC só propõe ações que existem no **registry de capacidades** (schemas exportados pelos plugins).
*   **Slot-filling robusto**: o LLM tenta preencher campos faltantes; se não couber no schema, ele **auto-repara** até N iterações.
*   **Políticas antes da fila**: `pii_local` pode reescrever o plano (trocar worker remoto por local) antes de enfileirar.
*   **Dois estágios**: `:propose` (ephemeral, barato) → `:commit` (assinado).
*   **Quotas explícitas**: o cartão de confirmação sempre mostra `tokens/time/custos` e **janelas** (quando pode rodar).
*   **Idempotência por plano**: repetir “Ok, manda bala” não duplica execução.
*   **Audit trail**: todo raciocínio externo vira `policy_trace` e métricas; o raciocínio interno não precisa ser exposto.

* * *

Contratos mínimos (exemplos)
----------------------------

```rust
// Pedido em linguagem natural → rascunho de ação
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct ActionDraft {
  pub action_id: String,              // ex.: "vcx.batch_transcode"
  pub slots: serde_json::Value,       // já validado contra o schema do action_id
  pub constraints: Option<serde_json::Value>,
  pub deadline_utc: Option<String>,
}

// Plano/DAG proposto
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct PlanPreview {
  pub dag: serde_json::Value,         // DAG canônico (ordenado)
  pub quotas: Quotas,                  // tokens/time/compute
  pub policy_trace: Vec<PolicyEntry>,  // o que será exigido
  pub estimated_ttft_ms: u32,
  pub estimated_ttr_ms: u32
}

// Confirmação
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct CommitRequest {
  pub plan_hash: String,               // blake3(dag_canon)
  pub confirm_phrase: Option<String>,  // ex.: "DUAL_CONTROL"
}
```

* * *

UX no chat (uma timeline, zero atrito)
--------------------------------------

*   Mensagem do humano: “Converte esses 20 vídeos pra 1080p, manda link pro time A até amanhã.”
*   OC responde com um **card de plano**:
    *   “Vou: extrair 20 itens da pasta X → `resize(1920x1080)` → `encode_av1(preset 6)` → `pack` → `notify:teamA`.”
    *   SLOs e custos previstos, janelas/policies aplicadas.
    *   Botões: **Confirmar**, **Editar** (abre slots), **Agendar**.
*   Ao confirmar, o stream mostra progresso por etapa; no fim, aparece **📜 Recibo** (assinado) e **Baixar VCX-PACK**.

* * *

Exemplos por domínio
--------------------

### VCX

“Padroniza pra 1080p e AV1, manda pro time A.”  
→ OC resolve slots (preset, CRF, FPS), verifica PII (não se aplica), estima TTR, pede confirmação, executa `vcx:graph`, anexa pack e envia `notify`.

### Ciência reprodutível

“Reproduz o experimento X com seed 1337 e compara com referência.”  
→ OC monta `ExperimentSpec`, injeta seed, bloqueia rede se policy pedir, aciona `exp_runner`, depois `exp_ingest` (KAT). Entrega diff e recibo.

### HDL

“Gera HDL do módulo Y, roda sim por 10k ciclos e me dá VCD.”  
→ OC monta `hdl_codegen` + `hdl_sim`, fixa TZ/clock/seed, produz pack com VCD/netlist; confirmação inclui tempo/capacidade do worker.

* * *

Controles que mantêm tudo “livre, mas seguro”
---------------------------------------------

*   **Lista branca de ações** (capabilities do registry) — nada fora do contrato.
*   **Campos obrigatórios** sempre destacados no card; se o LLM não conseguir inferir, pede só o mínimo essencial.
*   **DUAL\_CONTROL** para tópicos `*:commit`.
*   **Local-only** força workers locais automaticamente, sem o humano precisar saber.
*   **Dry-run** econômico: nunca queima quota pesada antes do OK.
*   **Rollback/Replay** direto no recibo.

* * *

Como integrar no ÁUREA agora
----------------------------

1.  **Registrar capacidades**: cada plugin expõe `schema/action_id` via endpoint (`/v1/schema/index`).
2.  **Criar o OC** no `plugins/op_llm_json` com três traits: `Intent`, `Planner`, `Confirmer`.
3.  **Implementar `:propose` → `:commit`** como dois tópicos internos (o primeiro não assina; o segundo assina).
4.  **UI**: componente Leptos `PlanCard` (renderiza a partir do `PlanPreview`), com botões **Confirmar/Editar/Agendar**.
5.  **Políticas**: hooks do OC chamam `policy.evaluate()` antes de enfileirar (aplica rota/quotas/local-only).
6.  **Idempotência**: `idem_key = blake3(dag_canon)` no `CommitRequest`.

* * *

Ganhos práticos
---------------

*   **Menos idas e vindas**: o OC resolve slots, valida e só te incomoda para o “OK” (ou um campo crítico).
*   **Erros precocemente baratos**: problemas aparecem no dry-run (policies, quotas, prazos).
*   **Risco controlado**: nada roda “de verdade” sem confirmação, recibos são assinados e auditáveis.
*   **Mesma UX para tudo**: VCX, ciência e HDL se comportam como “conversas com cartões”.

Se quiser, eu já adapto o `op_llm_json` do esqueleto para incluir **Intent → PlanPreview → Commit** e te deixo um exemplo real de **VCX batch** com card de confirmação e idempotência por hash do plano.

## Prompt:
Esse é o plano! Formalizar deixando o interior tb LLM friendly

## Response:
Perfeito. Abaixo está a **Especificação V1 — ÁUREA com interior LLM-friendly**, formalizando interfaces, contratos e o Operador Conversacional (OC) para que a UX seja “fale→confirme→execute”, enquanto o interior continua 100% tipado, verificável e seguro.

1) Princípios LLM-friendly
==========================

*   **Tudo é Schema**: cada ação publica JSON Schema (+ exemplos) e um “tool spec” claro para LLMs.
*   **Dois estágios**: `:propose` (barato, ephemero, sem efeitos) → `:commit` (executa, assina, ancora).
*   **Idempotência por plano**: `idem_key = blake3(dag_canon)`. Repetir “OK” não duplica execução.
*   **Auto-reparo limitado**: OC tenta reparar JSON inválido até `N=2` iterações; depois pede 1 campo humano.
*   **Privacidade de raciocínio**: sem exigir cadeia de pensamento; OC retorna apenas dados estruturados + justificativas curtas.
*   **Políticas antes da fila**: DUAL\_CONTROL/PII/local-only aplicadas no `:propose`, com traço explicativo no preview.

2) Registro de Capacidades (Capability Registry)
================================================

Endpoint: `GET /v1/capabilities`

```json
{
  "version": "1.0",
  "capabilities": [
    {
      "action_id": "vcx.batch_transcode",
      "schema_id": "aurea.vcx.batch_transcode",
      "schema_v": 1,
      "summary": "Transcodifica lista de mídias p/ alvo padronizado.",
      "examples": ["Padroniza 1080p AV1 para pasta X e notifica time A."],
      "topics": { "commit": "vcx:graph", "propose": "vcx:propose" }
    }
  ]
}
```

3) Convenções de Schema (JSON Schema, canônico)
===============================================

*   **Campos padrão**: `@type`, `@ver`, `schema_id`, `schema_v`.
*   **Enums/tags**: `#[serde(tag="op", rename_all="snake_case")]` para variantes.
*   **Exemplos e hints**: incluir `x-ui` e `x-llm` (metadados amigáveis ao LLM).
*   **Canonização**: JSON canônico para hashing (ordem estável, sem floats não determinísticos).

Exemplo (resumo) — `aurea.vcx.batch_transcode@1`:

```json
{
  "$id": "aurea.vcx.batch_transcode#1",
  "type": "object",
  "required": ["inputs", "video", "notify"],
  "properties": {
    "inputs": { "type": "array", "items": { "type": "string", "format": "uri" }, "minItems": 1,
      "description": "URIs ou IDs internos de mídias de entrada" },
    "video": {
      "type": "object",
      "required": ["width","height","codec"],
      "properties": {
        "width": {"type":"integer","minimum":16}, "height":{"type":"integer","minimum":16},
        "codec": {"enum": ["av1"]}, "preset": {"type":"integer","minimum":0,"maximum":10},
        "crf": {"type":"integer","minimum":0,"maximum":63}, "fps": {"type":"number","minimum":1}
      },
      "x-llm": { "defaults": {"codec":"av1","preset":6,"crf":28,"fps":30} }
    },
    "notify": {
      "type": "object",
      "required": ["channel"],
      "properties": { "channel":{"type":"string"}, "message":{"type":"string"} }
    },
    "deadline_utc": {"type":"string","format":"date-time"}
  }
}
```

4) Operador Conversacional (OC) — Contratos
===========================================

4.1 Intenção & Slots
--------------------

```json
{
  "@type":"aurea/intent.v1",
  "action_id":"vcx.batch_transcode",
  "slots": { "...": "valores já validados contra o schema do action_id" },
  "constraints": { "window":"business_hours", "local_only":true },
  "deadline_utc":"2026-02-20T17:00:00Z"
}
```

4.2 Plano/Preview (ephemero)
----------------------------

```json
{
  "@type":"aurea/plan_preview.v1",
  "plan_hash":"b3_..._abcd",
  "dag": { "nodes":[...], "edges":[...] },      // canônico
  "quotas": { "tokens": 0, "time_ms": 540000 },
  "policy_trace": [ {"rule":"pii_local","ok":true}, {"rule":"window","ok":true} ],
  "estimated_ttft_ms": 800,
  "estimated_ttr_ms": 300000,
  "artifacts_preview":[{"kind":"pack","label":"vcx-output","est_size":"~4.2GB"}],
  "confirm_required": "DUAL_CONTROL"            // ou null
}
```

4.3 Confirmação → Commit
------------------------

```json
{
  "@type":"aurea/commit_request.v1",
  "plan_hash":"b3_..._abcd",
  "confirm_phrase":"DUAL_CONTROL",
  "idem_key":"b3_..._abcd",                      // = plan_hash
  "tenant":"acme"
}
```

5) Tópicos e Estados (FSM)
==========================

**Tópicos**

*   Proposição: `*:propose` (sem assinatura; guarda preview opcional com TTL curto).
*   Execução: `*:commit` → enfileira em `vcx:graph`, `science:run`, `hdl:sim`, etc.

**Estados (stream único)**  
`accepted → assigned → progress{stage,time_ms,meta?} → result{...} | fail{error_code, details}`

**Códigos de erro (curtos, operacionais)**

*   `SCHEMA_INVALID`, `SLOT_MISSING`, `POLICY_BLOCKED`, `LEASE_EXPIRED`, `WORKER_UNAVAILABLE`, `ARTIFACT_VERIFY_FAIL`, `TIME_BUDGET_EXCEEDED`

6) APIs (HTTP/3 + SSE)
======================

*   `GET /v1/capabilities` → registry
*   `GET /v1/schema/{schema_id}/{v}` → JSON Schema
*   `POST /v1/oc/parse_intent` → `Intent` (OC interpreta NL→slots válidos)
*   `POST /v1/oc/plan_preview` → `PlanPreview` (aplica policies/quotas)
*   `POST /v1/oc/commit` → `{status, receipt_cid}`
*   `GET /v1/stream?tenant=&topic=&id=` → SSE estados
*   `GET /v1/receipts/{cid}` → `Receipt` assinado
*   `POST /v1/verify/pack` → `{"ok":true,"issues":[]}`
*   `GET /v1/metrics` (Prometheus) | `POST /v1/export?format=parquet|arrow`

7) Tool Spec p/ LLMs (funções chamáveis)
========================================

7.1 list\_capabilities
----------------------

*   **input**: `{}`
*   **output**: `{"capabilities":[{action_id,schema_id,schema_v,summary}]}`

7.2 get\_schema
---------------

*   **input**: `{"schema_id":"aurea.vcx.batch_transcode","schema_v":1}`
*   **output**: `JSON Schema`

7.3 propose\_plan
-----------------

*   **input**: `ActionDraft` (intenção+slots)
*   **output**: `PlanPreview` ou erro `SCHEMA_INVALID` com `repair_hints[]`

7.4 commit\_plan
----------------

*   **input**: `CommitRequest`
*   **output**: `{status:"accepted", "receipt_cid":"rcpt-..."}`

7.5 replay\_from\_receipt
-------------------------

*   **input**: `{"receipt_cid":"rcpt-...","override?":{}}`
*   **output**: `{status, new_receipt_cid}`

> Esses “tools” são estáveis para function-calling/Actions de qualquer provedor.

8) Auto-reparo e limites
========================

*   **Loop de reparo**: até **2** tentativas:
    1.  Validar `slots` contra schema → se falhar, gerar `repair_patch` minimal (ex.: preencher `preset`, `crf`, normalizar `fps` inteiro).
    2.  Se ainda inválido, pedir **apenas** os campos faltantes numa “mensagem curta de solicitação” (texto curto + lista de campos).
*   **Campos perigosos**: bloqueados por policy (`local_only`, `dual_control`, `max_duration_ms`, `allowed_codecs`).

9) Políticas (LLM-aware)
========================

*   OC chama `policy.evaluate(Intent|Plan)` antes de enfileirar.
*   Exemplo de regras:
    *   `pii_local`: se `inputs` apontam para espaço PII, `route=LocalOnly` e `capabilities=["local_gpu","no_network"]`.
    *   `window`: `deadline_utc` fora da janela → **agenda** (OC sugere `schedule_at`).
    *   `commitment`: ações com efeitos externos exigem `confirm_phrase:"DUAL_CONTROL"`.

`policy_trace` sempre vem no `PlanPreview` e no `Receipt`.

10) Recibos e Assinaturas
=========================

*   **Receipt** inclui: `work_hash`, `result_hash`, `policy_trace[]`, `ttft_ms`, `ttr_ms`, `signatures[]`, `artifacts[] (pack_cid, entries[])`, `schema_id/v`.
*   **Assinatura**: ed25519 (`alg:"Ed25519"`, `key_id:"kid-2026-02-01"`, `sig:base64`).
*   **Âncoras Merkle**: raiz diária armazenada e exposta em `/v1/anchors/YYYY-MM-DD`.

11) Observabilidade (SRE)
=========================

*   Métricas:
    *   `aurea_ttft_ms{topic}`
    *   `aurea_ttr_ms{topic}`
    *   `aurea_queue_depth{topic}`
    *   `aurea_reassigns_total{topic}`
    *   `aurea_error_rate{topic,error_code}`
*   Export: `/v1/export?format=parquet&since=...` com `receipts`, `stages`, `artifacts`.

12) UX (chat → cartão → confirmar)
==================================

*   **Card do Plano** renderizado de `PlanPreview`:
    *   Resumo em 3 linhas, lista de etapas, **custos/SLOs**, `policy_trace`.
    *   Botões: **Confirmar**, **Editar campos** (renderizados via schema), **Agendar**.
*   Confirmou? Stream mostra `accepted/assigned/progress/result` + link **📜 Recibo** e **Baixar VCX-PACK**.

13) Exemplos rápidos
====================

13.1 Requisição natural → OC
----------------------------

Usuário: “Converte 20 vídeos da pasta /lab/raw para 1080p AV1 e avisa o time A. Entrega até amanhã.”

**Fluxo interno:**

1.  `list_capabilities` → encontra `vcx.batch_transcode`.
2.  `get_schema` → aplica defaults (`preset=6, crf=28, fps=30`).
3.  `propose_plan` → `PlanPreview{plan_hash=..., policy_trace=[window:ok, pii_local:false]}`.
4.  UI mostra card → usuário clica **Confirmar**.
5.  `commit_plan{plan_hash, confirm_phrase?}` → `{status:"accepted",receipt_cid:"rcpt-..."}`.
6.  Stream + Recibo assinado.

13.2 Falta um slot
------------------

Se `fps` omitido → OC tenta `30`. Se schema exigir inteiro e veio `29.97`, **auto-repara** para `30`. Ainda falhou? Pede só `fps`.

14) Roadmap LLM-friendly (incremental)
======================================

*   **Fase A**: publicar capabilities+schemas+examples; implementar `propose_plan/commit_plan`; UI Card.
*   **Fase B**: reparo automático + dif de planos (explicativo) + idempotência por plano.
*   **Fase C**: políticas duras (DUAL\_CONTROL/local-only/janelas) + anchors.
*   **Fase D**: agents externos (CLI/mobile) usando o mesmo tool spec.

* * *

Entregáveis desta especificação
-------------------------------

1.  Endpoints e modelos JSON acima implementados no `aurea serve`.
2.  Registry de capacidades com schemas versionados e exemplos.
3.  OC dentro de `plugins/op_llm_json` com fases: **Intent → Plan → Commit**.
4.  UI Leptos com **PlanCard** e formulários auto-gerados pelo schema.
5.  Validação, auto-reparo (N=2), idempotência por `plan_hash`, assinatura de recibos e métricas SRE.

Com isso, a “conversa” vira a interface — e a complexidade fica dentro, contratualizada e auditável, sem atrito para quem usa. Quer que eu já atualize o esqueleto para incluir `capabilities`, `oc/*` (endpoints), `PlanCard` e um exemplo completo de `vcx.batch_transcode`?

## Prompt:
o plano completo são as 2 mensagens últimas né?

## Response:
Sim. O plano completo está nas **duas últimas mensagens**:

1.  **Padrão do Operador Conversacional (OC)** — como o LLM conduz intenção → plano → confirmação → execução, com dry-run, idempotência por plano e políticas aplicadas antes da fila.
2.  **Especificação V1 LLM-friendly** — formalização dos contratos e APIs:
    *   Registry de capacidades + JSON Schemas e metadados `x-llm`.
    *   Objetos `Intent`, `PlanPreview`, `CommitRequest` e FSM de estados.
    *   Endpoints (`/v1/oc/*`, schemas, receipts, metrics, export).
    *   Tool spec para function-calling (list/get/propose/commit/replay).
    *   Policies, assinaturas, âncoras Merkle, métricas SRE e UI PlanCard.
    *   Roadmap de implementação.

Se quiser, junto tudo em um único **doc Markdown** (ou PDF) com índice e links entre seções.

## Prompt:
Fazer pesquisa ampla na internet sobre softwares que tentam a reprodutibilidade de experimentos científicos - o que fazem, como fazem e as dores

## Response:
Thought for 24s

Perfeito, Dan! Fiz uma varredura ampla e organizei o panorama em 6 “famílias” de soluções para reprodutibilidade computacional — com exemplos, como fazem e as dores típicas que relatam. No fim, deixo um mapa das principais lacunas onde o ÁUREA pode brilhar.

1) Empacotamento/cápsulas executáveis
=====================================

**O que fazem:** empacotam código+ambiente+dados e (às vezes) resultados numa “cápsula” que pode ser reexecutada noutro lugar.  
**Como fazem:** traçam dependências e congelam ambiente (sistemas, libs, variáveis) e inputs, depois geram um pacote com manifesto/proveniência; alguns dão DOI e histórico.  
**Exemplos:**

*   **ReproZip/ReproServer**: captura execuções e gera um bundle `.rpz`; reexecuta em diversos “unpackers” (sem lock-in). [reprozip.org+2docs.libnova.com+2](https://www.reprozip.org/?utm_source=chatgpt.com)
*   **Code Ocean**: “compute capsules” imutáveis com lineage graph, versionamento, DOI/publish. [codeocean.com+2library.hkust.edu.hk+2](https://codeocean.com/product/compute-capsules?utm_source=chatgpt.com)
*   **Whole Tale**: “tales” como objetos de pesquisa executáveis (dados+ambiente+código+metadados). [wholetale.org+2labs.globus.org+2](https://wholetale.org/?utm_source=chatgpt.com)
*   **RO-Crate / Workflow RO-Crate**: padrão para empacotar objetos (dados, software, workflows) com metadados FAIR e perfis para **runs** (proveniência de execuções). [researchobject.org+3researchobject.org+3RDA+3](https://www.researchobject.org/ro-crate/about_ro_crate?utm_source=chatgpt.com)

**Dores comuns:** drift do ambiente com o tempo, formatos/ontologias diferentes de proveniência, dados sensíveis e acesso a repositórios externos. [ScienceDirect+1](https://www.sciencedirect.com/science/article/pii/S0167739X25002195?utm_source=chatgpt.com)

2) Orquestradores/workflow engines
==================================

**O que fazem:** definem pipelines determinísticos, paralelizam, escalam em HPC/cloud e **integram containers** para portabilidade.  
**Como fazem:** linguagens declarativas (Snakefile, Nextflow DSL, CWL), execução com Docker/Apptainer, caching, além de catálogos comunitários (nf-core, WorkflowHub).  
**Exemplos:**

*   **Snakemake**: workflows declarados em Python; descrevem softwares e implantam automaticamente; documentação foca em portabilidade e reprodutibilidade. [snakemake.readthedocs.io+1](https://snakemake.readthedocs.io/en/stable/?utm_source=chatgpt.com)
*   **Nextflow**: executa com Docker/Apptainer/Podman; mesma pipeline roda em qualquer Unix; integração com Git e nf-core. [nextflow.io+1](https://nextflow.io/docs/latest/container.html?utm_source=chatgpt.com)
*   **Galaxy**: plataforma web multi-usuário com workflows reprodutíveis e biblioteca pública; integra repositórios de dados (ex.: Dataverse) e fornece DOIs para workflows curados. [OUP Academic+2iwc.galaxyproject.org+2](https://academic.oup.com/nar/article/52/W1/W83/7676834?utm_source=chatgpt.com)
*   **WorkflowHub** (registro): publica/descobre workflows com **RO-Crate**, CWL, TRS; promove workflows como objetos FAIR. [workflowhub.eu+2Nature+2](https://workflowhub.eu/?utm_source=chatgpt.com)

**Dores comuns:** falhas de reexecução automática por dependências externas, falta de padrões interoperáveis de proveniência (daí o avanço do **Workflow Run RO-Crate**). [mir.cs.illinois.edu+1](https://mir.cs.illinois.edu/marinov/publications/GraysonETAL23SnakemakeNextflow.pdf?utm_source=chatgpt.com)

3) Notebooks reproduzíveis (ensino/demos)
=========================================

**O que fazem:** tornam repositórios executáveis em um clique para ensino, revisão e demo.  
**Como fazem:** **Binder/BinderHub** constrói imagens a partir de `environment.yml`/`requirements.txt`/`Dockerfile` e sobe um Jupyter efêmero. [jupyter.org+2mybinder.org+2](https://jupyter.org/binder?utm_source=chatgpt.com)  
**Dores:** limites de recursos/tempo, falta de GPU persistente, e dependência de arquivos de configuração bem mantidos. [earth-env-data-science.github.io](https://earth-env-data-science.github.io/lectures/environment/binder.html?utm_source=chatgpt.com)

4) Gestão de dados, versões e proveniência
==========================================

**O que fazem:** versionam datasets e modelos, ligam execuções a artefatos, e preservam o **lineage** (quem gerou o quê, quando e como).  
**Como fazem:** sobre Git/git-annex e/ou armazenamento de objetos; logs de execuções; referências imutáveis.  
**Exemplos:**

*   **DataLad**: versionamento de dados/coleções, integração com RDM; casos de “paper reprodutível”. [datalad.org+1](https://www.datalad.org/?utm_source=chatgpt.com)
*   **DVC**: extensão Git para dados/modelos/experimentos; pipelines reproduzíveis para ML. [DVC+1](https://dvc.org/?utm_source=chatgpt.com)
*   **MLflow**: rastreia parâmetros, métricas e **artefatos**; registry de modelos para reexecutar/validar. [MLflow+1](https://mlflow.org/docs/latest/api_reference/python_api/mlflow.artifacts.html?utm_source=chatgpt.com)
*   **Pachyderm**: pipelines dirigidos a dados com **data versioning** e lineage automáticos em Kubernetes. [GitHub+1](https://github.com/pachyderm/pachyderm?utm_source=chatgpt.com)

**Dores:** custo/complexidade de hospedar infra, GC de dados grandes, políticas de acesso/PII; necessidade de “glue” para ligar dados↔workflow↔recibos verificáveis. [Atlan+1](https://atlan.com/pachyderm-data-lineage/?utm_source=chatgpt.com)

5) Portais de compartilhamento/DOI e open science
=================================================

**O que fazem:** providenciam depósito/DOI para dados, código, cápsulas e materiais; melhoram citação e descoberta.  
**Exemplos:** **OSF** (gestão do ciclo de pesquisa, preregistro, integra GitHub/Drive); **Figshare** (repositório com DOI). [cos.io+2Current Protocols+2](https://www.cos.io/products/osf?utm_source=chatgpt.com)

**Dores:** políticas desiguais entre periódicos, pouca padronização de “o que conta” como material reproduzível. [Frontiers](https://www.frontiersin.org/journals/computer-science/articles/10.3389/fcomp.2024.1491823/full?utm_source=chatgpt.com)

6) Padrões de metadados e práticas
==================================

**O que fazem:** definem boas práticas e padrões para tornar pacotes/workflows **FAIR** e reproduzíveis (PROV, RO-Crate, “Ten Simple Rules”).  
**Referências úteis:** guias e revisões listando barreiras (ambiente, dados restritos, cultura e treino) e recomendações. [PLOS+2PLOS+2](https://journals.plos.org/ploscompbiol/article?id=10.1371%2Fjournal.pcbi.1003285&utm_source=chatgpt.com)

* * *

Padrões de “como” (recorrentes nos projetos)
--------------------------------------------

1.  **Containerização + pin de versões** (Docker/Apptainer) p/ isolar dependências. [nextflow.io+1](https://nextflow.io/docs/latest/container.html?utm_source=chatgpt.com)
2.  **Proveniência de execução** (graph/lineage, RO-Crate para runs). [codeocean.com+1](https://codeocean.com/product/compute-capsules?utm_source=chatgpt.com)
3.  **Empacotes citáveis** com manifesto + DOI (capsule/tale/RO-Crate). [wholetale.org+1](https://wholetale.org/?utm_source=chatgpt.com)
4.  **Registries** de workflows (descoberta, versões, testes contínuos). [iwc.galaxyproject.org](https://iwc.galaxyproject.org/about/?utm_source=chatgpt.com)
5.  **Versionamento de dados/modelos** e amarração aos jobs. [datalad.org+1](https://www.datalad.org/?utm_source=chatgpt.com)

* * *

Dores estruturais (síntese)
---------------------------

*   **Drift e envelhecimento**: ambientes e imagens se tornam irreprodutíveis com o tempo; necessidade de “receipts” verificáveis e âncoras. [mir.cs.illinois.edu](https://mir.cs.illinois.edu/marinov/publications/GraysonETAL23SnakemakeNextflow.pdf?utm_source=chatgpt.com)
*   **Dados e acesso**: PII, dados proprietários e repositórios privados quebram demos públicas; soluções integram uploads diretos e trilhas auditáveis. [galaxyproject.org](https://galaxyproject.org/news/2026-01-15-dataverse/?utm_source=chatgpt.com)
*   **Interoperabilidade de proveniência**: múltiplos formatos; RO-Crate Run surge para padronizar “execução”. [PLOS](https://journals.plos.org/plosone/article?id=10.1371%2Fjournal.pone.0309210&utm_source=chatgpt.com)
*   **Custo/infra**: operar clusters/containers/registries e armazenar versões é caro; GC e políticas de retenção são essenciais. [Atlan](https://atlan.com/pachyderm-data-lineage/?utm_source=chatgpt.com)
*   **Cultura e política editorial**: variação grande nas exigências de reprodutibilidade e pouco reconhecimento formal para software/dados. [Frontiers](https://www.frontiersin.org/journals/computer-science/articles/10.3389/fcomp.2024.1491823/full?utm_source=chatgpt.com)
*   **GPU/aleatoriedade**: determinismo em ML/AV1/HDL exige seeds, fixar clocks/TZ, desabilitar rede quando necessário (práticas, não só ferramentas). [PLOS](https://journals.plos.org/ploscompbiol/article?id=10.1371%2Fjournal.pcbi.1003285&utm_source=chatgpt.com)

* * *

Onde o ÁUREA se encaixa (oportunidades)
---------------------------------------

*   **Recibos fortes**: muitos projetos têm metadata/lineage, poucos entregam **receipts assinados** com hashes de entrada/saída e política aplicada — lacuna que o ÁUREA cobre nativamente. [PLOS](https://journals.plos.org/plosone/article?id=10.1371%2Fjournal.pone.0309210&utm_source=chatgpt.com)
*   **Idempotência por plano** + **DUAL\_CONTROL/local-only** antes da fila: raramente é 1ª classe fora de domínios regulados. [Frontiers](https://www.frontiersin.org/journals/computer-science/articles/10.3389/fcomp.2024.1491823/full?utm_source=chatgpt.com)
*   **Pacote universal de artefatos (VCX-PACK) + RO-Crate**: interoperar com RO-Crate/WorkflowHub mantendo benefícios de empacote binário eficiente. [researchobject.org](https://www.researchobject.org/workflow-run-crate/?utm_source=chatgpt.com)
*   **UI conversacional com dry-run**: grande parte das plataformas exige formulariões; o padrão OC do ÁUREA reduz atrito sem perder contrato. (Contexto comparado a Code Ocean “preview” e Galaxy “history/workflow tests”.) [codeocean.com+1](https://codeocean.com/product/compute-capsules?utm_source=chatgpt.com)

* * *

Lista curta de ferramentas (por uso)
------------------------------------

*   **Cápsulas/empacote:** ReproZip; Code Ocean; Whole Tale; RO-Crate/Workflow RO-Crate. [ScienceInsights+3reprozip.org+3codeocean.com+3](https://www.reprozip.org/?utm_source=chatgpt.com)
*   **Workflows:** Snakemake; Nextflow; Galaxy; CWL (via WorkflowHub). [workflowhub.eu+3snakemake.readthedocs.io+3nextflow.io+3](https://snakemake.readthedocs.io/en/stable/?utm_source=chatgpt.com)
*   **Dados/linhagem:** DataLad; DVC; MLflow; Pachyderm. [GitHub+3datalad.org+3DVC+3](https://www.datalad.org/?utm_source=chatgpt.com)
*   **Notebooks:** Binder/BinderHub. [jupyter.org](https://jupyter.org/binder?utm_source=chatgpt.com)
*   **Open science/DOI:** OSF; Figshare. [cos.io+1](https://www.cos.io/products/osf?utm_source=chatgpt.com)

Se quiser, converto isso num **doc comparativo** (tabelado) com critérios (empacote, workflows, proveniência, DOI, containers, GPU, PII, políticas), marcando onde cada um é forte/fraco e adiciono recomendações de interoperabilidade direta com o ÁUREA (ex.: export de **Workflow Run RO-Crate** junto do **Receipt**).

## Prompt:
O que é importante incorporar

## Response:
Perfeito — aqui vai o que **vale incorporar já** no ÁUREA para ficar sólido, interoperável e com UX leve, sem perder força técnica.

1) Padrões e interoperabilidade (sem lock-in)
---------------------------------------------

*   **Export/Import RO-Crate (incl. Workflow Run RO-Crate)** junto do **Receipt**: mantém FAIR/descoberta e facilita publicação/DOI.
*   **Mapeamento VCX-PACK ↔ RO-Crate**: manifesto → `ro-crate-metadata.json`, `entries[]` → `hasPart[]`; incluir `provenance/activity`.
*   **JSON Schema + “tool spec”** por ação: `schema_id/v`, exemplos, `x-llm` (defaults/hints), `x-ui` (widget/semântica).
*   **CWL/Snakemake/Nextflow adapters (leitura)**: importar um pipeline e convertê-lo para DAG tipado do ÁUREA (mesmo que com perda mínima).

2) Determinismo e proveniência “by default”
-------------------------------------------

*   **Canonical JSON** para hashing (NRF/CID) e idempotência por plano (hash do DAG).
*   **Seeds/TZ/clock fixos** e **“no network”** opcional por policy; registrar **hardware/runtime fingerprint** (CPU/GPU, driver, libc).
*   **Merkle diário** para recibos (root publicado); todas execuções com **assinatura ed25519** e `kid` rotativo.
*   **KATs contínuos**: toda ação crítica tem `exp_ingest`/comparador oficial (bit-a-bit para ciência/HDL; tolerâncias documentadas para mídia/ML).

3) Política e segurança antes da fila
-------------------------------------

*   **Avaliação de policy no `:propose`** (não no `:commit`): `local-only/PII`, janelas, cotas e DUAL\_CONTROL já aparecem no **PlanCard**.
*   **DUAL\_CONTROL** com frase de compromisso e carimbo no recibo; **rastreabilidade** do aprovador no `policy_trace`.
*   **Rotas automáticas**: se PII ou segredo → força **worker local**; se janela restrita → **agenda** em vez de falhar.

4) Dados e artefatos como 1ª classe
-----------------------------------

*   **Integração opcional com DVC/DataLad**: gravar no `Receipt` a referência imutável do dataset/modelo (hash/URL).
*   **`verify()` nativo do VCX-PACK** + validação de offsets/hashes; **policy de retenção** e **GC** para packs grandes.
*   **Catálogo de artefatos** consultável (por `pack_cid`, `schema_id`, tags), com **linhagem** (quem/qual recibo gerou).

5) Execução e orquestração simplificadas
----------------------------------------

*   **Um binário (`aurea`)**: API+UI+fila/leases+assinaturas+metrics; workers **in-proc** por padrão, QUIC/out-of-proc quando isolar.
*   **Leases no redb** com reassign automático; idempotência `(tenant, topic, idem_key)` preenchida por hash do plano se ausente.
*   **Estados padronizados** num stream: `accepted → assigned → progress{stage} → result|fail{code}` com **tempos por etapa**.

6) Operador Conversacional (LLM) com contratos
----------------------------------------------

*   **Duas etapas**: `:propose` → **PlanCard** (DAG, SLOs, custos, policy\_trace) → **Confirmar** → `:commit`.
*   **Auto-reparo N=2** para slots; se falhar, pedir **só** os campos faltantes (lista curta).
*   **Idempotência por plano**: `idem_key = blake3(dag_canon)`; repetir “OK” não duplica.

7) Observabilidade e SLOs operacionais
--------------------------------------

*   **Métricas SRE**: `ttft_ms`, `ttr_ms`, `queue_depth`, `reassigns`, `error_rate{code}`, `stage_time_ms{stage}`.
*   **Exports Arrow/Parquet** e consultas DataFusion; **amostras de recibos** para auditoria.
*   **Orçamentos em tempo real**: mostrar no PlanCard e emitir alerta quando excedidos.

8) Publicação e citabilidade
----------------------------

*   **DOI opcional** para packs/execuções (integração OSF/Figshare/Zenodo) via botão “Publicar” no recibo.
*   **Manifesto de citação** (CITATION.cff/RO-Crate) gerado automaticamente com autores, versão e `receipt_cid`.

9) Testes, CI e hardening
-------------------------

*   **Testes de determinismo** por domínio (ciência/HDL bit-a-bit; mídia com thresholds fixos).
*   **Chaos/staging** com latência/falha/kill para validar reassign e SLOs.
*   **Compatibilidade GPU**: lista de versões/flags que garantem determinismo (ex.: desativar kernels não determinísticos quando exigido).

* * *

Prioridade prática (o que entrar agora)
=======================================

### P0 — Base “impossível de quebrar”

1.  **Assinatura de recibos + Merkle diário**
2.  **Canonical JSON + idempotência por plano**
3.  **Policy no `:propose` + DUAL\_CONTROL**
4.  **Verificação VCX-PACK (`verify()`)**
5.  **PlanCard com SLOs/custos/policy\_trace**

### P1 — Interop e dados

6.  **Export RO-Crate (incl. Run)** junto do Receipt
7.  **Referências DVC/DataLad no recibo**
8.  **Adapters de leitura de CWL/Snakemake → DAG ÁUREA**

### P2 — Operação e SRE

9.  **Métricas SRE completas + exports Parquet**
10.  **Chaos/staging + testes de determinismo automatizados**

* * *

Onde encaixa no monorepo
------------------------

*   `core/`: canonical JSON, tipos `Intent/PlanPreview/CommitRequest`.
*   `receipts/` (ou dentro do binário): assinatura, Merkle, `verify()` de assinatura.
*   `artifacts/vcx_pack/`: `verify()`, mapeamento para RO-Crate.
*   `runtime/`: leases, tempos por etapa, roteamento por `topic`.
*   `plugins/op_llm_json/`: `:propose → PlanCard → :commit` com auto-reparo.
*   `ui_web/`: **PlanCard** + “📜 Recibo” com download/verify/export/DOI.
*   `bus/` (ou `aurea`): endpoints `/v1/capabilities`, `/v1/oc/*`, `/v1/export`, `/v1/anchors/*`.

* * *

Aceite mínimo para cada item
----------------------------

*   **Recibo assinado**: `signatures[].alg=Ed25519`, `kid` válido, `verify_receipt()` retorna OK.
*   **Merkle**: `anchors/YYYY-MM-DD.json` com root e lista de `receipt_cid`.
*   **PlanCard**: mostra `policy_trace`, SLOs (TTFT/TTR), custos e **Confirmar/Agendar/Editar**.
*   **RO-Crate**: arquivo `ro-crate-metadata.json` alinhado ao pack; `run` presente quando houver execução.
*   **Determinismo**: replays do mesmo recibo geram **hash(es) idênticos** (ou dentro do tolerance configurado/registrado).

Se topar, já abro os épicos: **P0-Assinaturas+Merkle**, **P0-Canonical+Idempotência**, **P0-Policy-Propose+DUAL\_CONTROL**, **P0-PlanCard**, **P0-VCX-PACK Verify** — e deixo os contratos/rotas prontos para o OC e para o export RO-Crate.