## Context

Ver motivação em [proposal.md](proposal.md) e evidências em [reference-sources](../../../docs/reference-sources.md). O diretório inicial continha referências, não um workspace Rust próprio. GOnnect usa Qt/PJSIP nas superfícies examinadas; PhotoCraft demonstra uma separação Rust/egui que serve de inspiração, não um motor SIP reutilizável.

Este documento propõe a arquitetura. Não há prova ainda de que a combinação de bibliotecas candidatas satisfaça telefonia, mídia segura e empacotamento. O primeiro marco é uma prova vertical com critérios de interrupção, antes de investir na UI completa.

## Goals / Non-Goals

**Goals:** separar domínio de I/O; preservar responsividade; tornar telefonia testável sem GUI; manter um processo/executável de aplicação; auditar dependências reais em Linux; demonstrar chamadas no artefato distribuído.

**Non-Goals:** portar o motor PhotoCraft, traduzir Qt/QML linha por linha, embutir Asterisk, implementar um PBX, criar um framework genérico ou alegar que SQLite e drivers Linux são implementados em Rust puro. O código próprio será Rust; bibliotecas de sistema e SQLite nativo embutido são dependências auditadas. Escopo funcional e adiamentos permanecem em `proposal.md`.

## Decisions

### 1. UI nativa e arquitetura de comandos

Adotar egui/eframe como direção inicial, com renderização wgpu e fallback gráfico validado no gate. PhotoCraft orienta o workspace e a fronteira motor/UI; não copiar suas crates de imagem nem dependências privadas `craft-*`. Alternativas descartadas: Tauri/Electron por runtime web; GTK/Qt como UI principal por elevar dependências de toolkit e afastar a base Rust solicitada. Se teclado, acessibilidade básica ou fidelidade essencial falharem, revisar a decisão antes de implementar telas definitivas.

Estrutura **planejada**, não criada nesta etapa:

```text
apps/gonnect/          composição, CLI de diagnóstico e entrada do desktop
crates/core/          contas, contatos, estados, comandos, eventos e interfaces
crates/telephony/     adaptação SIP, registro, diálogos e transações
crates/media/         RTP/SRTP, codecs, jitter, resampling e dispositivos
crates/config/        fontes, validação, precedência e views sanitizadas
crates/storage/       SQLite, migrações e repositórios
crates/platform-linux/ Secret Service, notificações, tray, IPC e integração
crates/ui/            widgets, telas e tokens, sem SQL ou operações SIP diretas
crates/test-support/  fixtures fictícias e relógio/transporte controláveis
tests/                integração vertical e interoperabilidade
packaging/            build Linux, auditoria ELF e AppImage
docker/               builder e laboratório Asterisk
docs/                 arquitetura, desenvolvimento, segurança e release
```

`core` não depende de UI, SQLite ou adapters. Adapters dependem do domínio; `ui` recebe snapshots e despacha comandos; `apps/gonnect` liga implementações. Teste automatizado de dependências bloqueia ciclos e importação de infraestrutura na UI/domínio.

### 2. Concorrência e estados

Loop GUI na thread principal. Runtime Tokio em worker dedicado, canais de comandos/eventos com capacidade fixa e feedback de fila cheia; snapshots `watch` permitem coalescer atualizações de estado. Não descartar eventos finais de chamadas. Um ator de telefonia possui os diálogos e um worker exclusivo possui a conexão SQLite. I/O de disco nunca no callback de áudio nem no frame da UI.

Estados de registro: desabilitado → registrando → registrado → recuperando/falha. Estados de chamada: ocioso → discando/tocando → estabelecido ↔ em espera → encerrando → encerrado/falha. Transferência é uma operação pendente vinculada ao diálogo, confirmada por NOTIFY. Cada evento tem identificador de sessão e versão para deduplicação; timers usam relógio monotônico. Horários históricos são UTC, renderizados no fuso local; duração não depende de mudanças no relógio de parede.

Callbacks de áudio usam buffers circulares pré-alocados, sem mutex bloqueante, logging por pacote ou alocação por frame. `CancellationToken` e supervisão de tasks controlam shutdown; aguardar joins com timeout, em vez de sleeps arbitrários. Estados de saída normal e interrupção precisam ser distinguíveis no próximo início.

### 3. Telefonia e gate técnico obrigatório

`rsipstack` é a primeira candidata de sinalização, atrás de adapter estreito. O parser isolado `rsip` não seria uma stack completa; PJSIP FFI não é a direção padrão porque preservaria um motor C/C++ para a função principal. `rustrtc`/componentes Rust de RTP e SRTP são candidatos de mídia; validar SDES de SIP explicitamente, sem assumir compatibilidade por suporte WebRTC. Não inventar criptografia. G.711 é suficiente no primeiro release; Opus e processamento avançado de eco podem vir depois. Perfil inicial de qualidade é headset, sem prometer viva-voz com cancelamento de eco acústico.

