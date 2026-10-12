## Context

Escolha explícita: chamadas completas na web em release posterior, não controle remoto do desktop. Ver [proposal.md](proposal.md) e [spec](specs/web-wasm-softphone/spec.md). Referência de implantação: [Asterisk WebRTC](https://docs.asterisk.org/Configuration/WebRTC/Configuring-Asterisk-for-WebRTC-Clients/). Desktop Linux ainda está em planejamento.

## Goals / Non-Goals

**Goals:** compartilhar domínio e linguagem visual; zero instalação de cliente; contrato browser próprio e interoperabilidade.
**Non-Goals:** compilar adapters Linux cegamente para WASM, transportar UDP/TCP no browser, capturar áudio em background garantido, servidor SIP em WASM, app mobile/Safari certificado neste primeiro release web ou prometer tudo Rust sem código de integração JS.

## Decisions

### Fronteiras Rust/WASM e browser
apps/web usa UI egui/eframe web como candidata e core sem dependências de SO; wasm-bindgen/web-sys liga APIs de navegador. platform-web implementa tema, lifecycle/permissões e armazenamento não secreto; capabilities explicitam ausência de tray/keyring/IPC nativo. Domínio/UI puros são compartilháveis; Tokio/CPAL/rusqlite/Secret Service e IPC Unix não entram no bundle WASM. Não congelar o domínio por necessidade de uma stack de rede específica.

SIP.js full framework é candidata para sinalização SIP/WSS e session description WebRTC, isolada por bridge JS tipada e mínima, fixada/auditada no build. UI/regras próprias continuam Rust/WASM; APIs WebRTC, codecs e captura são do navegador, não RTP/SDES portados do desktop. Não prometer uma stack browser inteiramente Rust. Gate GW0 antes da UI completa prova registro, entrada/saída, G.711, DTMF, re-INVITE hold e REFER/NOTIFY nos dois browsers. Se a biblioteca ou UI candidata falhar, revisar esta proposta/ADR; não omitir funcionalidades ou recorrer ao aplicativo nativo silenciosamente.

### Infraestrutura de telefonia
Assets estáticos em HTTPS; SIP WSS direto a Asterisk/WebRTC habilitado ou gateway do operador, nunca UDP do browser. WebRTC usa ICE e DTLS-SRTP com RTP/SAVPF/rtcp-mux; não SDES como desktop. PBX ancora mídia e interopera com endpoint G.711; preferir PCMA/PCMU quando negociáveis, permitir Opus no browser com conversão PBX explicitamente auditada. PBX/gateway precisa fornecer codecs válidos; recursos do servidor não estão no bundle.

Laboratório Asterisk separado do default desktop: override exclusivo de WebRTC, WSS/TLS e ICE/DTLS, rede host-only em VMs de teste, ACL explícita e sem PSTN. Não copiar binds públicos dos exemplos da documentação. CA de laboratório confiada somente nos perfis/VMs de teste. Para WEB-05 criar coturn fixado, credenciais efêmeras geradas localmente e rede NAT roteada em VMs isoladas que bloqueia mídia direta para comprovar uso de relay. Não adicionar privilégios ao runner default. Deploy produção é do operador: configurar domínio/certificados, WSS, mídia e TURN, nunca fornecer relay aberto. Reverse proxy de WSS é obrigatório no deploy suportado para validar Origin por allowlist antes de encaminhar ao PBX; allowlist de destino WSS e rate limit; nenhum AMI/ARI/admin público. HTTPS da UI não elimina exposição do servidor SIP a autenticação/abuso.

### Credenciais, dados e segurança
Config pública contém endpoints/features/defaults não secretos; .env do build não é cofre. Conta/senha entram por formulário em memória, reconexão só enquanto sessão ativa; logout libera tracks e tenta BYE/unregister. Sem “lembrar senha”. Memória JS/WASM não permite garantir zeroização; encerrar sessão remove referências em melhor esforço, sem prometer apagamento físico da memória. Desabilitar logs internos SIP.js/SDP/Authorization e redigir erros antes de console/telemetria; teste de WEB-04 inspeciona console e buffers de debug, não apenas stores. TURN é fornecido pelo operador por sessão com curta validade; quando TURN for necessário, emissor/gerador autenticado de credenciais temporárias é infraestrutura externa obrigatória. No laboratório, operador gera ticket temporário no coturn e o informa ao formulário de sessão, sem segredo mestre no browser; produção pode integrar emissor autenticado existente, fora do bundle, nunca chave mestra no cliente. Sem backend da aplicação obrigatório; proxy WSS do operador é obrigatório no deploy suportado, mas relay/provisionamento e gateway são serviços externos quando necessários.

Preferências em IndexedDB; contatos/histórico somente com consentimento opt-in e aviso de armazenamento por origem/device, nunca sincronização implícita. Modo privado/quota/eviction degradam para sessão e aviso. Não usar SQLite WASM no primeiro web release, pois não traz vantagem para esse contrato e requer infraestrutura adicional; SQLite permanece exclusivo do app nativo. Import/export web sanitizado, com prévia e sem credenciais; limites CSV do domínio mantidos.

Hospedar sem scripts terceiros/analytics/CDN, escapes de nomes/labels, CSP restrita connect-src a endpoints aprovados, wasm-unsafe-eval esperado para carregar WASM, confirmado/justificado no ADR e testado no bundle, sem unsafe-eval amplo, frame-ancestors none e Permissions-Policy microphone=(self), camera=(). Incluir headers de segurança no exemplo HTTPS; testar CSP com bridge e bundle reais. Nada de service worker/PWA no primeiro release web; chamadas requerem rede. Não afirmar proteção de credenciais contra XSS/origem comprometida, mitigar por CSP e dependências fixadas.

### Permissões, dispositivos e lifecycle
Ação “Preparar áudio” informa captura e solicita microfone; sem ela, pedir ao aceitar/originar. Não capturar no simples carregamento/registro. Toque/reprodução podem exigir gesto de usuário; UI pede habilitar áudio e mostra aviso de recebimento limitado. getUserMedia/listagem condicionadas à permissão; seleção de saída via setSinkId é opcional e usa default do navegador quando não suportada, sem prometer seleção uniforme. Tracks encerradas ao terminar teste/chamada; reuso só com consentimento e indicador ativo. Hotplug não troca silenciosamente captura.

Sem garantir BYE em close/crash: best effort, PBX com timers e interface marca chamada interrompida na retomada, não atendida ficticiamente. Sem background incoming garantido, autostart/tray/notificação OS obrigatórios. Aba retomada reconcilia estado/registro, nunca rediscagem. Uma aba ativa por perfil via Web Locks quando disponível nos browsers suportados; aba concorrente fica leitura/aviso e não registra; queda libera lock, sem takeover de chamada automaticamente.

### Build e validação
Builder Docker pinado inclui Rust wasm32, ferramenta de bundle fixada e lockfile JS caso SIP.js adotado; nenhum Node/Rust instalado no host além de Docker. Bundle self-hosted gera HTML/JS/WASM/fontes/ícones e manifesto/checksums/SBOM/licenças; não “um binário web”. Testar sem COOP/COEP obrigatório inicialmente; se threads/shared memory forem necessários, revisar deploy e gate antes de adotá-los. Não prometer bundle universal ou latência/RSS do ELF.

Matriz inicial web: Chromium e Firefox estáveis em desktop Ubuntu 24.04, versões exatas congeladas no aceite; áudio real com headset e segundo endpoint humano, cinco minutos por codec acordado, e integração sintética separada. GW0 resolve UI/renderização, WASM/bridge, TLS, permissões, G.711, hold/transfer, CSP e bundle. Release web só após GW0 e WEB-01..06; Claude revisa proposta e depois resultados, não substitui chamadas.

## Risks / Trade-offs

- [PBX legado não fala WebRTC] → gateway/PBX compatível explícito, custo/operação do servidor fora do cliente.
- [Browser suspende aba/autoplay] → gesto/preparar áudio e limitações visíveis, sem promessa de softphone permanente.
- [JS sinalização além de Rust/WASM] → bridge isolada, versões/licenças auditadas e GW0; não mascarar a dependência.
- [NAT requer relay] → teste de TURN forçado e guia de infraestrutura, sem garantia NAT universal.
- [Credenciais acessíveis à origem comprometida] → memória por sessão, CSP, sem terceiros e sem claims de cofre browser.

## Migration Plan

Concluir Linux/CLI/MCP antes deste release. Aprovar GW0; criar target e adapters web; validar PBX e browsers; implementar UI/dados; build/deploy; testes reais e revisão. Publicar assets versionados atomicamente e rollback do bundle com schema compatível, sem migrar dados nativos. Configuração pessoal/PBX produção nunca é usada nos testes.
