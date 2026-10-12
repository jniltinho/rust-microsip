# Plano de testes e critérios de aceite

**Estado: todos os testes do aplicativo estão pendentes; nenhum código Rust foi implementado nesta etapa.** Os nomes abaixo são contratos propostos, não testes existentes. Cada cenário OpenSpec tem um identificador estável e uma linha de rastreabilidade. A validação OpenSpec e a revisão do Claude avaliam o planejamento, não atestam funcionamento do produto.

## Gates e ordem

1. G0: prova vertical SIP + G.711 + TLS/SRTP + Docker/ELF; bloqueia a UI completa se não passar.
2. G1: domínio/configuração/persistência com testes RED → GREEN → refactor e doctests.
3. G2: suite Asterisk isolada com fluxo SIP real e mídia sintética nas duas direções.
4. G3: fluxo compacto MicroSIP, snapshots do mock Rust aprovado, teclado/temas e integração em GNOME/Wayland e KDE/X11.
5. G3b: contratos AG-01..05 e parte automatizada AG-06 de CLI/MCP (proprietário/IPC, schemas, autorização GUI/grants, ledger/restart, concorrência e controle Asterisk); bloqueiam candidato Linux com agentes.
6. G4: concluir AG-06 humano em GUI/service na mesma topologia host-only, com recibos separados por modo/hash; ELF e AppImage exatos em VMs limpas, headset, chamadas reais de cinco minutos por codec e recibos por hash.
7. G5: revisão independente Claude, licenças, segurança, SBOM, auditoria do Git e decisão de release.

## Matriz de execução

| Ambiente | Finalidade | Evidência obrigatória |
|---|---|---|
| Docker, sem ports, rede interna | PBX/peer/runner automatizados | readiness, estados SIP, resultados DTMF/transfer, métricas de tons |
| Docker perfil loopback opt-in, bridge não interna | PBX para aplicativo no host | binds locais, SDP correto, áudio ancorado |
| PBX Docker em VM auxiliar, perfil G4 host-only opt-in | chamadas das VMs limpas e peer humano | bind no IP explícito, ACL de laboratório, sem LAN/Internet |
| Ubuntu 24.04 x86_64 GNOME/Wayland | suporte primário desktop | teclado/temas/escala, chamadas ELF e AppImage |
| Ubuntu 24.04 x86_64 KDE/X11 | suporte primário desktop | mesmas provas, com e sem bandeja |
| VM sem pacotes adicionais além do desktop padrão | contrato de distribuição | ELF sem dependência de Qt, libs auditadas, first-run sem downloads de recursos, chamada real |

Testar UDP/TCP no perfil RTP explicitamente inseguro e TLS no perfil SRTP obrigatório, usando fixtures de endpoints separados. Testar PCMA/PCMU, certificado válido/expirado/CA desconhecida/hostname incorreto e ausência de Secret Service, incluindo KDE/KWallet sem serviço Secrets exposto. No G4, operador A usa a VM limpa/aplicativo/headset e operador B outro softphone de referência/headset na rede isolada; nenhum Docker/toolchain é instalado nas VMs cliente. O runner de sinalização não prova captura/reprodução do microfone físico. Não gravar conversas; fixtures e tons são sintéticos.

AUD-02 usa proxy UDP de impairment em espaço de usuário, sem administração de rede nos containers. AUD-06 exige topologia manual dedicada com VM roteador descartável/NAT simples, cliente atrás dela e PBX/STUN do outro lado na rede isolada; não é coberto pela rede default Compose. Registrar endereçamento e SDP, sem alterar rotas/firewall do host pessoal.

## Rastreabilidade de todos os cenários

