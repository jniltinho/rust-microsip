# Revisão Claude CLI — pós-correções rust-microsip

Data: 2026-10-12. Somente planejamento sanitizado.

# Veredito: **APPROVE**

Os seis bloqueadores da rodada 3 estão resolvidos no conjunto atualizado. Não restou bloqueador concreto. Avaliei apenas o planejamento: nada aqui comprova funcionamento, e os gates G0–G5 e GW0 continuam pendentes.

## Verificação dos bloqueadores

| # | Status | Evidência no conjunto |
|---|---|---|
| **B1** TTY/pty não comprova humano | ✅ Resolvido | `add-agent-cli-mcp/design.md`, seção "Autorização que o agente não pode autoemitir": aprovação e emissão de grants acontecem só na GUI, sem `cli approve/grant` e sem fallback TTY/pty. O bootstrap é single-use, exportado pela GUI e consumido atomicamente pelo service. Shell irrestrito no mesmo UID está declarado fora do boundary. A spec AG-04 reflete isso ("TTY/pty MUST NOT criar aprovação"), e a tarefa 2.2 testa as tentativas de autoaprovação. |
| **B2** Namespace e aprovação pendente | ✅ Resolvido | Ledger com chave global `(perfil, epoch, idempotency_key)`, independente de cliente e grant. Payload ou escopo diferente gera `conflict`. A aprovação fica vinculada a `(perfil, epoch, key, HMAC do payload)`, não a `request_id`. O fluxo `awaiting_approval` → `operation_get` não reenvia a intenção. |
| **B3** Ledger entre epochs | ✅ Resolvido | Mutação de epoch antiga retorna `stale_epoch` sem efeito. `operation_get` de epoch antiga funciona por 24 h com grant de leitura vigente e depois retorna `result_expired`, que nunca vira nova intenção. Consta tanto na spec AG-05 quanto na tarefa 2.3. |
| **B4** URI e GUI contra service | ✅ Resolvido | O service retorna `uri_confirmation_unavailable` visível e nunca disca. A GUI iniciada contra o service sai com `engine_owned_by_service`, e o service sem grant responde `grant_required`, sem criar canal falso de aprovação. LIN-03 e AG-01 estão alinhados com isso. |
| **B5** Hash de DTMF/PIN | ✅ Resolvido | O fingerprint é HMAC-SHA-256 com chave aleatória por epoch, mantida só em memória e nunca persistida. DTMF pendente fica só em memória, e um crash leva a `outcome_unknown`/`cancelled` sem restaurar dígitos. A tarefa 2.5 testa a ausência de hash simples reversível. |
| **B6** Ordem de gates | ✅ Resolvido | Desktop 1.1 aprova as duas mudanças antes do G0. A tarefa 1.8 inclui IPC v1/SO_PEERCRED no spike. CLI 1.1 só implementa após o G0 técnico. O test-plan define G3b (AG-01..06) antes de G4/G5, e a tarefa 9.10 exige AG-01..06, G4 e G5 antes de publicar. |

## Observações não bloqueadoras

1. **`awaiting_approval` em crash ou restart.** O design cobre explicitamente o caso entre envio e confirmação (`outcome_unknown`) e o DTMF pendente. O estado `awaiting_approval` de epoch anterior só fica coberto indiretamente pela regra "novo efeito exige epoch atual". Sugiro uma frase explícita: "intenções `awaiting_approval` de epoch anterior tornam-se `expired` sem efeito; a nova GUI não as exibe para aprovação". Também vale incluir isso no teste da tarefa 2.3.
2. **Validade de 1 h contra service de longa duração.** Com grant de no máximo 1 h e bootstrap sem reimportação, renovar exige parar o service, abrir a GUI e exportar um novo bootstrap. A restrição é coerente, mas convém documentá-la no guia e no threat model, com o erro esperado ao expirar.
3. **Onde roda o AG-06 humano.** O AG-06 exige headset e peer humano em GUI e service, mas o G3b vem antes do G4. Falta dizer se a voz humana do AG-06 roda na topologia G4 (VM auxiliar e host-only) ou em ambiente próprio, e se o recibo pode ser compartilhado com o G4 sem "aprovação herdada" (tarefa 9.5).
4. **Bootstrap e bind do grant à epoch.** "Associa o grant à nova epoch" está claro. Vale explicitar que um `--grant-file` cujo ID já foi consumido falha com erro tipado (ex.: `bootstrap_consumed`) e que o arquivo não é apagado nem reescrito pelo service sem instrução.
5. **Web: "sem backend próprio" contra proxy obrigatório.** O design web diz "sem backend próprio obrigatório para o modo básico de PBX direto" e, ao mesmo tempo, "reverse proxy de WSS é obrigatório no deploy suportado". Não há contradição real, porque o proxy é infraestrutura do operador, mas trocar "PBX direto" por "sem backend da aplicação; proxy WSS do operador obrigatório" evita leitura ambígua.
6. **Web: observações da rodada.** Todas estão presentes:
   - DND em WEB-02;
   - logs SIP.js/SDP/Authorization desabilitados e inspecionados no console (WEB-04/05, tarefa 2.5);
   - Origin validado no proxy WSS (tarefa 2.6);
   - emissor TURN externo obrigatório quando houver relay;
   - ausência de garantia de zeroização.
7. **Smoke Wine.** O design continua limitando a evidência do Wine a "abertura/layout", e o DIST-06 bloqueia janela, mocks ou revisão como prova de telefonia. Está correto.