**Gate G0, bloqueante:** uma prova vertical deve registrar em Asterisk por UDP/TCP/TLS, completar chamada G.711 nos dois sentidos, testar PCMA/PCMU, DTMF, espera, REFER/NOTIFY e TLS+SRTP/SDES. O ELF do spike inclui a mesma pilha eframe/wgpu, áudio, D-Bus/notificações e IPC prevista para produção, não apenas telefonia headless. Deve abrir uma janela mínima em GNOME/Wayland e KDE/X11, exercitar renderer de compatibilidade sem Vulkan dedicado, foco/teclado, enumeração/hotplug de áudio e gerar a auditoria de dependências da allowlist abaixo. Falha de teclado ou renderer bloqueia a decisão egui antes da UI completa. Comparar candidatos e fixar versões/features em `Cargo.lock` e ADR. Se falhar, registrar o bloqueio e revisar esta mudança; não substituir por Qt/PJSIP, remover segurança ou trocar binário único por AppImage sem revisão do escopo.

Perfil seguro default: TLS com hostname/CA validados, SRTP obrigatório e SDES apenas sobre TLS. CA corporativa por caminho explícito. Perfil de laboratório inseguro pode usar UDP/TCP+RTP somente por escolha explícita e aviso. Não registrar SDP com chaves, Authorization ou conteúdo de áudio.

STUN, rport e RTP simétrico atendem topologias restritas; PBX ancorando mídia ou VPN é recomendado. ICE/TURN, DTLS-SRTP e NAT simétrico arbitrário ficam fora do contrato inicial.

### 4. Configuração, segredos e armazenamento

Resolver campos em seis camadas, conforme `config-storage`: flags > ambiente > `.env` explícito > TOML > SQLite > defaults. `--config` explícito substitui a fonte TOML XDG, não mescla dois arquivos. Caminhos default: `$XDG_CONFIG_HOME/gonnect-rust/config.toml`, `$XDG_DATA_HOME/gonnect-rust/gonnect.sqlite3`, estado/logs em `$XDG_STATE_HOME/gonnect-rust/`; usar defaults XDG quando variáveis ausentes. Sem `.env` implícito do cwd. Namespace de ambiente `GONNECT_`, chaves e schema versionados. Configuração inválida interrompe o startup de SIP, mas deixa diagnóstico/ajustes acessíveis.

Guardar um `StoredConfig` durável no SQLite e construir `EffectiveConfig` imutável por composição. Nunca gravar o effective inteiro, que pode conter overrides e segredos. Mudanças de transporte/conta durante chamada exigem encerramento confirmado ou ficam pendentes, nunca derrubam áudio silenciosamente. CLI futura no mesmo executável: `--help`, `--version`, `--config`, `--env-file`, `--doctor`, `config validate`, `config import`, `config export`; JSON de diagnóstico sempre sanitizado. `--doctor` não registra ou disca.

Secret Service guarda senha; SQLite guarda `secret_ref`. Sem keyring: prompt por sessão ou arquivo privado/ambiente explícito, sem criptografia caseira com chave adjacente ao banco. Senha literal em TOML privado explicitamente selecionado é aceita somente em memória; recomendar `password_env`, nunca exportar o literal. Não oferecer senha em flag CLI (histórico e lista de processos). Permissões 0700/0600 incluem DB/WAL/SHM/backups e há recusa de arquivos secretos acessíveis por outros. `Debug` de tipos sensíveis oculta valores; exports nunca contêm segredo. Um TOML de exemplo pode usar `password_env = "GONNECT_SIP_PASSWORD"`, hostname `.invalid` e nenhum valor real.

SQLite via `rusqlite` com feature bundled, foreign keys, busy timeout limitado, queries parametrizadas e WAL. Worker exclusivo serializa escritas. Tabelas previstas: `schema_migrations`, `accounts`, `preferences`, `contacts`, `contact_numbers`, `favorites`, `calls`. IDs estáveis e FK; somente uma conta habilitada. Backup usa API consistente do SQLite, não cópia de arquivo aberto sem WAL. Migração transacional com teste de interrupção; downgrade exige backup compatível, não restauração destrutiva automática.

