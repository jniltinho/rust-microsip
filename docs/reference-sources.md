# Fontes e limites das evidências

Levantamento de 2026-10-12. Este documento não comprova funcionalidade do futuro aplicativo.

## Bases locais

| Referência | Revisão ou identificação | Observação |
|---|---|---|
| GOnnect upstream | https://github.com/gonicus/gonnect ; checkout local `7c48ed6b5dc81e93458053d4de025bc3a4b28738` (`v2.4.1-12`) | `base/gonnect-native`; não é automaticamente a revisão do AppImage |
| Cópia local stable | `base/gonnect-native-stable` | Ponteiro de worktree quebrado após cópia de diretório. Não alegar revisão Git exata; arquivos identificados abaixo por hash |
| PhotoCraft | https://github.com/storytold/photocraft/tree/b3222ee8e7e6bcb4799c37f904c595f20f9baf5e | Referência de organização Rust, UI nativa egui/eframe, não base de telefonia |
| AppImage instalado | `GOnnect-2.4.1-ubuntu24.04-x86_64.AppImage` em `/home/nilton/Applications/` | SHA-256 `28a3ecb8692a7d7eadcb6ab69364fceb42048ba45f4192911e3b03ecce729cc4`; identificado sem executar, registrar contas ou acessar configuração pessoal |

Arquivos stable usados na análise (SHA-256):

- `src/Main.qml`: `0a684c3a33178c2612133a2751488155ba0f111d085e8f9141d9685b98ba8ad1`.
- `src/sip/SIPManager.h`: `1dcff0b33b79a4c9c57657324105b49b1141b7aba52f696c88ee7d173dd57bf4`.
- `src/sip/SIPCallManager.h`: `4d65ceab2a1fea379addb79b2df288d394b2baeb9951f6ffdca7aa201a3ef400`.
- `media/main-screen.png`: `4bf10bf45f431944cdb17ffe16beb5ba4c323ccd3e329b8ad482ef82f88b0abf`.

O screenshot público local foi inspecionado: navegação vertical à esquerda, busca superior, histórico central, agenda/favoritos à direita. É evidência do layout do projeto, não captura do AppImage instalado. A comparação exata com o AppImage, nas telas e nos dois temas, ainda precisa de uma sessão isolada com dados fictícios.

## codebase-memory-mcp

Indexado como `gonnect-rust`, modo fast, geração inicial `2026-10-12T21:30:11Z`; exploração dirigida Tier 2. Símbolos localizados pelo grafo e lidos por `get_code_snippet`:

- `gonnect-rust.base.photocraft.apps.photocraft.src.main.main`: inicialização da aplicação desktop, `eframe` e ligação com a UI.
- `gonnect-rust.base.gonnect-native-stable.src.sip.SIPManager.SIPManager`: fronteira Qt/PJSIP (`pj::Endpoint`).
- `gonnect-rust.base.gonnect-native-stable.src.sip.SIPCallManager.SIPCallManager`: superfície de chamadas/hold/DTMF/transferência/conferência.
- `gonnect-rust.base.gonnect-native-stable.src.Main`: janelas, diálogo de credenciais, falhas de registro e ciclo de fechamento.

`check_index_coverage` aplicado aos quatro arquivos e ao `Cargo.toml` do PhotoCraft. Os dois headers C++ têm parsing parcial, especialmente macros Qt: leitura direta integral realizada para complementar os trechos. `main.rs`, `Main.qml` e `Cargo.toml` sem lacuna registrada, sinal best-effort, não prova de completude. A navegação de chamadas de `main` foi exploratória, não usada para afirmação exaustiva de dependências.

Não foi feita auditoria completa das bases. O `Cargo.toml` lido confirma workspace, edição 2024, egui/eframe e licenças MIT/Apache-2.0; a nova aplicação não herdará automaticamente o MSRV nem as 24 crates do editor. Nenhuma base foi modificada ou incluída no repositório.

## ai-memory

Consulta do projeto `default/gonnect-rust` confirmou apenas o pedido, sem implementação anterior comprovada. Consulta global encontrou `default/site-linuxpro`, página `sessions/72aa78cd-c49a-4eca-a90b-bb5c314bcf98.md`: relato histórico distingue chamada no binário anterior de smoke test do AppImage reconstruído.

Aplicação desse contexto: exigir teste de áudio bidirecional no **artefato exato** do release, sem herdar aprovação de outro binário. Memória é evidência histórica não confiável; os resultados antigos não foram reexecutados nesta proposta. Não houve escrita manual de memória permanente.

## Documentação primária para decisões propostas

- GOnnect: https://github.com/gonicus/gonnect e arquivos locais `README.md`/`LICENSE` (GPL-2.0-or-later).
- PhotoCraft: https://github.com/storytold/photocraft e `Cargo.toml` local (MIT OR Apache-2.0).
- OpenSpec: https://github.com/Fission-AI/OpenSpec ; templates e validação do CLI instalado 1.10.0.
- UI nativa: https://docs.rs/eframe/latest/eframe/ .
- SIP candidato: https://github.com/restsend/rsipstack . O projeto descreve transportes e transações SIP; isso não comprova um softphone completo.
- Mídia candidata: https://github.com/restsend/rustrtc ; avaliar aderência SIP/SDES antes de adotar, sem assumir que WebRTC e SIP usam a mesma negociação.
- Áudio candidato: https://docs.rs/cpal/latest/cpal/ ; inventariar dependências Linux antes de fechar backend.
- SQLite embarcado: https://github.com/rusqlite/rusqlite .
- PBX de laboratório: https://docs.asterisk.org/ e https://github.com/asterisk/asterisk .

As bibliotecas acima são candidatas, não dependências já aprovadas ou lockadas. Gate técnico inicial decidirá versões, features, licenças e integração. A licença proposta para o futuro código é GPL-3.0-or-later, sujeita à revisão de compatibilidade antes de incorporar qualquer código/asset de terceiros; sem alegar parecer jurídico.
