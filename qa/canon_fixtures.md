# Canonical JSON Fixtures

`ProjetoFinal.md` define o baseline e estes fixtures são a referência canônica para validação de canonicalização JSON/NRF.

## Localização
- `qa/fixtures/canon/jcs_vectors_ok.json`
- `qa/fixtures/canon/jcs_vectors_err.json`
- `qa/fixtures/canon/profile_null_strip.json`
- `qa/fixtures/canon/profile_num_norm.json`

## Cobertura
- `jcs_vectors_ok.json`: casos válidos de ordenação/chars Unicode.
- `jcs_vectors_err.json`: entradas inválidas que devem falhar com `SCHEMA_INVALID`.
- `profile_null_strip.json`: perfil com remoção de `null`.
- `profile_num_norm.json`: perfil com normalização numérica (`1.0 -> 1`, `-0.0 -> 0`).

## Regra
- Qualquer implementação de canonicalização usada para `NRF/CID` deve passar todos os vetores acima sem divergência.
