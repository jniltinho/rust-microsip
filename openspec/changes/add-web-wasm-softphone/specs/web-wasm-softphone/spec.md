## Purpose

Permitir chamadas de voz completas no navegador sem instalar cliente nativo, usando uma interface Rust/WASM, transporte web seguro e consentimento para acesso a áudio.

## ADDED Requirements

### Requirement: Acesso web independente
O produto SHALL carregar por HTTPS sem aplicativo Linux, extensão ou plugin instalado. SHALL oferecer UI compacta Discador/Chamadas/Contatos com temas claro/escuro/sistema, teclado e mensagens de incompatibilidade. Release web SHALL ser posterior e independente do v0.1 Linux.

#### Scenario: WEB-01 navegador limpo
- **WHEN** um usuário abre o endereço HTTPS num perfil limpo de Chromium ou Firefox desktop Linux da matriz publicada, sem cliente nativo
- **THEN** a UI WASM carrega e permite configurar sessão, trocar temas e navegar por teclado sem instalar software

### Requirement: Telefonia web completa
O cliente SHALL registrar uma conta SIP por WSS e suportar uma chamada simultânea via WebRTC DTLS-SRTP, interoperando com PBX compatível ou gateway. SHALL permitir discar/receber/aceitar/recusar/encerrar, DTMF, mute, espera, DND e transferência cega confirmada, sem downgrade para WS/RTP inseguros. SHALL negociar ao menos um codec interoperável validado por navegador com G.711 no PBX.

#### Scenario: WEB-02 chamada e controles
- **WHEN** duas sessões/ramais de laboratório fazem e recebem chamada e exercitam controles no navegador suportado
- **THEN** PBX confirma registro/DTMF/espera/REFER-NOTIFY/DND e ambos participantes ouvem voz por cinco minutos, com resultado real no histórico

### Requirement: Áudio consentido e ciclo da aba
O cliente SHALL solicitar microfone somente após ação consciente de preparar áudio ou aceitar/originar chamada; SHALL mostrar captura ativa, tratar recusa/ausência/hotplug e parar tracks quando não necessários. SHALL informar limites de autoplay, aba suspensa/fechada e notificações; MUST NOT prometer recebimento permanente com navegador fechado.

#### Scenario: WEB-03 permissão e suspensão
- **WHEN** usuário recusa/revoga microfone, remove headset, fecha/recarrega ou suspende aba durante sessão
- **THEN** erro/limitação fica visível quando a UI estiver ativa, nenhuma captura alternativa inicia silenciosamente e recursos são encerrados em melhor esforço, sem rediscagem automática

### Requirement: Credenciais e armazenamento web
O cliente SHALL manter senha SIP e credenciais TURN somente na memória da sessão, sem localStorage, sessionStorage, IndexedDB, URL, logs, service worker ou assets. SHALL persistir apenas preferências e, por escolha explícita, contatos/histórico locais, com limpeza/export sanitizado. SHALL distinguir dados locais por origem/perfil e não importar automaticamente SQLite/TOML/.env do desktop.

#### Scenario: WEB-04 armazenamento e limpeza
- **WHEN** usuário configura conta, chama, escolhe salvar dados locais e depois encerra sessão/limpa armazenamento
- **THEN** senha não aparece em armazenamento/export/log, captura/registro são encerrados em melhor esforço e dados consentidos podem ser removidos, sem prometer apagamento forense ou persistência no modo privado

### Requirement: Rede e implantação explícitas
A distribuição SHALL documentar HTTPS, WSS, certificados, PBX WebRTC/gateway, ICE/STUN/TURN e acesso de rede necessários, incluindo emissor/gerador externo de credenciais temporárias quando usar TURN e reverse proxy WSS com validação de Origin; não SHALL prometer acesso direto SIP UDP/TCP pelo WASM ou funcionamento de chamadas offline. Configuração pública MUST NOT conter segredos. Laboratório SHALL isolar contas e mídia e não expor admin PBX, PSTN ou ambiente pessoal.

#### Scenario: WEB-05 conectividade e NAT
- **WHEN** usuário usa PBX legado sem WSS/WebRTC, certificado inválido, rede bloqueada ou cenário de laboratório que exige relay TURN
- **THEN** recebe diagnóstico honesto sem bypass de TLS e o cenário TURN suportado completa mídia nos dois sentidos sem publicar credenciais permanentes; credencial efêmera vem de infraestrutura externa documentada e logs SIP/SDP/Authorization estão desabilitados/sanitizados

### Requirement: Build e aceite próprios
O projeto SHALL oferecer build Docker fixado exportando assets web, checksums, licenças/SBOM e guia de hospedagem HTTPS. Release SHALL exigir testes em versões fixadas/publicadas de Chromium e Firefox Linux, teclado/temas, duas pontas reais, privacidade e teste do deploy exato. Aprovação desktop ou fake media MUST NOT substituir áudio humano no navegador.

#### Scenario: WEB-06 clone e deploy limpos
- **WHEN** operador compila clone sem base com Docker e publica o bundle em ambiente de laboratório HTTPS
- **THEN** browsers limpos usam os assets verificados, passam os cenários WEB-01..05 e voz real possui recibo por hash/versão, sem instalar o cliente Linux