| Cenário | Teste proposto | Método | Estado |
|---|---|---|---|
| [AUD-01](specs/audio-media/spec.md) — voz bidirecional | `test_aud_01_voz_bidirecional` | Integração de mídia + hardware real em VM | pendente |
| [AUD-02](specs/audio-media/spec.md) — rede degradada e codec incompatível | `test_aud_02_rede_degradada_e_codec_incompativel` | Integração de mídia + hardware real em VM | pendente |
| [AUD-03](specs/audio-media/spec.md) — seleção e headset removido | `test_aud_03_selecao_e_headset_removido` | Integração de mídia + hardware real em VM | pendente |
| [AUD-04](specs/audio-media/spec.md) — teste e mute | `test_aud_04_teste_e_mute` | Integração de mídia + hardware real em VM | pendente |
| [AUD-05](specs/audio-media/spec.md) — SRTP obrigatório | `test_aud_05_srtp_obrigatorio` | Integração de mídia + hardware real em VM | pendente |
| [AUD-06](specs/audio-media/spec.md) — topologia suportada | `test_aud_06_topologia_suportada` | Integração de mídia + hardware real em VM | pendente |
| [CFG-01](specs/config-storage/spec.md) — precedência e origem | `test_cfg_01_precedencia_e_origem` | Unitário/propriedades + integração SQLite/filesystem | pendente |
| [CFG-02](specs/config-storage/spec.md) — arquivo inválido | `test_cfg_02_arquivo_invalido` | Unitário/propriedades + integração SQLite/filesystem | pendente |
| [CFG-03](specs/config-storage/spec.md) — ajuste durável | `test_cfg_03_ajuste_duravel` | Unitário/propriedades + integração SQLite/filesystem | pendente |
| [CFG-04](specs/config-storage/spec.md) — override externo ativo | `test_cfg_04_override_externo_ativo` | Unitário/propriedades + integração SQLite/filesystem | pendente |
| [CFG-05](specs/config-storage/spec.md) — credencial e ausência de keyring | `test_cfg_05_credencial_e_ausencia_de_keyring` | Unitário/propriedades + integração SQLite/filesystem | pendente |
| [CFG-06](specs/config-storage/spec.md) — arquivo de segredo inseguro | `test_cfg_06_arquivo_de_segredo_inseguro` | Unitário/propriedades + integração SQLite/filesystem | pendente |
| [CFG-07](specs/config-storage/spec.md) — migração interrompida | `test_cfg_07_migracao_interrompida` | Unitário/propriedades + integração SQLite/filesystem | pendente |
| [CFG-08](specs/config-storage/spec.md) — banco indisponível ou futuro | `test_cfg_08_banco_indisponivel_ou_futuro` | Unitário/propriedades + integração SQLite/filesystem | pendente |
| [CFG-09](specs/config-storage/spec.md) — importação sem tocar o legado | `test_cfg_09_importacao_sem_tocar_o_legado` | Unitário/propriedades + integração SQLite/filesystem | pendente |
| [CH-01](specs/contacts-history/spec.md) — contato e favorito | `test_ch_01_contato_e_favorito` | Unitário + integração SQLite/CSV | pendente |
| [CH-02](specs/contacts-history/spec.md) — CSV inválido e duplicado | `test_ch_02_csv_invalido_e_duplicado` | Unitário + integração SQLite/CSV | pendente |
| [CH-03](specs/contacts-history/spec.md) — resultados e recuperação | `test_ch_03_resultados_e_recuperacao` | Unitário + integração SQLite/CSV | pendente |
| [CH-04](specs/contacts-history/spec.md) — busca e retenção | `test_ch_04_busca_e_retencao` | Unitário + integração SQLite/CSV | pendente |
| [LIN-01](specs/linux-integration/spec.md) — desktop sem bandeja | `test_lin_01_desktop_sem_bandeja` | Integração desktop/IPC + testes de URI | pendente |
| [LIN-02](specs/linux-integration/spec.md) — link legítimo e malicioso | `test_lin_02_link_legitimo_e_malicioso` | Integração desktop/IPC + testes de URI | pendente |
| [LIN-03](specs/linux-integration/spec.md) — segunda instância | `test_lin_03_segunda_instancia` | Integração desktop/IPC + testes de URI | pendente |
| [LIN-04](specs/linux-integration/spec.md) — saída durante chamada | `test_lin_04_saida_durante_chamada` | Integração desktop/IPC + testes de URI | pendente |
| [ND-01](specs/native-desktop/spec.md) — layout comparável | `test_nd_01_layout_comparavel` | Snapshot/teclado + revisão visual em VM | pendente |
| [ND-02](specs/native-desktop/spec.md) — função fora do release | `test_nd_02_funcao_fora_do_release` | Snapshot/teclado + revisão visual em VM | pendente |
| [ND-03](specs/native-desktop/spec.md) — alternância de tema | `test_nd_03_alternancia_de_tema` | Snapshot/teclado + revisão visual em VM | pendente |
| [ND-04](specs/native-desktop/spec.md) — tema do sistema | `test_nd_04_tema_do_sistema` | Snapshot/teclado + revisão visual em VM | pendente |
| [ND-05](specs/native-desktop/spec.md) — teclado e HiDPI | `test_nd_05_teclado_e_hidpi` | Snapshot/teclado + revisão visual em VM | pendente |
| [ND-06](specs/native-desktop/spec.md) — falha de registro | `test_nd_06_falha_de_registro` | Snapshot/teclado + revisão visual em VM | pendente |
| [DIST-01](specs/reproducible-distribution/spec.md) — máquina limpa | `test_dist_01_maquina_limpa` | Build/isolamento/artefato + checklist de release | pendente |
| [DIST-02](specs/reproducible-distribution/spec.md) — ELF e AppImage separados | `test_dist_02_elf_e_appimage_separados` | Build/isolamento/artefato + checklist de release | pendente |
| [DIST-03](specs/reproducible-distribution/spec.md) — build de clone limpo | `test_dist_03_build_de_clone_limpo` | Build/isolamento/artefato + checklist de release | pendente |
| [DIST-04](specs/reproducible-distribution/spec.md) — laboratório e isolamento | `test_dist_04_laboratorio_e_isolamento` | Build/isolamento/artefato + checklist de release | pendente |
| [DIST-05](specs/reproducible-distribution/spec.md) — perfil desktop manual | `test_dist_05_perfil_desktop_manual` | Build/isolamento/artefato + checklist de release | pendente |
| [DIST-06](specs/reproducible-distribution/spec.md) — bloqueio de release incompleto | `test_dist_06_bloqueio_de_release_incompleto` | Build/isolamento/artefato + checklist de release | pendente |
| [DIST-07](specs/reproducible-distribution/spec.md) — auditoria antes do push | `test_dist_07_auditoria_antes_do_push` | Build/isolamento/artefato + checklist de release | pendente |
| [SIP-01](specs/sip-telephony/spec.md) — registro e renovação | `test_sip_01_registro_e_renovacao` | Integração Asterisk + assertions de diálogo/estado | pendente |
| [SIP-02](specs/sip-telephony/spec.md) — configuração e autenticação inválidas | `test_sip_02_configuracao_e_autenticacao_invalidas` | Integração Asterisk + assertions de diálogo/estado | pendente |
| [SIP-03](specs/sip-telephony/spec.md) — chamadas nos dois sentidos | `test_sip_03_chamadas_nos_dois_sentidos` | Integração Asterisk + assertions de diálogo/estado | pendente |
| [SIP-04](specs/sip-telephony/spec.md) — cancelamento e corrida de atendimento | `test_sip_04_cancelamento_e_corrida_de_atendimento` | Integração Asterisk + assertions de diálogo/estado | pendente |
| [SIP-05](specs/sip-telephony/spec.md) — recusa e segunda chamada | `test_sip_05_recusa_e_segunda_chamada` | Integração Asterisk + assertions de diálogo/estado | pendente |
| [SIP-06](specs/sip-telephony/spec.md) — DTMF e espera | `test_sip_06_dtmf_e_espera` | Integração Asterisk + assertions de diálogo/estado | pendente |
| [SIP-07](specs/sip-telephony/spec.md) — não perturbe | `test_sip_07_nao_perturbe` | Integração Asterisk + assertions de diálogo/estado | pendente |
| [SIP-08](specs/sip-telephony/spec.md) — transferência e falha | `test_sip_08_transferencia_e_falha` | Integração Asterisk + assertions de diálogo/estado | pendente |
| [SIP-09](specs/sip-telephony/spec.md) — certificado inválido | `test_sip_09_certificado_invalido` | Integração Asterisk + assertions de diálogo/estado | pendente |
| [SIP-10](specs/sip-telephony/spec.md) — rede e suspensão | `test_sip_10_rede_e_suspensao` | Integração Asterisk + assertions de diálogo/estado | pendente |

