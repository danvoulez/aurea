# Operação

- Start: `aurea serve --db ./aurea.redb --listen 0.0.0.0:8080`
- Keys: `aurea keys rotate`
- Leases expirados: `aurea runtime sweep --expired --db ./aurea.redb`
- Rebuild de âncora diária: `aurea anchors rebuild --db ./aurea.redb --date YYYY-MM-DD --out-dir ./anchors`
- Retenção (dry-run): `aurea retention receipts --db ./aurea.redb --older-than-days 30`
- Retenção (apply): `aurea retention receipts --db ./aurea.redb --older-than-days 30 --apply`
- Backups: snapshots de redb + exports Parquet

## Supervisor (PMDaemon)
- Start:
  - `aurea-pmdaemon --cmd target/debug/aurea --arg0 serve --listen 0.0.0.0:8080 --db ./aurea.redb --keys-dir ./keys`
- Health defaults:
  - `health_url=http://127.0.0.1:<port>/healthz`
  - `health_grace_ms=5000`, `health_interval_ms=1500`, `health_timeout_ms=2000`
  - `max_consecutive_health_failures=3`
- Restart/backoff:
  - backoff exponencial (1s, 2s, 4s, ...) com teto de 30s
  - rotação de `aurea.out.log` e `aurea.err.log` a cada restart

## Teste manual PMDaemon
1. Subir `aurea-pmdaemon` apontando para um `db` temporário.
2. Confirmar `/healthz` respondendo em 200.
3. Forçar falha no child (kill do processo `aurea`) e verificar restart com backoff.
4. Simular health inválido (`--health-url` incorreto) e verificar restart por `max_consecutive_health_failures`.


## Backup transacional (recomendado)
1) Pausar ingest (modo leitura): `aurea serve --read-only` ou flag admin
2) Executar snapshot do redb
3) Retomar ingest; registrar horário e versão
