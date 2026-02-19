# Microcopy — Padrões (v1.1)

## Ações
- Confirmar: "Conferi e confirmo o plano."
- Editar: "Ajustar parâmetros"
- Agendar: "Executar mais tarde"
- Repetir: "Repetir execução (mesmo plano)"

## Estados
- Preview vazio: "Envie os detalhes ou escolha um template."
- Em execução: "Seu job está processando. Acompanhe o progresso aqui."
- Sucesso: "Pronto! O recibo está assinado e disponível abaixo."
- Erro geral: "Não deu certo desta vez. Veja os detalhes e tente novamente."

## Tooltips
- policy_trace: "Regras avaliadas antes da execução."
- local-only: "Executa estritamente no ambiente local."

## Erros (mapa code→mensagem humana)
- SCHEMA_INVALID → "Faltam informações. Complete os campos destacados."
- POLICY_BLOCKED → "Este plano viola uma política. Veja as regras aplicadas."
- DUAL_CONTROL_REQUIRED → "Confirmação adicional é necessária para continuar."
- PLAN_CONFLICT → "O plano mudou. Atualize o cartão e confirme novamente."