Total: **46 cenários**, todos pendentes. Para cenários compostos, desdobrar em testes específicos de sucesso/falha sem remover a rastreabilidade.

## Qualidade e casos adicionais

- Propriedades: transitions nunca criam duas chamadas; eventos duplicados não duplicam histórico; valores inválidos não acionam SIP; URI/CSV/SDP malformados não causam panic.
- Cancelamento e saturação: filas cheias, encerramento sem PBX, perda de rede, suspensão e falha do worker; nenhuma espera ilimitada ou lock de UI/áudio.
- SQLite: backup consistente em WAL, migração interrompida, constraint/FK, schema futuro, disco cheio, permissões e preservação de dados.
- Áudio: silêncio correto no mute, sequência RTP/timestamp, limites do buffer, hotplug sem captura em microfone alternativo, jitter/loss e codec incompatível.
- Performance: medir p95 de pelo menos 20 inicializações após aquecimento, RSS idle após 60 s e interação durante falha de rede; informar hardware/driver e budgets do design.
- Segurança: checar `.env`, DB/WAL, exports, Debug, logs e artefatos contra marcadores secretos fictícios; nenhuma chamada de emergência/PSTN no laboratório.
- Build: verificar `ldd`/`readelf` e carregamentos dinâmicos observados, incluindo áudio/gráficos; ausência de libs próprias e assets externos.
- Publicação: clone limpo sem `base/`; auditoria de `git ls-files`, índice e artefatos; nenhuma referência local como submódulo.

