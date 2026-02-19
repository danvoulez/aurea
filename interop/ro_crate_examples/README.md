# RO-Crate — Exemplos
- Exemplo mínimo: `ro-crate-metadata.json` + recibo embutido
- Workflow Run: inputs/outputs/steps mapeados do PlanPreview

## Gerar exemplo local (MVP)
1) Execute jobs para gerar recibos.
2) Chame:
   - `POST /v1/export` com body `{"format":"ro-crate"}`
3) Abra o diretório retornado em `path`:
   - `ro-crate-metadata.json`
   - `receipts.json`
