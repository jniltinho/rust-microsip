## 1. Aprovação e prova vertical G0

- [ ] 1.1 Obter aprovação desta proposta e registrar limites do v0.1.0, verificando explicitamente uma conta/chamada simultânea, integrações adiadas e contrato ELF versus AppImage.
- [ ] 1.2 Criar scaffold mínimo Rust para a prova vertical com toolchain fixado e lockfile; verificar build limpo no builder Docker sem `base/` ou Cargo no host (DIST-03).
- [ ] 1.3 Inventariar licenças, fontes/ícones e dependências nativas; entregar ADR de compatibilidade e licença antes de copiar qualquer recurso upstream.
- [ ] 1.4 Criar PBX Asterisk Docker pinado com ramais 1001/1002, echo, URA e tons; verificar readiness PJSIP e ausência de portas publicadas no perfil default (DIST-04).
- [ ] 1.5 Criar gerador de segredos/CA fictícios, perfil host com bridge não interna/loopback e perfil G4 host-only em VM auxiliar; verificar `docker compose config`, binds/ACL, SDP, mídia ancorada e teardown sem acesso a dados pessoais (DIST-05).
- [ ] 1.6 Provar registro UDP/TCP/TLS e G.711 PCMA/PCMU nos dois sentidos com stack Rust candidata; anexar recibos PBX e medição de mídia sintética (SIP-01, SIP-03, AUD-01).
- [ ] 1.7 Provar DTMF, hold, REFER/NOTIFY e TLS+SRTP/SDES com casos negativos; aprovar adapter/versões apenas com testes verdes (SIP-06, SIP-08, SIP-09, AUD-05).
- [ ] 1.8 Gerar spike ELF com mesma pilha eframe/wgpu, áudio, D-Bus/notificações e IPC de produção; verificar janela, foco/teclado, hotplug e renderer compatível em GNOME/Wayland e KDE/X11, auditar DT_NEEDED/dlopen e bloquear G0 se exigir pacotes extras (DIST-01, ND-05, AUD-03).
- [ ] 1.9 Registrar decisão G0 e revisão read-only pelo CLI do Claude; verificar recibo vinculado aos commits/resultados, impedindo avanço para UI completa se houver bloqueador.

## 2. Workspace e contratos documentados

- [ ] 2.1 Estruturar crates e binário conforme design; verificar grafo de dependências sem ciclos e domínio sem UI/SQL/network adapters.
- [ ] 2.2 Implementar tipos/IDs, máquina de estados e erros tipados; verificar propriedades de transição e ausência de panic em entradas inválidas.
- [ ] 2.3 Implementar comandos/eventos com filas limitadas, deduplicação e supervisão; verificar saturação, cancelamento e retenção de eventos finais sem bloquear UI/áudio.
- [ ] 2.4 Adicionar rustdoc de crates/APIs, exemplos e README de arquitetura; verificar `cargo test --doc` e rustdoc com warnings como erros.
- [ ] 2.5 Criar fixtures, relógio falso e regra de camadas; verificar teste automatizado que rejeita dependência de infraestrutura no domínio/UI.

## 3. Configuração, banco e segredos

- [ ] 3.1 Implementar schema/validação e resolver CLI/env/dotenv/TOML/SQLite/defaults; verificar todas as combinações, origem e falha antes de SIP (CFG-01, CFG-02).
- [ ] 3.2 Implementar repositórios SQLite, permissões XDG e edição durável sem persistir effective config; verificar reinício, override e constraints de conta habilitada (CFG-03, CFG-04).
- [ ] 3.3 Integrar Secret Service e fallback efêmero; verificar scanner no DB/logs/export, arquivos secretos 0600 e ausência de keyring (CFG-05, CFG-06).
- [ ] 3.4 Implementar migrações transacionais e backup SQLite consistente; verificar interrupção, lock, disco cheio, corrupção e schema futuro sem perda de arquivo (CFG-07, CFG-08).
- [ ] 3.5 Implementar import/export TOML com prévia sanitizada; verificar atomicidade e legado intocado (CFG-09).
- [ ] 3.6 Implementar CLI `--doctor`, validação, import/export e mensagens; verificar JSON sem segredos e diagnóstico sem registro/chamada.

## 4. Telefonia completa do release

- [ ] 4.1 Consolidar registro, renovação e backoff por transporte no adapter escolhido; verificar 401/407, senha errada, conta duplicada e ausência de retry infinito (SIP-01, SIP-02).
- [ ] 4.2 Implementar receber/originar/aceitar/recusar/BYE/CANCEL; verificar entrada/saída e corrida CANCEL/200 OK sem sessão fantasma (SIP-03, SIP-04).
- [ ] 4.3 Implementar ocupado e DND com estado coerente; verificar segunda entrada preserva chamada ativa e DND não a interrompe (SIP-05, SIP-07).
- [ ] 4.4 Consolidar DTMF e re-INVITE hold/resume; verificar sequência reconhecida pela URA e restauração do áudio (SIP-06).
- [ ] 4.5 Consolidar transferência cega com NOTIFY; verificar sucesso final e destino inválido sem perder chamada original quando diálogo preservado (SIP-08).
- [ ] 4.6 Implementar validação TLS/CA customizada e recuperação rede/suspensão; verificar certificados negativos, retries limitados e nenhuma rediscagem automática (SIP-09, SIP-10).

## 5. Áudio e mídia de produção