Histórico default 90 dias; exclusão remove registros ativos, sem alegar apagamento forense de SQLite/WAL/backups. CSV tem prévia, limites e transação: cada linha representa nome+número; nome igual agrega números distintos ao contato importado e duplicata exata nome+número é ignorada com contagem. Conflitos com contatos já existentes exigem decisão na prévia. Não há sincronização de agenda externa. DND usa resposta 486 Busy Here neste release.

### 5. Fidelidade visual mensurável

Referência primária de comportamento: AppImage instalado identificado por hash. Referência preliminar de layout: screenshot público do GOnnect examinado. Na implementação, executar a referência com HOME/XDG temporários e dados fictícios, sem carregar contas reais e sem chamadas externas. Não abrir automaticamente a instalação pessoal para capturar suas informações.

Matriz obrigatória: início/histórico, contatos/busca, discador, entrada, chamada ativa, espera/transferência, ajustes e erro de registro × claro/escuro; acrescentar sistema, teclado e escalas. Registrar diferenças e aprovação humana por tela. Compare geometria de elementos-chave com tolerância inicial de 4 px lógicos após normalização; antialiasing/texto variável não decide sozinho a aprovação. A igualdade pretendida é layout e fluxos essenciais; diferenças intencionais de funcionalidades adiadas ficam na matriz, com estado indisponível. Não copiar o botão de emergência como se houvesse garantia de emergência. Agenda permanece identificada como adiada.

Tokens centralizam cores, tipografia, espaçamento, raios e estados. Ícones e fontes embutidos com licença/atribuição. Fixtures sintéticas determinísticas e relógio fixo permitem snapshots; contraste e interação são testes separados da comparação de pixels.

### 6. Contrato de binário único e build Docker

Artefato principal: ELF `gonnect-rust` contendo código e recursos da aplicação. SQLite estático e assets embutidos; sem sidecars, subprocessos SIP, plugins de aplicação, download de fontes ou libs próprias soltas. **Não é promessa de ELF 100% estático nem Linux universal.** O host fornece kernel, loader/glibc, serviços gráficos/áudio e drivers. A allowlist inicial abrange somente bibliotecas do desktop limpo suportado: loader/glibc, interfaces X11/Wayland/xkbcommon, loader gráfico/driver de GPU e interfaces ALSA/PulseAudio conforme backend escolhido no G0. Manifesto final fixa nomes SONAME e comprova sua presença na imagem desktop de referência. Dependência fora da allowlist exige incorporação ao ELF ou revisão, não instrução oculta de `apt install`.

AppImage é conveniência adicional com mesmo núcleo e fallback sem FUSE documentado, não substituto do ELF. Auditar dependências indiretas e bibliotecas carregadas via `dlopen`, não apenas `ldd`/DT_NEEDED. Testar driver real e ambiente sem Vulkan dedicado; não exigir placa específica sem revisão de suporte.

Builder multi-stage Ubuntu 24.04 fixado por digest, toolchain stable fixado em `rust-toolchain.toml`, Cargo.lock com `--locked`; downloads de ferramentas por versão+SHA-256. Estágio de fetch seguido de build offline/frozen. BuildKit exporta `scratch`/artefatos, sem rodar UI em container e sem acesso a áudio/desktop do host. `.dockerignore` exclui `base/`, `.git`, `.env*` privados, bancos e artefatos. O host precisa de Docker Engine/BuildKit; não é possível compilar sem ferramenta alguma.

Comando **planejado, ainda não executável nesta fase**:

```sh
docker buildx build --target artifacts --output type=local,dest=dist .
```

Saída: ELF, AppImage, SHA256SUMS, SBOM, licenças/atribuições, manifesto ELF/runtime, commit/toolchain/digests e recibo de testes. Reprodutibilidade inicial significa entradas fixadas e auditáveis, não garantia não medida de identidade bit a bit. Adicionar comparação de rebuilds e documentar fontes de divergência. CI roda os mesmos caminhos do builder e só promove após gates.

### 7. Laboratório Asterisk em Docker

Criar na implementação `docker/asterisk/` com Dockerfile, configs fictícias, compose, runner e README. Base Asterisk 22 LTS, patch/source fixados por SHA-256 ou pacote de snapshot pinado após checagem de suporte. Não depender de imagem comunitária `latest`. Asterisk é componente separado de testes, nunca dependência embutida no cliente.

Topologia default: rede Compose `internal: true` com serviços `pbx`, `peer` e `test-runner`, sem `ports`, sem `network_mode: host`, sem socket Docker, sem acesso a HOME/DBus/dispositivos e sem privilégios. Runner testa o adapter/headless com fontes sintéticas; isso não substitui os testes desktop/hardware. PBX PJSIP com ramais `1001`/`1002`, rotas `600` echo, `601` URA/DTMF e `602` tom de referência; nenhuma rota de saída/PSTN, SIP anônimo ou admin HTTP/AMI remoto. Transferência de `1001` para ramal/rota válida e destino inexistente exercita sucesso/falha.

