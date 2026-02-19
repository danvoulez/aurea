# Mapeamento VCX-PACK → RO-Crate

- Manifesto → `ro-crate-metadata.json`
- Entradas → `hasPart[]` com hashes/offsets
- Execução → Workflow Run RO-Crate (inputs/outputs/steps)

## Implementação MVP atual (`POST /v1/export`, `format=ro-crate`)
- Dataset raiz (`@id: "./"`) com `name`, `datePublished` e `hasPart`
- Parte principal:
  - `receipts.json` com snapshot dos recibos exportados
- Se `receipt.artifacts[]` existir:
  - adiciona nós `File` (`artifact-N`) com `identifier=cid:<pack_cid>` e `contentSize`
- Metadado obrigatório:
  - `ro-crate-metadata.json` com `conformsTo: https://w3id.org/ro/crate/1.1`

## Próximo passo (PR-08 completo)
- Adicionar mapeamento de `PlanPreview` para `CreateAction/HowToStep`
- Ligar artefatos VCX-PACK (`hasPart`) com `sha256`/`contentSize`