- [ ] 5.1 Consolidar RTP, G.711, resampling e jitter buffer limitado; verificar loss/jitter/reorder por proxy UDP de usuário com seed fixo, codec incompatível e buffers limitados sem privilégios de rede no runner (AUD-01, AUD-02).
- [ ] 5.2 Implementar seleção de captura/reprodução/toque e persistência por ID; verificar headset, dispositivo ausente, permissão e confirmação de novo microfone no hotplug (AUD-03).
- [ ] 5.3 Implementar teste local e mute; verificar voz bloqueada no peer sem interromper recepção e nenhum bloqueio/alocação no callback (AUD-04).
- [ ] 5.4 Consolidar SRTP/SDES obrigatório no perfil TLS; verificar rejeição de downgrade e nenhum vazamento de chaves (AUD-05).
- [ ] 5.5 Implementar STUN/rport/keepalive/RTP simétrico e roteiro manual com VM roteador NAT e servidor STUN em rede isolada; verificar áudio nos dois sentidos, SDP e teardown sem mudar o host pessoal, documentando ausência de TURN (AUD-06).

## 6. Contatos e histórico

- [ ] 6.1 Implementar contatos, múltiplos números e favoritos SQLite; verificar persistência, busca e seleção antes de discar (CH-01).
- [ ] 6.2 Implementar prévia/importação CSV limitada e transacional; verificar duplicatas, UTF-8, tamanho e registros inválidos sem escrita parcial (CH-02).
- [ ] 6.3 Persistir resultados de sessão, duração monotônica e recuperação após crash; verificar unicidade por sessão e resultados reais (CH-03).
- [ ] 6.4 Implementar filtros, busca local, retenção 90 dias e limpeza confirmada; verificar zero tráfego de busca e remoção das entradas ativas (CH-04).

## 7. UI fiel com temas

- [ ] 7.1 Capturar referência em HOME/XDG isolados com fixtures, sem usar configuração pessoal; entregar matriz por tela/tema e registrar exceções de escopo (ND-01, ND-02).
- [ ] 7.2 Implementar shell de navegação/busca/cards e tokens; verificar snapshots e geometria essencial com diferenças explicadas (ND-01).
- [ ] 7.3 Implementar telas de contatos, discador, entrada, chamada/hold/transferência e erros, conectadas a comandos reais; verificar estados exibidos a partir de backend e funções adiadas indisponíveis (ND-02, ND-06).
- [ ] 7.4 Implementar ajustes validados e claro/escuro/sistema; verificar persistência, troca durante chamada e serviço de tema ausente (ND-03, ND-04).
- [ ] 7.5 Implementar foco, rótulos, navegação de teclado, contraste e layout escalável; verificar matriz 1024x768 e 100/150/200% com ações essenciais acessíveis (ND-05).

## 8. Integração Linux e shutdown

- [ ] 8.1 Implementar notificações e bandeja opcionais; verificar desktop com e sem suporte e entrada sempre acessível (LIN-01).
- [ ] 8.2 Implementar parsing de URI e integração opt-in; verificar confirmação, CR/LF, entradas longas e comandos shell rejeitados (LIN-02).
- [ ] 8.3 Implementar instância única e IPC local protegido; verificar segunda instância sem novo registro ou disputa destrutiva de banco (LIN-03).
- [ ] 8.4 Implementar fechar/ocultar/sair, confirmação e shutdown com prazo; verificar saída em chamada, SIGTERM, PBX sem resposta e áudio liberado (LIN-04).

## 9. Build, segurança e release candidate

- [ ] 9.1 Completar builder Docker fixado, fetch/build offline e export; verificar build de clone limpo sem pasta base e sem toolchain no host (DIST-03).
- [ ] 9.2 Implementar empacotamento ELF/AppImage, licenças e auditoria runtime; verificar separadamente ambos os artefatos, incluindo AppImage sem FUSE (DIST-01, DIST-02).
- [ ] 9.3 Implementar CI para fmt/clippy/test/doc, camadas, audit/deny e fuzz limitado; verificar falhas injetadas bloqueiam pipeline e toda API pública tem documentação.
- [ ] 9.4 Implementar scanner de publicação/artefatos e regra de proteção de `base/`; verificar testes negativos com caminho proibido ou segredo fictício (DIST-07).
- [ ] 9.5 Executar suíte Asterisk inteira com relatório por cenário e metadados de build; verificar todas as linhas SIP/AUD/CFG relevantes no plano sem aprovações herdadas.
- [ ] 9.6 Executar checklist manual no ELF e AppImage exatos em VMs GNOME/Wayland e KDE/X11 limpas, ligadas ao PBX auxiliar e segundo endpoint humano via rede G4 isolada; verificar headset, áudio bidirecional cinco minutos/codec, recibos por hash/tema/transporte e nenhuma instalação extra nas VMs cliente (AUD-01, DIST-01, DIST-02, DIST-06).
- [ ] 9.7 Medir startup p95/RSS/estabilidade e revisar matriz visual com operador; verificar budgets do design e nenhum defeito crítico de usabilidade/áudio.
- [ ] 9.8 Executar revisão final pelo CLI do Claude com contexto sanitizado; verificar relatório de bloqueadores e sua resolução, distinguindo revisão de evidência runtime (DIST-06).
- [ ] 9.9 Preparar SBOM, checksums, notas, limitações, guia Docker e rollback; verificar cada artefato corresponde ao commit/tag candidato e não contém dados pessoais (DIST-07).
- [ ] 9.10 Publicar v0.1.0 somente após todos os gates e decisão de visibilidade do proprietário; verificar checklist completo, download/checksums e ausência de promessa de paridade total.