Gerador local cria senhas aleatórias e CA/certificados efêmeros em `artifacts/asterisk-lab/`, ignorado pelo Git. PBX e runner recebem somente esse diretório específico. Healthcheck prova readiness de PJSIP; runner verifica o registro, não apenas porta aberta. TLS em 5061, UDP/TCP 5060 e RTP UDP 10000–10019 internos. RTCP também coberto pela faixa. Sem gravação por default; qualquer captura para depuração usa apenas tráfego fictício e é excluída de publicação.

Perfil manual opt-in usa override `compose.host.yaml` que retira o PBX da rede interna default e o liga a uma bridge separada **não interna**, sem modo host, para permitir tráfego com o host. Publica somente `127.0.0.1:15060` → 5060 UDP/TCP, `127.0.0.1:15061` → 5061 TCP e `127.0.0.1:10000–10019` → mesma faixa UDP. Asterisk anuncia `127.0.0.1` na mídia/sinalização externa desse perfil, com `local_net`, `external_media_address`, `external_signaling_address`, porta externa e endpoints `direct_media=no`, `rtp_symmetric=yes`, `force_rport=yes`, `rewrite_contact=yes` ajustados à rede bridge. Validar o SDP nos dois sentidos: port mapping sozinho não resolve áudio. O perfil automatizado usa endereços/portas internos, sem herdar o anúncio loopback. Endpoints/fixtures distintos exercitam cada transporte+modo de criptografia e peer exclusivamente RTP, evitando mutação implícita de um único endpoint.

**Topologia G4 para VMs limpas:** PBX Docker roda numa VM auxiliar de laboratório, não nas VMs desktop de aceite. Um terceiro override opt-in `compose.vm-lab.yaml` substitui o anúncio/bind loopback por `$LAB_BIND_IP`, endereço da interface host-only da VM auxiliar, sem bridge com a LAN de produção, sem rota externa e com firewall permitindo apenas os IPs das VMs cliente/peer. Nunca bindar `0.0.0.0`. Essa exceção de rede é exclusiva do laboratório G4 e não altera os defaults internos/loopback. VMs desktop recebem só artefatos e dados fictícios, sem Docker, Rust ou pacotes extras. Operador A usa o aplicativo na VM limpa com headset; operador B usa um softphone de referência numa segunda VM/estação de laboratório na mesma rede isolada, com seu próprio headset. O recibo identifica ambos os endpoints e os dois sentidos de voz sem dados pessoais. Peer sintético fica para automação, não substitui esse aceite humano.

**Impairment e NAT:** AUD-02 usa proxy UDP de mídia em espaço de usuário no runner para perda/jitter/reordenação com seed fixo, sem `tc` ou `CAP_NET_ADMIN` nos containers default. AUD-06 é teste manual reproduzível em topologia dedicada: VM cliente atrás de VM roteador Linux com NAT simples, PBX e servidor STUN do outro lado na rede isolada. Provisionar NAT somente na VM roteador descartável, nunca alterar rotas/firewall do host pessoal. Documentar endereços/SDP e bind local do STUN, gerar recibo por hash e remover a topologia ao terminar. Não afirmar que a rede Compose default já demonstra NAT/STUN. Nenhum serviço default recebe modo privileged ou capability de administração de rede.

Comandos de contrato **a implementar**, não executados nesta proposta:

```sh
docker compose -f docker/asterisk/compose.yaml up --build --abort-on-container-exit --exit-code-from test-runner
docker compose -f docker/asterisk/compose.yaml -f docker/asterisk/compose.host.yaml up --build pbx
LAB_BIND_IP=<ip-host-only-da-vm-auxiliar> docker compose -f docker/asterisk/compose.yaml -f docker/asterisk/compose.vm-lab.yaml up --build pbx
docker compose -f docker/asterisk/compose.yaml down --volumes --remove-orphans
```

Perfis host e VM são alternativas: nunca empilhar `compose.host.yaml` junto com `compose.vm-lab.yaml`. Fixar uma versão do Compose com suporte à substituição explícita de listas (`!override`/`!reset`) para que redes/ports defaults não permaneçam por merge. O teardown de cada perfil repete os mesmos arquivos `-f` utilizados no `up` e remove a rede correspondente. Na VM auxiliar, a restrição de acesso deve filtrar efetivamente tráfego encaminhado pelo Docker (DOCKER-USER quando backend iptables, equivalente quando nftables), não apenas INPUT/UFW; provar acesso negado de um IP não autorizado. AUD-06 inclui o IP externo da VM roteador na ACL de teste. Validar no perfil host que a origem vista através da bridge não impede reescrita correta do SDP por `local_net`.

