**VERDICT: APPROVE**

Conferi os cinco apontamentos da primeira rodada contra o texto real dos documentos. Não restam bloqueadores documentais concretos.

## Verificação das correções

1. **Rede interna vs. ports no host: corrigido e consistente.**
   - No design §7, o default continua `internal: true` sem `ports`.
   - `compose.host.yaml` move o PBX para uma bridge separada não interna, com binds só em `127.0.0.1` e anúncio externo coerente (`external_*_address`, porta externa, `rtp_symmetric`, `rewrite_contact`). A exigência de validar o SDP nos dois sentidos também está presente.
   - DIST-04, DIST-05, a matriz do test-plan e a task 1.5 dizem a mesma coisa.

2. **VM cliente sem acesso ao loopback do host: corrigido.**
   - O G4 usa PBX numa VM auxiliar com `compose.vm-lab.yaml`, bind em `$LAB_BIND_IP` host-only, ACL/firewall e proibição de `0.0.0.0`.
   - As VMs cliente ficam sem Docker, Rust ou pacotes extras, e o operador B usa um softphone próprio com headset.
   - O texto é coerente no design, em DIST-01, DIST-04 e DIST-05, na matriz do test-plan e na task 9.6.

3. **Impairment/NAT sem topologia ou privilégios: corrigido.**
   - AUD-02 usa um proxy UDP em espaço de usuário com seed fixo, sem `tc`/`CAP_NET_ADMIN`. Isso aparece no design, no test-plan e na task 5.1.
   - AUD-06 tem topologia manual com VM roteador NAT descartável, PBX/STUN do outro lado e host pessoal intocado. Isso aparece no design, no test-plan e na task 5.5.
   - O texto não afirma que a rede Compose default demonstra NAT.

4. **ELF do G0 sem UI/áudio/D-Bus: corrigido.**
   - O design G0 e a task 1.8 exigem a mesma pilha eframe/wgpu, áudio, D-Bus/notificações e IPC de produção.
   - Também exigem renderer de compatibilidade, foco/teclado, hotplug e auditoria DT_NEEDED/dlopen. A falha bloqueia o G0.

5. **VM KDE sem Qt: corrigido.**
   - O texto agora diz "sem pacotes adicionais ao desktop padrão" e "ELF sem dependência de Qt".
   - DIST-01 ("sem instalar … Qt") e ND ("executar sem … Qt") não conflitam com o KDE padrão.

## Consistência geral

- As contagens batem: 7 specs, 46 cenários (AUD 6, CFG 9, CH 4, LIN 4, ND 6, DIST 7, SIP 10), 46 linhas no plano de testes e 54 tarefas (9+5+6+6+5+4+5+4+10).
- A precedência tem seis camadas, igual no design e em CFG-01.
- Também estão alinhados entre design e specs: DND com 486, permissões incluindo WAL/SHM/backups, senha literal só em memória e regras de CSV.
- Todas as evidências runtime estão declaradas como pendentes.

## Observações não bloqueadoras

Estes pontos já são cobertos pelas tasks 1.5 e 9.6 ou por gates runtime:

- **Falta o comando do perfil G4.** O bloco "Comandos de contrato" não inclui `compose.vm-lab.yaml`, e o texto não diz se ele se empilha sobre `compose.host.yaml`. Vale explicitar isso na implementação.
- **Overrides do Compose mesclam listas.** `ports` e `networks` são mesclados, não substituídos. Retirar o PBX da rede interna ou trocar o bind loopback vai exigir `!reset`/`!override` ou arquivos não empilhados. A verificação via `docker compose config` na task 1.5 cobre isso.
- **Firewall vs. portas publicadas.** Na VM auxiliar, o firewall precisa atuar na chain `DOCKER-USER`, porque portas publicadas ignoram regras comuns de INPUT/UFW. A ACL do Asterisk complementa, mas não substitui isso.
- **Teardown com overrides.** O `down` documentado usa só o arquivo base. Redes definidas apenas em overrides podem sobrar, então convém repetir os mesmos `-f` no teardown.
- **NAT no perfil host.** Com port mapping, o Asterisk pode ver a origem como o gateway da bridge, que está dentro de `local_net`, e não aplicar o anúncio externo. A validação de SDP prevista cobre isso.
- **ACL no AUD-06.** A ACL precisa incluir o IP externo da VM roteador.
