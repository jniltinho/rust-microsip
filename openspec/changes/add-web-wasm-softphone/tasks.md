## 1. Gate web GW0 após Linux

- [ ] 1.1 Confirmar Linux/CLI/MCP concluídos e aprovar release web separado; verificar roadmap/escopo e nenhuma promessa de web no v0.1.
- [ ] 1.2 Criar apps/web e separação de core/platform-web; verificar wasm32 sem CPAL/SQLite/Secret Service/IPC Linux e build Docker sem toolchain no host (WEB-01, WEB-06).
- [ ] 1.3 Fixar/auditar egui web, wasm-bindgen, SIP.js full framework e bridge; verificar ADR de versões/MSRV/licenças e nenhum CDN/recurso remoto implícito.
- [ ] 1.4 Criar laboratório Asterisk WebRTC/WSS/DTLS e CA isolada; verificar readiness, bind/ACL host-only, SDP e nenhuma rota PSTN/admin pública (WEB-05).
- [ ] 1.5 Criar cenário NAT/TURN forçado em VMs isoladas com coturn fixado e secrets efêmeros; verificar mídia relayed nos dois sentidos e teardown (WEB-05).
- [ ] 1.6 Provar GW0 em Chromium/Firefox: WASM/UI/CSP, registro/chamada, G.711, DTMF, hold, REFER/NOTIFY e permissões; verificar recibos por versão e bloquear UI completa se candidato falhar (WEB-01, WEB-02).

## 2. UI, dados e telefonia

- [ ] 2.1 Implementar UI compacta/temas/foco e diagnóstico browser; verificar snapshots do mock aprovado e teclado/escalas nas três abas (WEB-01).
- [ ] 2.2 Implementar adapter WSS/WebRTC e máquina de estados; verificar entrada/saída, recusa/BYE/CANCEL, DND, mute/hold/transfer e resultado real no PBX (WEB-02).
- [ ] 2.3 Implementar preparar áudio, permissões e seleção degradável de dispositivos; verificar autoplay/recusa/revogação/hotplug/indicador e nenhum microfone no carregamento (WEB-03).
- [ ] 2.4 Implementar lifecycle/lock por aba e reconciliação; verificar reload/close/suspensão, aba duplicada, sem rediscagem/takeover automático e tracks liberadas em melhor esforço (WEB-03).
- [ ] 2.5 Implementar IndexedDB não secreto e consentimento de contatos/histórico; verificar quota/modo privado, import/export/limpeza e ausência de senha/TURN em todos os stores/logs/URLs/console SIP.js e documentação sem garantia de zeroização (WEB-04).
- [ ] 2.6 Implementar validação de endpoints/URI, CSP/Permissions-Policy, proxy WSS obrigatório/Origin e erro de rede/TLS; verificar inputs maliciosos, origem WSS bloqueada e nenhum bypass de certificado (WEB-04, WEB-05).

## 3. Build, deploy e aceite

- [ ] 3.1 Completar bundle Docker com locks/checksums/SBOM/licenças; verificar clone sem base, assets self-hosted e ausência de segredos de build/runtime no bundle (WEB-06).
- [ ] 3.2 Documentar HTTPS, certificados, PBX/gateway, STUN/TURN e headers; verificar deploy isolado reproduzível sem app nativo e sem acesso a config/PBX pessoais (WEB-05, WEB-06).
- [ ] 3.3 Executar WEB-01..06 com versões congeladas de Chromium/Firefox, integração sintética e dois operadores/headsets; verificar cinco minutos por codec acordado no bundle exato, incluindo relay forçado (WEB-02, WEB-05, WEB-06).
- [ ] 3.4 Revisar com Claude CLI, security/dependency audit e operador; verificar zero bloqueadores, limites de background documentados e rollback de bundle/dados compatível antes de release separado.