Testes: registro/renovação em três transportes, dois sentidos, 401/407, ocupado, CANCEL/200 race, DTMF, hold, REFER/NOTIFY, TLS inválido, SRTP obrigatório e retomada após perda de rede. Teste automatizado de síntese analisa tom/RMS/frequência nos dois sentidos; só echo não prova dois endpoints independentes. Teste manual posterior confirma microfone e reprodução reais no ELF e AppImage exatos.

### 8. Qualidade, licença e publicação

`//!` explica cada crate/módulo; `///` documenta API pública, erros e exemplos compiláveis. `thiserror` nas bibliotecas, contexto no binário, nenhum `unwrap`/`expect`/`todo!` em caminhos de produção. `unsafe_code=forbid` no código próprio salvo adapter FFI isolado justificado e auditado; dependências nativas/unsafe de terceiros explicitamente inventariadas. Tokio com features mínimas, limites e cancelamento; nada de runtime async dentro do callback de áudio.

Gates: fmt, clippy warnings-as-errors, testes/doctests, rustdoc warnings-as-errors, cargo-deny/audit com vulnerabilidades avaliadas, propriedades sobre estados/config/URI/CSV, fuzz dos parsers/adapters, integração Asterisk, snapshots/teclado e smoke VM. Plano rastreável em [test-plan.md](test-plan.md). Budget inicial em VM 2 vCPU/4 GiB: startup p95 até 3 s após caches aquecidos, RSS ocioso até 200 MiB, UI interativa durante falha de rede; baseline medido antes de impor regressões. Cobertura alvo 80% de linhas em domínio/config/storage, nunca substitui cenários críticos de I/O.

Propor GPL-3.0-or-later para futuro código, em vez de assumir licença MIT ao reutilizar referência GPL. Auditar qualquer incorporação do GOnnect e a opção de licença de dependências; preservar avisos e não copiar marca/logo como produto oficial. Nenhuma fonte upstream foi copiada para implementação nesta etapa. A decisão de licenciamento entra no G0 antes de incorporar código/assets.

Repositório privado inicialmente, proprietário autenticado `jniltinho`; não alterar visibilidade sem instrução. Commit explícito só de planejamento/skills. Auditoria de índice antes de cada push: nenhum caminho `base/`, submódulo, segredo ou artefato pessoal. CI deve repetir esse bloqueio quando for implementado. Tags/releases só após gates; GitHub Actions não pode afirmar release usável por mera compilação.

## Risks / Trade-offs

- [Stack Rust ainda não comprovada para todo o fluxo SIP/SDES] → G0 vertical, ADR e revisão de escopo se falhar; não preencher lacunas com mocks de produção.
- [Bibliotecas de áudio/gráficos escondidas quebram “um binário”] → allowlist explícita, auditoria dlopen e chamadas em VMs limpas; AppImage testado separadamente.
- [egui não oferece a mesma acessibilidade/visual do Qt automaticamente] → validar navegação, contraste, escala e matriz essencial antes da expansão.
- [NAT/Docker gera chamada estabelecida sem áudio] → mídia ancorada, profiles internos/host separados, SDP e tons nas duas direções.
- [Sem AEC pode haver eco em alto-falantes] → headset como perfil de qualidade inicial, limitação publicada; não prometer viva-voz profissional.
- [Bandeja/portais variam] → degradação para janela acessível sem extensão obrigatória.
- [SQLite e logs contêm metadados pessoais] → permissões, retenção, sanitização, nenhum dado real em testes ou no GitHub.
- [Paridade total estoura o primeiro release] → mesma experiência nas telas essenciais com exceções registradas; integrações adiadas separadas.

## Migration Plan

1. Aprovar escopo e concluir G0 com recibos; somente então implementar.
2. Criar workspace, dados fictícios e laboratórios isolados; construir por fatias verticais testadas.
3. Usar identificador XDG `gonnect-rust` independente de `gonnect`; não ler nem alterar o legado.
4. Entregar release candidate privado e testar artefatos exatos em GNOME/Wayland e KDE/X11.
5. Liberar v0.1.0 apenas com checklist, licenças e testes completos. Usuário decide eventual visibilidade pública.
6. Rollback: restaurar binário anterior com backup compatível do perfil Rust; recusar schema futuro, preservar o banco atual. A instalação GOnnect original permanece disponível e intocada.
