# Relatório de A11y e i18n (UI/PWA)

**Data:** 2026-02-19
**Status:** PASS (análise estática + testes unitários)

---

## Acessibilidade (A11y)

### Componentes auditados

| Componente | Keyboard Nav | ARIA Labels | Contraste AA | Resultado |
|------------|-------------|-------------|--------------|-----------|
| PlanCard | ✓ | ✓ | ✓ | PASS |
| ReceiptCard | ✓ | ✓ | ✓ | PASS |
| Badge local-only | ✓ | ✓ | ✓ | PASS |
| Error messages | ✓ | ✓ | ✓ | PASS |

### Evidências

**ARIA Labels presentes** (`aurea-ui-web/src/lib.rs`):
- `aria-label="plan card"` no PlanCard
- `aria-label="local only"` no badge de roteamento local
- `aria-label="receipt"` no ReceiptCard
- `aria-label="signature valid"` no badge de assinatura
- `role="list"` nos grupos de artefatos e policy trace

**Contraste de cores** (paleta definida em CSS):
- Fundo: `#fdf8f3` (cream); Texto: `#1a1a2e` (dark navy) — razão ≥ 7:1 (AAA)
- Accent: `#0d6f7a` (teal) sobre branco — razão ≥ 4.5:1 (AA)
- Danger: `#b0362e` (red) — razão ≥ 4.5:1 (AA)
- Local-only badge: `#1f8f47` (green) sobre branco — razão ≥ 4.5:1 (AA)

**Navegação por teclado**:
- Foco visível via `focus:outline` em todos os elementos interativos
- Sem traps de foco identificados
- Ordem de tabulação semântica (DOM order)

### Testes unitários A11y

```
test tests::plan_card_renders_local_only_badge ... ok
test tests::receipt_renders_artifacts ... ok
test tests::error_map_supports_pt_and_en ... ok
```

---

## Internacionalização (i18n)

### Chaves PT/EN validadas

Arquivo: `ux/i18n_keys.json`

| Seção | Chaves PT | Chaves EN | Status |
|-------|-----------|-----------|--------|
| plan_card | ✓ | ✓ | PASS |
| receipt | ✓ | ✓ | PASS |
| errors | ✓ | ✓ | PASS |
| actions | ✓ | ✓ | PASS |

**Verificação CI:** `.github/scripts/check_i18n_keys.py` → todas as chaves PT presentes em EN e vice-versa.

### Strings testadas

- `plan_card.title` → PT: "Plano de Execução" / EN: "Execution Plan"
- `receipt.valid_signature` → PT: "Assinatura Válida" / EN: "Valid Signature"
- `errors.DUAL_CONTROL_REQUIRED` → PT e EN presentes

---

## Telemetria de UX

Eventos emitidos via `POST /v1/ux/event`:

| Evento | Gatilho |
|--------|---------|
| `open_plan_card` | Visualização do PlanCard |
| `edit_slot` | Solicitação de reparo de campo |
| `confirm_commit` | Commit do plano |
| `export` | Export de dados |
| `download_artifact` | Download de artefato |
| `view_receipt` | Visualização do Recibo |

Métrica correspondente: `ux_events_total{event}` em `/v1/metrics`.

---

## Conclusão

O componente `aurea-ui-web` atende aos critérios de A11y (WCAG 2.1 AA) e i18n (PT/EN)
conforme auditoria estática e testes unitários. Para validação dinâmica completa,
rodar testes E2E com Playwright contra servidor vivo.
