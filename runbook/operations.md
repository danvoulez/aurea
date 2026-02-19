# Operação

- Start: `aurea serve --db ./aurea.redb --listen 0.0.0.0:8080`
- Keys: `aurea keys rotate`
- Backups: snapshots de redb + exports Parquet


## Backup transacional (recomendado)
1) Pausar ingest (modo leitura): `aurea serve --read-only` ou flag admin
2) Executar snapshot do redb
3) Retomar ingest; registrar horário e versão
