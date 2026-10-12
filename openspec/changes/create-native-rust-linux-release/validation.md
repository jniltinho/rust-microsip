# Validação da proposta

Data: 2026-10-12. Escopo: documentos de planejamento, sem implementação do aplicativo.

## Verificações locais

- OpenSpec CLI 1.10.0: `openspec validate create-native-rust-linux-release --strict --no-interactive` aprovado.
- `openspec status`: proposta, specs, design e tarefas completos (4/4 artefatos de planejamento).
- Rastreabilidade conferida: sete specs, 46 cenários únicos, 46 entradas no plano de testes, 54 tarefas não executadas.
- `base/` ignorada por regra raiz `/base/`; nenhum arquivo dela está no índice. Nenhum submódulo de referência foi adicionado.
- Repositório `jniltinho/gonnect-rust` criado como privado. Somente documentos/skills aprovados para envio; nenhum release/tag de aplicativo criado.
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

Segunda rodada: **APPROVE**, sem bloqueadores documentais restantes, registrada em [round-2](../../../docs/reviews/claude-proposal-round-2.md). Entrada sanitizada identificada pelo SHA-256 `185fc546a12be27d1f23cbec7b9240751ff5118a0973aa6586a0196a9a9dd540`; processo terminou com exit code 0 e resultado success, sem erro. Observações não bloqueadoras sobre comando G4, merge de overrides, filtro de forwarding Docker e teardown foram explicitadas no design após a aprovação. Revisão não prova interoperabilidade nem execução do futuro laboratório.

## Limites

Não foram executados Cargo, Docker build, testes Asterisk, chamadas de voz, comparação de UI do novo aplicativo ou validação de ELF. Não existem ainda Cargo.toml próprio, Dockerfile, compose ou aplicativo compilado. Todos os gates runtime do [test-plan](test-plan.md) permanecem pendentes. O próximo trabalho, após aprovação, começa pelas tarefas de G0 e pelo laboratório Asterisk.
