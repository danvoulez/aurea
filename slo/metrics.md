# Métricas & Export

- Prometheus em `/v1/metrics`
- Exports em `/v1/export` (Parquet/Arrow)

## Regras SLO (Prometheus)

- Arquivo de regras: `configs/prometheus/aurea-alerts.yml`
- `AureaTTFTP95High`:
  `histogram_quantile(0.95, sum(rate(ttft_ms_bucket[15m])) by (le)) > 4000`
- `AureaTTRP95High`:
  `histogram_quantile(0.95, sum(rate(ttr_ms_bucket[15m])) by (le)) > 9000`
- `AureaErrorRateHigh`:
  `max_over_time(error_rate{code="runtime_fail"}[10m]) > 0.02`
