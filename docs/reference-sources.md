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

## MicroSIP: referência principal da UI compacta

Adição de 2026-10-12, após pedido de renomear o produto para rust-microsip. Referências oficiais:
- https://www.microsip.org/ .
- https://www.microsip.org/help : modos, discador, contatos, conta e ajustes; o modo de chamada única orienta o fluxo inicial, sem herdar todos os recursos ou permissões de URI.
- https://www.microsip.org/source : arquivo incremental de fontes; dependências de compilação não estão incluídas.
- https://www.microsip.org/download/MicroSIP-3.22.16-src.7z .
- https://www.microsip.org/download/MicroSIP-3.22.16.exe .
- https://www.microsip.org/download/MicroSIP-Lite-3.22.16.zip .

Downloads HTTPS preservados somente em `base/microsip/downloads/`:

| Arquivo | SHA-256 |
|---|---|
| MicroSIP-3.22.16-src.7z | `9c1ff942ae1ae56bdfcdd04c3d6e242a056d7f0dd50bb19f6e13226e7c819b83` |
| MicroSIP-3.22.16.exe | `e351bbac5e0337091cc0e55a80715208e5550e758028f36efa266c604304daaa` |
| MicroSIP-Lite-3.22.16.zip | `17dcaa628dfd1ea4ba6686c436fb62f42eb9ed124cd671c9d81f3d1580e1207e` |

Arquivo fonte extraído em `base/microsip/source/MicroSIP-3.22.16-src/`, após verificação de caminhos do archive. Site declara GPLv2; headers C++ examinados declaram GPL-2.0-or-later, portanto revisar cada componente antes de incorporação. Não alegar que o pacote incremental contém PJSIP e todas as dependências. `res/dialog.rc2` declara Microsoft Sans Serif; varredura do source extraído não encontrou arquivos TTF/OTF/WOFF. Fonte Windows não será redistribuída; Rust usará fonte aberta licenciada de proporções semelhantes.

Grafo de referência `gonnect-rust-microsip-reference`, fast, geração `2026-10-12T21:51:48Z`: pesquisa localizou CmainDlg.OnInitDialog, OnTcnSelchangeTab e operações do discador. Snippet de troca de abas confirma mostrar/ocultar página, foco, aba ativa e reset de perdidas em Calls. `check_index_coverage` verificou `mainDlg.cpp`, `Dialer.cpp`, `main.rc` e escopo `res`; os C++ têm macros parcialmente parseadas, complementadas por leitura direta dos intervalos reportados. Recursos RC não rastreados foram lidos diretamente, especialmente `main.rc` e `res/dialog.rc2`. Não usar o grafo como inventário completo de recursos/licenças.

MicroSIP Lite PE32 foi executado com Wine 9.0, prefixo win32 privado e Xvfb em container descartável sem rede, capabilities, privilégios, HOME/áudio/display pessoais ou conta SIP. Somente runtime `/usr` read-only e scratch dedicado foram montados. Primeira tentativa falhou no GLX/EGL Nvidia; nova execução com GLX desabilitado/software gráfico abriu janela MicroSIP 243×459 com abas Phone/Logs/Contacts, campo de número, teclado e status. Captura `base/microsip/wine-runtime/microsip-wine.png` inspecionada localmente; evidência não enviada ao GitHub. Sem window manager, sem placa de áudio e com avisos ALSA esperados; sem teste de chamadas, temas ou interoperabilidade. Instalador completo baixado, mas não executado. Container removido e Wine encerrado.

## Renomeação e extensões

Repositório privado renomeado de `jniltinho/gonnect-rust` para `jniltinho/rust-microsip`; diretório movido para `/home/nilton/Projetos/nilton/NOVOS/rust-microsip`, com base preservada e ignorada. Nomes antigos de grafos/consultas ai-memory acima são proveniência histórica, não identificadores atuais inferidos. A mudança de caminho torna os caminhos antigos do índice históricos; antes de nova exploração estrutural, reindexar/conferir freshness e coverage no caminho novo. Não renomear/migrar memória ou apagar índices históricos sem necessidade.

Fontes oficiais adicionais para os contratos planejados:
- MCP stdio: https://modelcontextprotocol.io/specification/2025-11-25/basic/transports ; stdout exclusivo de mensagens, stderr para logs.
- SDK MCP Rust: https://github.com/modelcontextprotocol/rust-sdk ; candidato sujeito a versão/auditoria.
- Sinalização web candidata: https://github.com/onsip/SIP.js ; SIP sobre WebSocket, WebRTC, espera/transfer e DTMF precisam de prova no nosso bundle.
- Browser: https://developer.mozilla.org/en-US/docs/Web/API/MediaDevices/getUserMedia ; contexto seguro e permissão de microfone.
- Asterisk WebRTC: https://docs.asterisk.org/Configuration/WebRTC/Configuring-Asterisk-for-WebRTC-Clients/ ; WSS e endpoint WebRTC, sem copiar exposição pública dos exemplos.

CLI/MCP foram solicitados para operação por agentes; web foi confirmada como chamadas completas em release posterior. A arquitetura prepara adapters macOS/Windows futuros; nenhum desses recursos está implementado.