Comandos previstos para a implementação, **não executados nesta etapa**:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --doc --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
cargo deny check
cargo audit
```

Cobertura alvo: 80% de linhas em domínio/config/storage. Cobertura e mocks não substituem nenhum cenário SIP/mídia/distribuição. Features extras devem ter matriz CI explícita se forem introduzidas.

## Recibo por execução

Guardar apenas relatório sanitizado: commit, hash do ELF/AppImage, SO/desktop/driver, codec/transporte, dispositivos sem identificadores pessoais, cenário, comando, exit code, horário UTC, métricas, resultado e operador. Evidência manual identifica os dois sentidos da voz, não apenas eco ou janela aberta. Qualquer falha crítica mantém o gate pendente. Outputs brutos, pcaps, chaves, DBs e gravações ficam fora do Git e de releases.

## Validação desta proposta

Ver [validation.md](validation.md) para validação estrutural, análise independente e auditoria de publicação. A revisão Claude receberá somente estes documentos/contratos sanitizados, com ferramentas desabilitadas; não acesso à configuração pessoal nem ao AppImage.

## Extensões vinculadas

CLI/MCP têm cenários e tarefas adicionais em [add-agent-cli-mcp](../add-agent-cli-mcp/tasks.md), obrigatórios para o candidato Linux solicitado com agentes. Os 46 cenários acima continuam sendo apenas o contrato desktop. A versão [web/WASM](../add-web-wasm-softphone/proposal.md) é release posterior com critérios próprios de WebRTC/navegadores. Testes de contratos de plataforma usam adapter fake e Linux real; macOS/Windows não entram na matriz atual.
