# Prometheus Alert Rules

Arquivo de regras MVP: `configs/prometheus/aurea-alerts.yml`.

## Como carregar no Prometheus

No `prometheus.yml`, inclua:

```yaml
rule_files:
  - /etc/prometheus/rules/aurea-alerts.yml
```

Depois, copie este arquivo para o path configurado e recarregue:

```bash
promtool check rules /etc/prometheus/rules/aurea-alerts.yml
curl -X POST http://localhost:9090/-/reload
```

## Regras incluídas (MVP)

- `AureaTTFTP95High` (WARN): `ttft_ms` p95 > 4s por 15m
- `AureaTTRP95High` (WARN): `ttr_ms` p95 > 9s por 15m
- `AureaErrorRateHigh` (ALERT): `error_rate` > 2% por 10m
