#!/usr/bin/env bash
# tools/smoke_idem.sh — Smoke test A: Idempotência por plano
# Demonstrates: same plan_hash → same receipt_cid (IDEM_DUPLICATE)
# Usage: bash tools/smoke_idem.sh [BASE_URL]
set -euo pipefail

BASE_URL="${1:-http://localhost:8080}"
ARTIFACTS="artifacts/smoke"
mkdir -p "$ARTIFACTS"
LOG="$ARTIFACTS/idem.log"

log() { echo "[idem] $*" | tee -a "$LOG"; }

log "=== Smoke A: Idempotência por plano ==="

# Step 1 — Parse intent
log "Step 1: POST /v1/oc/parse_intent"
INTENT=$(curl -sS -f \
    -H "X-Aurea-Api: 1.0" \
    -H "Content-Type: application/json" \
    -d '{"schema_id":"vcx.batch_transcode","v":"1","payload":{"codec":"av1","width":640,"height":360,"bitrate":600}}' \
    "$BASE_URL/v1/oc/parse_intent")
echo "$INTENT" | tee "$ARTIFACTS/idem_step1_intent.json"
log "Intent parsed OK"

# Step 2 — Plan preview
log "Step 2: POST /v1/oc/plan_preview"
PREVIEW=$(curl -sS -f \
    -H "X-Aurea-Api: 1.0" \
    -H "Content-Type: application/json" \
    -d "{\"intent\": $(echo "$INTENT" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(json.dumps(d["intent"]))')}" \
    "$BASE_URL/v1/oc/plan_preview")
echo "$PREVIEW" | tee "$ARTIFACTS/idem_step2_preview.json"

PLAN_HASH=$(echo "$PREVIEW" | python3 -c 'import sys,json; print(json.load(sys.stdin)["plan_hash"])')
log "plan_hash=$PLAN_HASH"

# Step 3 — First commit (no idem_key; uses plan_hash)
log "Step 3: POST /v1/oc/commit (first)"
COMMIT1=$(curl -sS -f \
    -H "X-Aurea-Api: 1.0" \
    -H "Content-Type: application/json" \
    -d "{\"plan_hash\":\"$PLAN_HASH\",\"confirm_phrase\":\"Conferi e confirmo o plano.\"}" \
    "$BASE_URL/v1/oc/commit")
echo "$COMMIT1" | tee "$ARTIFACTS/idem_step3_commit1.json"
STATUS1=$(echo "$COMMIT1" | python3 -c 'import sys,json; print(json.load(sys.stdin)["status"])')
WORK_ID=$(echo "$COMMIT1" | python3 -c 'import sys,json; print(json.load(sys.stdin)["work_id"])')
log "First commit: status=$STATUS1 work_id=$WORK_ID"

# Wait for job to complete
log "Waiting for job to complete..."
sleep 2

# Try to get receipt_cid from completed job
RECEIPT_CID=""
for attempt in 1 2 3 4 5; do
    RECEIPT_RESP=$(curl -sS -f \
        -H "X-Aurea-Api: 1.0" \
        -H "Content-Type: application/json" \
        -d "{\"plan_hash\":\"$PLAN_HASH\",\"confirm_phrase\":\"Conferi e confirmo o plano.\"}" \
        "$BASE_URL/v1/oc/commit") || true
    DUP=$(echo "$RECEIPT_RESP" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(d.get("duplicate","false"))' 2>/dev/null || echo "false")
    if [ "$DUP" = "True" ] || [ "$DUP" = "true" ]; then
        RECEIPT_CID=$(echo "$RECEIPT_RESP" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("receipt_cid",""))' 2>/dev/null || echo "")
        break
    fi
    sleep 1
done

# Step 4 — Re-commit same plan_hash → must be duplicate
log "Step 4: POST /v1/oc/commit (repeat — expect IDEM_DUPLICATE)"
COMMIT2=$(curl -sS -f \
    -H "X-Aurea-Api: 1.0" \
    -H "Content-Type: application/json" \
    -d "{\"plan_hash\":\"$PLAN_HASH\",\"confirm_phrase\":\"Conferi e confirmo o plano.\"}" \
    "$BASE_URL/v1/oc/commit")
echo "$COMMIT2" | tee "$ARTIFACTS/idem_step4_commit2.json"

DUP2=$(echo "$COMMIT2" | python3 -c 'import sys,json; d=json.load(sys.stdin); print(str(d.get("duplicate",False)).lower())')
STATUS2=$(echo "$COMMIT2" | python3 -c 'import sys,json; print(json.load(sys.stdin)["status"])')
log "Second commit: status=$STATUS2 duplicate=$DUP2"

if [ "$DUP2" != "true" ]; then
    log "FAIL: expected duplicate=true on second commit, got $DUP2"
    exit 1
fi

# Assert receipt_cid matches if available
if [ -n "$RECEIPT_CID" ]; then
    CID2=$(echo "$COMMIT2" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("receipt_cid",""))' 2>/dev/null || echo "")
    if [ -n "$CID2" ] && [ "$CID2" != "$RECEIPT_CID" ]; then
        log "FAIL: receipt_cid mismatch: first=$RECEIPT_CID second=$CID2"
        exit 1
    fi
    log "receipt_cid consistent: $RECEIPT_CID"
fi

# Generate idempotency report
cat > "$ARTIFACTS/../reports/report_idempotency.md" << EOF
# Relatório de Idempotência

**Data:** $(date -u +%Y-%m-%dT%H:%M:%SZ)
**Status:** PASS

## Resumo
- \`plan_hash\`: \`$PLAN_HASH\`
- Primeiro commit: status=\`$STATUS1\`
- Segundo commit: status=\`$STATUS2\`, duplicate=\`$DUP2\`
- \`receipt_cid\` consistente: \`${RECEIPT_CID:-n/a}\`

## Evidências
- \`artifacts/smoke/idem_step1_intent.json\`
- \`artifacts/smoke/idem_step2_preview.json\`
- \`artifacts/smoke/idem_step3_commit1.json\`
- \`artifacts/smoke/idem_step4_commit2.json\`

## Conclusão
O sistema retornou resposta com \`duplicate=true\` na segunda submissão do mesmo
\`plan_hash\`, confirmando o comportamento de idempotência (IDEM_DUPLICATE).
EOF

log "=== PASS: Idempotência confirmada ==="
