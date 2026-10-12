## Why

Criar um softphone Linux nativo em Rust, com a simplicidade visual do MicroSIP e referências complementares de telefonia/desktop do GOnnect, instalação simples e configuração local persistente. O primeiro release precisa fazer chamadas reais com áudio confiável, em vez de apenas reproduzir telas ou empacotar o aplicativo Qt existente.

## What Changes

- Criar `rust-microsip` v0.1.0 inicialmente para Linux x86_64, Ubuntu 24.04, GNOME/Wayland e KDE/X11; outros ambientes sem promessa de suporte neste release.
- Usar UI nativa Rust (egui/eframe como candidata), tomando PhotoCraft como referência de arquitetura, sem Electron, Tauri, webview ou runtime Qt.
- Criar janela compacta com abas Discador, Chamadas e Contatos, busca local, favoritos nos contatos, controles de chamada e ajustes inspirados no MicroSIP; oferecer temas claro, escuro e sistema. Exceções de escopo explicitamente indicadas, sem botões falsamente funcionais.
- Entregar uma conta SIP habilitada por vez e uma chamada simultânea, com UDP/TCP/TLS, registro, receber/originar/recusar/encerrar, DTMF, mute, espera, DND e transferência cega.
- Incluir mídia RTP/G.711 PCMA/PCMU, seleção de dispositivos, reconexão e perfil seguro TLS + SRTP/SDES; não prometer NAT universal.
- Oferecer TOML ou `.env`, UI de ajustes e SQLite incorporado, com precedência explícita e credenciais protegidas, nunca senha em claro no banco.
- Manter contatos locais/CSV, favoritos e histórico pesquisável; integrar notificações, bandeja quando disponível e links `tel:`/`sip:` confirmados pelo usuário.
- Planejar executável autossuficiente em recursos da aplicação, AppImage adicional, build Docker e laboratório Asterisk Docker com ramais fictícios.
- Organizar workspace Rust por domínio, UI, telefonia, mídia, configuração, armazenamento e contratos de plataforma com adapter Linux; exigir documentação, testes e revisão pelo CLI do Claude.

### Non-goals

Entrega Windows/macOS/mobile/web neste release; paridade completa de MicroSIP ou UC; múltiplas chamadas/contas simultâneas; conferência, transferência assistida, vídeo/Jitsi, RTT, chat, LDAP/CardDAV/CalDAV/EDS/M365, BLF, headsets HID/busylights, provisionamento remoto e atualização automática. Não prometer chamadas de emergência, gravação ou certificação. Não migrar silenciosamente dados/configuração do aplicativo instalado.

## Capabilities

### New Capabilities

- `native-desktop`: UI Linux compacta inspirada no MicroSIP nas telas essenciais, temas e estados honestos.
- `sip-telephony`: registro, ciclo de chamada, DTMF, espera, DND, transferência e recuperação.
- `audio-media`: áudio bidirecional, dispositivos, RTP, codecs e mídia segura.
- `config-storage`: precedência TOML/env/SQLite, edição, migrações e proteção de segredos.
- `contacts-history`: contatos, CSV, favoritos, busca, histórico e retenção.
- `linux-integration`: notificações, bandeja opcional, URI, instância única e encerramento.
- `reproducible-distribution`: binário, build Docker, laboratório Asterisk, testes e publicação segura.

### Modified Capabilities

Nenhuma; projeto novo, sem specs anteriores.

## Impact

Novo repositório independente `jniltinho/rust-microsip`, sem publicar `base/`. Gate técnico precisa comprovar a combinação Rust/SIP/mídia/áudio e o contrato de empacotamento antes da UI completa. Nenhum backend PBX será embutido no aplicativo: Asterisk é infraestrutura descartável de testes e o uso normal depende de um servidor SIP acessível. Fontes, limites e identificação do AppImage em [reference-sources](../../../docs/reference-sources.md). Esta mudança entrega planejamento; código e infraestrutura executável serão criados apenas na implementação aprovada.

## Evolução vinculada

macOS e Windows são alvos futuros, preparados por uma crate `platform`, sem promessa de suporte no v0.1. [CLI/MCP](../add-agent-cli-mcp/proposal.md) é uma extensão solicitada para agentes, planejada para o candidato Linux após G0; a aprovação/release do conjunto exige os gates das duas mudanças. [Web/WASM](../add-web-wasm-softphone/proposal.md) fará chamadas completas via WebRTC em release posterior, conforme escolha do usuário; não substitui o ELF Linux nem reutiliza sockets SIP nativos no navegador.
