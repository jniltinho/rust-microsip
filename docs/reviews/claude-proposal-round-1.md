# Revisão independente da proposta OpenSpec

**VERDICT: REVISE**

A proposta é coerente no essencial. Escopo v0.1, non-goals, a escolha de Rust nativo com FFI apenas como exceção, o contrato ELF versus AppImage, a ordem de precedência da configuração, a política de segredos, o SRTP sem downgrade e a rastreabilidade (46 cenários, contagem confere) estão bem amarrados. Os pontos abaixo são lacunas documentais concretas: cenários de aceite que, como escritos, não podem ser executados na topologia descrita. Não são riscos que o G0 já cobre.

## Bloqueadores concretos

**1. O perfil host não diz como sair da rede `internal: true`**
- Onde: `design.md` §7 e `specs/reproducible-distribution/spec.md` (DIST-05).
- Problema: uma rede Compose `internal: true` não tem rota para o host. Por isso, `ports` adicionados só via override não ficam acessíveis, e o documento não diz que o `compose.host.yaml` troca ou acrescenta rede. O DIST-05 fica sem caminho executável.
- Sugestão: declarar que o override liga o `pbx` a uma rede bridge não interna, apenas com binds em `127.0.0.1`. Validar com `docker compose config` e com um teste de que nada escuta em `0.0.0.0`.

**2. A VM limpa não alcança um PBX ligado só ao loopback do host**
- Onde: `specs/reproducible-distribution/spec.md` (DIST-01), `test-plan.md` (matriz de execução e G4) e `tasks.md` 9.6.
- Problema: o DIST-01 exige chamada real a partir de uma VM limpa. O único perfil manual faz bind em `127.0.0.1` do host, que a VM não alcança. Instalar Docker dentro da VM "limpa" altera o ambiente de referência. Também não está definido quem é o segundo participante em "os dois participantes ouvem voz" (AUD-01).
- Sugestão: definir uma topologia própria de G4. Por exemplo, um PBX em VM ou host separado numa rede isolada só de laboratório, documentada como exceção controlada ao "somente loopback". Definir também o segundo endpoint (operador humano ou peer sintético) e o que o recibo registra de cada lado.

**3. AUD-02 e AUD-06 exigem manipular a rede, o que contradiz "sem privilégios"**
- Onde: `design.md` §7, `specs/audio-media/spec.md` (AUD-02, AUD-06) e `tasks.md` 5.1 e 5.5.
- Problema: perda, jitter e reordenação exigem `tc netem`, e NAT exige masquerade. Ambos pedem `CAP_NET_ADMIN`. Além disso, não há servidor STUN nem elemento de NAT na topologia (`pbx`, `peer`, `test-runner`). O teste de "NAT simples com STUN configurado" não tem onde rodar.
- Sugestão: escolher e documentar uma das duas saídas:
  - um proxy UDP de impairment em espaço de usuário mais um contêiner STUN e NAT, com a capability concedida só a esse serviço e justificada;
  - ou declarar AUD-06 como teste manual numa topologia descrita.

**4. O ELF do G0 não inclui a pilha de UI, então a auditoria de dependências não prova nada**
- Onde: `tasks.md` 1.8 versus `design.md` §1 ("fallback gráfico validado no gate") e §6.
- Problema: a tarefa 1.8 fala em "ELF mínimo". Nenhuma tarefa do G0 abre janela eframe/wgpu, testa o fallback sem Vulkan dedicado ou exercita teclado e AccessKit. Sem isso, a allowlist de SONAMEs e o `dlopen` de gráficos, Wayland/X11, D-Bus e áudio não são medidos, e a revisão da decisão egui prometida no §1 não tem gatilho.
- Sugestão: exigir que o ELF do G0 contenha a mesma pilha UI, áudio, D-Bus e notificação prevista. Ele deve abrir uma janela em GNOME/Wayland e KDE/X11 e alternar o renderer de fallback. Incluir um spike mínimo de foco e teclado com critério de interrupção.

**5. A matriz exige VM "sem Qt" e ao mesmo tempo um ambiente KDE**
- Onde: `test-plan.md`, linha "VM sem Rust/Node/Qt/dev packages adicionais".
- Problema: o Plasma já traz Qt instalado, então a VM KDE nunca satisfaz essa linha.
- Sugestão: reescrever como "sem pacotes adicionais além do desktop padrão" e "ELF sem dependência de Qt", verificado por `readelf` e `dlopen`.

## Riscos residuais

Estes não bloqueiam a aprovação.

**Já cobertos pelo G0:**
- Maturidade de `rsipstack` e da mídia Rust para SDES de SIP (sem WebRTC), re-INVITE com rekey SRTP e telephone-event sobre SRTP.
- Escolha do backend de áudio (ALSA, PulseAudio ou PipeWire) e seu efeito na allowlist.
- Compatibilidade de licença entre a GPL proposta e as dependências.

**Não cobertos pelo G0, mas aceitáveis como risco de implementação:**
- **Hotplug e persistência de dispositivo por ID (AUD-03):** com cpal sobre ALSA isso é frágil. Vale incluir enumeração e hotplug no spike do G0.
- **Secret Service no Kubuntu 24.04:** KWallet pode não expor `org.freedesktop.secrets` por padrão. O fallback por sessão vai aparecer com frequência no KDE e deveria estar na matriz.
- **Foco de janela em chamada recebida no Wayland:** o compositor pode negar foco (xdg-activation). A notificação passa a ser o caminho principal.
- **Laboratório Asterisk:** cada combinação de transporte e media encryption, e o peer só-RTP do AUD-05, exige endpoints PJSIP distintos. Hoje isso está implícito.
- **Permissões 0600:** devem cobrir também os arquivos `-wal`, `-shm` e de backup do SQLite.
- **Ambiguidades menores:**
  - se `--config` explícito suprime o TOML XDG;
  - se senha literal em TOML é aceita ou apenas `password_env` (CFG-06 sugere que é);
  - como o CSV de uma coluna "número" representa contatos com vários números versus duplicatas (CH-02);
  - qual código SIP o DND usa (486 ou 603).
- **Orçamento de RSS:** 200 MiB com wgpu sobre Mesa ou lavapipe pode ser apertado. O design já prevê medir um baseline antes.
- **Documentos não fornecidos:** `docs/reference-sources.md` e `validation.md` são citados mas não vieram junto. A identificação por hash do AppImage de referência e a validação estrutural não foram verificadas nesta revisão.

Nenhum teste de runtime foi executado. Esta revisão avalia apenas o planejamento.
