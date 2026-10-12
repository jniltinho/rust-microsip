# Validação da proposta

Data: 2026-10-12. Escopo: documentos de planejamento, sem implementação do aplicativo.

## Verificações locais

- OpenSpec CLI 1.10.0: `openspec validate create-native-rust-linux-release --strict --no-interactive` aprovado.
- `openspec status`: proposta, specs, design e tarefas completos (4/4 artefatos de planejamento).
- Rastreabilidade desktop conferida: sete specs, 46 cenários únicos, 46 entradas no plano de testes, 54 tarefas não executadas. Extensões CLI/MCP e web acrescentam uma spec e seis cenários cada, com 14 e 16 tarefas; total de 58 cenários e 84 tarefas pendentes.
- `base/` ignorada por regra raiz `/base/`; nenhum arquivo dela está no índice. Nenhum submódulo de referência foi adicionado.
- Repositório criado inicialmente como `jniltinho/gonnect-rust` e renomeado para `jniltinho/rust-microsip`, mantido privado. Somente documentos/skills aprovados para envio; nenhum release/tag de aplicativo criado.
- Snapshot limpo exportado do índice validado pelo OpenSpec sem `base/`; links relativos, IDs únicos, symlinks das skills e ausência de submódulos verificados.
- `git diff --cached --check` aprovado após normalizar whitespace, incluindo textos importados das skills.
- Nenhum AppImage pessoal executado ou modificado, nenhum banco/credencial pessoal lido, nenhum PBX iniciado nesta etapa.

## Revisão independente pelo CLI do Claude

CLI instalado: 2.1.296. Modo print, ferramentas desabilitadas (`--tools ''`), MCP vazio/estrito, sem hooks ou settings de projeto/usuário e sem persistência de sessão. Entrada: somente documentos sanitizados, não a pasta base ou configurações pessoais. Saída bruta permanece fora do repositório; relatório textual sanitizado preservado em `docs/reviews/`.

Primeira rodada: **REVISE**, com cinco bloqueadores documentais, registrada em [round-1](../../../docs/reviews/claude-proposal-round-1.md).

Correções aplicadas após avaliação dos apontamentos:

1. Perfil host troca a rede interna por bridge não interna com binds somente loopback.
2. G4 usa PBX Docker numa VM auxiliar e rede host-only restrita; VMs cliente continuam limpas, com segundo endpoint humano definido.
3. Impairment usa proxy UDP de usuário; NAT/STUN recebe roteiro manual em VMs isoladas, sem privilégio nos containers default.
4. ELF do G0 inclui a mesma pilha UI/áudio/D-Bus/IPC, renderer compatível, teclado e hotplug antes de fechar allowlist.
5. “Sem Qt instalado” corrigido para “sem pacotes adicionais ao desktop padrão” e “ELF sem dependência Qt”, permitindo KDE padrão.

Também explicitados permissões WAL/SHM/backup, comportamento de `--config`, senha literal privada, duplicatas CSV e resposta DND.

Segunda rodada (escopo desktop anterior, sem as extensões atuais): **APPROVE**, sem bloqueadores documentais restantes, registrada em [round-2](../../../docs/reviews/claude-proposal-round-2.md). Entrada sanitizada identificada pelo SHA-256 `185fc546a12be27d1f23cbec7b9240751ff5118a0973aa6586a0196a9a9dd540`; processo terminou com exit code 0 e resultado success, sem erro. Observações não bloqueadoras sobre comando G4, merge de overrides, filtro de forwarding Docker e teardown foram explicitadas no design após a aprovação. Revisão não prova interoperabilidade nem execução do futuro laboratório.

## Limites

Não foram executados Cargo, Docker build, testes Asterisk, chamadas de voz, comparação de UI do novo aplicativo ou validação de ELF. Não existem ainda Cargo.toml próprio, Dockerfile, compose ou aplicativo compilado. Todos os gates runtime do [test-plan](test-plan.md) permanecem pendentes. O próximo trabalho, após aprovação, começa pelas tarefas de G0 e pelo laboratório Asterisk.

## Revisão de escopo rust-microsip

O usuário confirmou nome/diretório rust-microsip, UI compacta MicroSIP com GOnnect complementar, biblioteca de plataforma preparando macOS/Windows, CLI/MCP para agentes e web com chamadas completas em release posterior. Pasta local e repositório privado foram renomeados; base permaneceu local.

MicroSIP source/installer/Lite foram baixados para base; Lite abriu em Wine/Xvfb num container descartável sem rede, contas ou dispositivos pessoais. Captura visual inspecionada, sem chamadas ou teste de áudio; detalhes/hashes em [reference-sources](../../../docs/reference-sources.md). Ajuda oficial adicionada como referência de comportamento, não promessa de paridade.

Três mudanças completas de planejamento (4/4 cada), validadas individualmente pelo OpenSpec strict:
- create-native-rust-linux-release: desktop Linux compacto, plataforma e persistência.
- add-agent-cli-mcp: cliente CLI, service proprietário opcional e MCP stdio, autorizados e sem duplicação SIP/SQLite.
- add-web-wasm-softphone: release posterior, UI Rust/WASM com integração browser declarada, WSS/WebRTC e PBX/gateway.

Os gates das extensões estão nas specs/tarefas respectivas. As rodadas antigas de Claude não aprovam automaticamente o novo escopo. Terceira rodada CLI: **REVISE**, [relatório](../../../docs/reviews/claude-proposal-round-3.md), entrada SHA-256 `867b53b47f5e7bc07d95df88c10a15987f4c71cb60411c03a5d543ec673e4648`. Seis bloqueadores novos foram corrigidos: aprovação fora do protocolo e threat model honesto sobre mesmo UID; namespace/fluxo pendente de idempotência; leitura de resultado entre epochs sem reexecução; service/URI/GUI sem canal falso; HMAC efêmero para DTMF; aprovação antes G0 e gates AG antes do release.

Quarta rodada CLI: **APPROVE**, sem bloqueador concreto remanescente, [relatório](../../../docs/reviews/claude-proposal-round-4.md), entrada SHA-256 `27d5aa3522b9055339478c30f0bb207cd22cfd0e56ae34188b6a307513aa933a`. Ambos processos terminaram exit code 0, subtype success/is_error false, com tools desabilitadas, MCP vazio, hooks/settings isolados e nenhuma persistência. Observações não bloqueadoras foram explicitadas depois: awaiting_approval expira após restart; erro bootstrap_consumed; renovação de grant exige nova sessão; parte humana AG-06 fecha em G4 com recibos por modo/hash; proxy WSS pertence à infraestrutura do operador.

Auditoria atual: nove specs, 58 IDs únicos, 84 tarefas pendentes; links relativos e symlinks de skills íntegros, nenhuma base/submódulo no índice. Snapshot limpo dos documentos atuais foi validado individualmente para as três mudanças sem base. git diff --cached --check aprovado. Revisão e smoke Wine não provam aplicação ou telefonia; todos os testes runtime continuam pendentes.
