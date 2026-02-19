# Operação

- Start: `aurea serve --db ./aurea.redb --listen 0.0.0.0:8080`
- Keys: `aurea keys rotate`
- Leases expirados: `aurea runtime sweep --expired --db ./aurea.redb`
- Rebuild de âncora diária: `aurea anchors rebuild --db ./aurea.redb --date YYYY-MM-DD --out-dir ./anchors`
- Retenção (dry-run): `aurea retention receipts --db ./aurea.redb --older-than-days 30`
- Retenção (apply): `aurea retention receipts --db ./aurea.redb --older-than-days 30 --apply`
- Backups: snapshots de redb + exports Parquet


## Backup transacional (recomendado)
1) Pausar ingest (modo leitura): `aurea serve --read-only` ou flag admin
2) Executar snapshot do redb
3) Retomar ingest; registrar horário e versão
