# SLIs & SLOs

- SLIs: ttft_ms, ttr_ms, queue_depth, reassigns_total, error_rate{code}, stage_time_ms{stage}
- SLOs MVP: TTFT p95 ≤ 4s; TTR p95 ≤ 9s; error_rate < 2%


## Alertas (recomendado)
- ttft_ms_p95 > 4s por 15min → WARN
- ttr_ms_p95 > 9s por 15min → WARN
- error_rate > 2% por 10min → ALERT

## Orçamentos por tenant (exemplo)
- tokens_mês, tempo_cpu_ms_mês (rejeitar preview > orçamento)


## Cobertura de KATs (meta)
- ≥95% dos pipelines críticos por domínio com KAT definido e executado em CI
