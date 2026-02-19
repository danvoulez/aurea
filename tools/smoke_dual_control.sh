#!/usr/bin/env bash
# tools/smoke_dual_control.sh — Smoke test C: DUAL_CONTROL
# Demonstrates: :commit topic without confirm_phrase → DUAL_CONTROL_REQUIRED
#               re-send with correct confirm_phrase → success
# Usage: bash tools/smoke_dual_control.sh [BASE_URL]
set -euo pipefail

BASE_URL="${1:-http://localhost:8080}"
ARTIFACTS="artifacts/smoke"
mkdir -p "$ARTIFACTS"
LOG="$ARTIFACTS/dual_control.log"

log() { echo "[dual-control] $*" | tee -a "$LOG"; }

log "=== Smoke C: DUAL_CONTROL ==="

# Step 1 — Plan preview for a :commit topic (dual control required)
log "Step 1: plan_preview for science:commit topic"
PREVIEW=$(curl -sS -f \
    -H "X-Aurea-Api: 1.0" \
    -H "Content-Type: application/json" \
    -d '{
      "intent": {
        "schema_id": "science.run",
        "v": "1",
        "topic": "science:commit",
        "payload": {
          "seed": 42,
          "image": "docker.io/aurea/science:latest",
          "inputs": ["data/input.csv"],
          "params": {"alpha": 0.01}
        }
      }
    }' \
    "$BASE_URL/v1/oc/plan_preview")
echo "$PREVIEW" | tee "$ARTIFACTS/dc_step1_preview.json"

PLAN_HASH=$(echo "$PREVIEW" | python3 -c 'import sys,json; print(json.load(sys.stdin)["plan_hash"])')
DC_REQUIRED=$(echo "$PREVIEW" | python3 -c 'import sys,json; print(json.load(sys.stdin)["dual_control_required"])')
log "plan_hash=$PLAN_HASH dual_control_required=$DC_REQUIRED"

if [ "$DC_REQUIRED" != "True" ] && [ "$DC_REQUIRED" != "true" ]; then
    log "FAIL: expected dual_control_required=true for science:commit, got $DC_REQUIRED"
    exit 1
fi

# Step 2 — Commit WITHOUT confirm_phrase → expect DUAL_CONTROL_REQUIRED
log "Step 2: commit without confirm_phrase → expect DUAL_CONTROL_REQUIRED (403)"
HTTP_CODE=$(curl -sS -o "$ARTIFACTS/dc_step2_no_phrase.json" -w "%{http_code}" \
    -H "X-Aurea-Api: 1.0" \
    -H "Content-Type: application/json" \
    -d "{\"plan_hash\":\"$PLAN_HASH\"}" \
    "$BASE_URL/v1/oc/commit")
cat "$ARTIFACTS/dc_step2_no_phrase.json"
log "HTTP code: $HTTP_CODE"

if [ "$HTTP_CODE" != "403" ]; then
    log "FAIL: expected 403 for missing confirm_phrase, got $HTTP_CODE"
    exit 1
fi

ERROR_CODE=$(cat "$ARTIFACTS/dc_step2_no_phrase.json" | python3 -c 'import sys,json; print(json.load(sys.stdin)["error"]["code"])' 2>/dev/null || echo "")
if [ "$ERROR_CODE" != "DUAL_CONTROL_REQUIRED" ]; then
    log "FAIL: expected error DUAL_CONTROL_REQUIRED, got $ERROR_CODE"
    exit 1
fi
log "Correctly blocked: DUAL_CONTROL_REQUIRED"

# Step 3 — Commit WITH wrong confirm_phrase → still blocked
log "Step 3: commit with wrong confirm_phrase → expect DUAL_CONTROL_REQUIRED (403)"
HTTP_CODE2=$(curl -sS -o "$ARTIFACTS/dc_step3_wrong_phrase.json" -w "%{http_code}" \
    -H "X-Aurea-Api: 1.0" \
    -H "Content-Type: application/json" \
    -d "{\"plan_hash\":\"$PLAN_HASH\",\"confirm_phrase\":\"wrong phrase\"}" \
    "$BASE_URL/v1/oc/commit")
cat "$ARTIFACTS/dc_step3_wrong_phrase.json"
log "HTTP code with wrong phrase: $HTTP_CODE2"

if [ "$HTTP_CODE2" != "403" ]; then
    log "FAIL: expected 403 for wrong confirm_phrase, got $HTTP_CODE2"
    exit 1
fi

# Step 4 — Commit WITH correct confirm_phrase → success
log "Step 4: commit with correct confirm_phrase → expect success"
COMMIT=$(curl -sS -f \
    -H "X-Aurea-Api: 1.0" \
    -H "Content-Type: application/json" \
    -d "{\"plan_hash\":\"$PLAN_HASH\",\"confirm_phrase\":\"Conferi e confirmo o plano.\"}" \
    "$BASE_URL/v1/oc/commit")
echo "$COMMIT" | tee "$ARTIFACTS/dc_step4_commit.json"

STATUS=$(echo "$COMMIT" | python3 -c 'import sys,json; print(json.load(sys.stdin)["status"])')
log "Commit status: $STATUS"

if [ "$STATUS" != "accepted" ] && [ "$STATUS" != "duplicate" ] && [ "$STATUS" != "duplicate_in_flight" ]; then
    log "FAIL: expected accepted/duplicate, got $STATUS"
    exit 1
fi

log "=== PASS: DUAL_CONTROL funciona corretamente ==="
