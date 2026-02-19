# Rotação de Chaves — Procedimento
1) Gerar nova chave (`aurea keys rotate`)
2) Publicar chave pública e `kid`
3) Verificar recibos antigos e novos
4) Rollback: reverter `current` para o `kid` anterior
