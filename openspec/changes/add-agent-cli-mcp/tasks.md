## 1. Contratos e proprietário único

- [ ] 1.1 Aprovar planejamento desta extensão junto ao desktop antes de G0, iniciando sua implementação somente após aprovação técnica G0 e fixar SDK MCP/MSRV/licenças; verificar ADR e compatibilidade do ELF/build Docker.
- [ ] 1.2 Extrair application/control com DTO JSON v1 e schemas; verificar core independente de UI/SQL/MCP e erros/limites por testes (AG-02).
- [ ] 1.3 Implementar modos GUI/service e lock/IPC protegido via platform; verificar dois clientes, proprietário concorrente, socket/symlink malicioso e engine_unavailable sem spawn (AG-01).
- [ ] 1.4 Implementar service sem DISPLAY com dispositivos/segredos explícitos; verificar inicialização/encerramento e ausência de captura antes de operação autorizada (AG-01, AG-04).

## 2. CLI, autorização e ledger

- [ ] 2.1 Implementar consultas CLI, paginação e JSON/exit codes; verificar snapshots de contrato, dados sensíveis negados e config/doctor sem segredo (AG-02, AG-04).
- [ ] 2.2 Implementar aprovações humanas no proprietário e concessões GUI com bootstrap headless single-use fora de CLI/MCP; verificar escopos, destinos, prazo/quota, revogação, restart e autoaprovação via confirm/yes/TTY/pty rejeitada e boundary mesmo UID documentado (AG-04).
- [ ] 2.3 Implementar ledger durável e versionamento de sessão; verificar repetição, payload conflitante, expiração/epoch e awaiting_approval expirada sem efeito e crash outcome_unknown consultável entre epochs com novo grant, HMAC efêmero e nenhum retry automático (AG-05).
- [ ] 2.4 Implementar controles CLI e consulta de operation_id; verificar estado real, filas, prazo, sessão inexistente e operação negada sem efeito (AG-02, AG-05).
- [ ] 2.5 Implementar auditoria sanitizada/retida; verificar nenhum segredo, DTMF/PIN ou hash simples reversível, nome/número completo em erro/log sem escopo e exclusão confirmada (AG-04, AG-05).

## 3. MCP e aceite real

- [ ] 3.1 Implementar MCP stdio com SDK Rust oficial fixado e tools estritas; verificar initialize/tools/list/call, schemas e stdout exclusivamente protocolo com cliente SDK real (AG-03).
- [ ] 3.2 Implementar limites/cancelamento e reuse dos use cases/policies; verificar entrada malformada/excessiva, rate limit, desconexão e nenhuma autorização emitida pelo agente (AG-03, AG-04).
- [ ] 3.3 Executar Asterisk no ELF exato via CLI e MCP; verificar dial/answer/reject/hangup/DTMF/mute/hold/transfer/DND e ausência de duplicação por retransmissão (AG-05, AG-06).
- [ ] 3.4 Validar GUI e service sem DISPLAY com headset/peer humano na topologia G4 VM auxiliar/host-only; verificar áudio bidirecional, captura consentida, cliente desconectado e shutdown do proprietário com recibo sanitizado próprio por modo/hash, fechando parte humana AG-06 em G4 (AG-06).
- [ ] 3.5 Documentar comandos, JSON/erros, threat model, grants e configuração MCP genérica; revisar com CLI Claude e verificar clone sem base, fmt/clippy/test/doc/audit e gates AG no G3b, G4/G5 desktop antes de publicar.
