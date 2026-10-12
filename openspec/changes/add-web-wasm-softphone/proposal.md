## Why

Usuários precisam fazer chamadas sem instalar um cliente. A opção confirmada pelo usuário é um softphone web completo em release posterior, não um painel remoto que dependa do aplicativo Linux instalado.

## What Changes

- Entregar UI Rust compilada para WASM, compacta como o desktop, com temas claro/escuro/sistema, contatos e histórico locais.
- Usar WebRTC do navegador para áudio e SIP sobre WSS para sinalização a PBX compatível, com ponte mínima para APIs web.
- Oferecer uma conta/chamada, discar/receber/aceitar/recusar/encerrar, DTMF, mute, espera, DND e transferência cega conforme gate de interoperabilidade.
- Publicar assets estáticos por HTTPS; nenhuma instalação do cliente nativo, extensão ou plugin.
- Documentar requisito de PBX WebRTC ou gateway separado para redes SIP legadas, STUN/TURN quando necessário e limites de chamadas em segundo plano.
- Manter credenciais em memória por sessão; usar armazenamento web não secreto, sem transportar configurações/SQLite nativos silenciosamente.
- Criar build Docker/deploy e laboratório Asterisk WebRTC próprios, com testes de navegador e voz real.

## Capabilities

### New Capabilities

- `web-wasm-softphone`: softphone web sem instalação, WebRTC/WSS, permissões, privacidade, operação e distribuição.

### Modified Capabilities

Nenhuma spec canônica existente. Contrato separado, sem reduzir os requisitos desktop.

## Impact

Release posterior ao v0.1 Linux e CLI/MCP; depende de domínio compartilhável, contratos de plataforma e gate WebRTC próprio. Não prometer SIP UDP/TCP, SQLite nativo, keyring, tray, escuta permanente ou execução offline de chamadas no navegador. Assets web são múltiplos arquivos, não o ELF único. Implantação precisa de HTTPS e PBX/gateway acessível; não embutir servidor SIP no WASM. macOS/Windows nativos continuam roadmap separado. Nenhum código web/deploy implementado nesta etapa.
