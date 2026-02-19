# OC — Diretrizes de Prompts

## Confirmações
- "Conferi e confirmo o plano."

## Padrões por ação
- vcx.batch_transcode: {codec, width, height, bitrate}
- science.run: {seed, image, inputs[], params{}}
- hdl.sim: {top, cycles, asserts[]}

## Anti-padrões
- Campos vagos ("rápido", "alto"): normalize para números/unidades
- Campos PII: marcar para local-only
