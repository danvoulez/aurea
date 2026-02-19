# Runbook de Incidentes (v1.1)

## 1) Jobs presos / Reassign em massa
1. Verificar `queue_depth` e `reassigns_total` em /v1/metrics
2. Forçar varredura de leases expirados (`aurea runtime sweep --expired`)
3. Confirmar retomada no stream SSE

## 2) Assinatura inválida de recibo
1. `POST /v1/verify/receipt` e inspecionar `kid`
2. Recarregar chave pública correspondente
3. Revalidar; se falhar: isolar dia e recomputar Merkle

## 3) Âncora do dia ausente/corrompida
1. Rodar `aurea anchors rebuild --date YYYY-MM-DD`
2. Publicar novo root; registrar incidente

## 4) GC de packs
1. Executar política de retenção (dry-run → apply)
2. Rodar `verify()` em amostras e checar recibos impactados
