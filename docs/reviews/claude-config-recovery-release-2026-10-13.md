# Claude static review: configuration, recovery and release engineering batch — 2026-10-13

Sanitized, tools-disabled static reviews only; not runtime or release approval. Raw JSON and prompts stay in ignored `artifacts/`.

## Round 1

- Prompt SHA-256: `f3394964f013901bbe169552bfe3975f31f2cd86b3d0e9d97b385d0888f38e4b`; raw JSON SHA-256: `1dbd270996ff52b39dc6b660c5b6c7f40a17bd2c71f877bfa4a32b5db81ce2f7`.

### Verbatim result

BLOCK

Revisão estática apenas. Não afirmo nada sobre comportamento em runtime.

## Blockers

**1. Engine: BYEs adiados são descartados em `unregister()`, `register()` e `shutdown()`**
`Actor::unregister` derruba o stack (`endpoint.shutdown()` + `stop.cancel()`) sem tocar em `pending_byes`. Três caminhos comuns perdem o BYE:

- O usuário sai depois que a rede voltou, mas antes da próxima tentativa do backoff. Essa espera pode chegar a 300 s.
- O usuário clica em Registrar ou salva a conta. `register()` chama `unregister()`.
- `Command::Unregister`, que é permitido porque a call já terminou localmente.

Nos três casos, os dialogs pendentes ficam presos a um endpoint morto. O `drain` no próximo sucesso chama `hangup()` sobre esse endpoint e nada sai na rede. O resultado é um canal fantasma no PBX: o Asterisk vem com `rtptimeout` desligado por padrão. Hoje o BYE adiado só é entregue se o mesmo stack receber resposta a um REGISTER.

Correção: em `unregister()`, antes do `endpoint.shutdown()`, drenar `pending_byes` e aguardar os `hangup()` com timeout curto (≈2 s no total). Isso cobre os três caminhos. `shutdown()` também passa por ali.

**2. Engine: cada probe de inatividade cria uma cadeia extra de refresh que nunca é cancelada**
`schedule()` cria timers independentes, e cada `registered(Ok)` agenda mais um. O probe disparado em `MediaFailed(Inactivity)` acontece fora da cadeia normal. Quando ele tem sucesso, agenda um segundo timer, enquanto o timer original continua vivo.

Depois de N quedas de RTP existem até N+1 cadeias. As cadeias só se fundem por acaso, quando um timer encontra `in_flight`. Isso multiplica o tráfego de REGISTER na sessão e pode acionar rate-limit ou fail2ban no registrar.

Correção mínima: adicionar `refresh_seq: u64` no `Actor`, incrementado a cada `schedule`. `RefreshRegistration` passa a carregar esse seq e ignora valores antigos. Uma alternativa é guardar o `JoinHandle` do timer e chamar `abort()` antes de reagendar.

**3. Scanner: `git grep -I` ignora arquivos binários no índice**
`scan_index` (chave privada e tokens) e a busca de `personal-home` usam `-I`. Qualquer arquivo rastreado que o git considere binário é pulado, por exemplo um arquivo UTF-16 ou um texto com um único NUL nos primeiros 8000 bytes. A varredura de artefatos já usa `grep -a`, então o comportamento é inconsistente.

Correção: trocar `-I` por `--text` (`-a`) no `git grep --cached`.

**4. Scanner: só o índice é verificado, não o histórico**
O script se apresenta como auditoria "pre-push", mas um `git push` publica todo o histórico. Um segredo commitado e removido depois passa no scan.

Correção: varrer todos os blobs alcançáveis, por exemplo:

```sh
git rev-list --objects --all | git cat-file --batch-check='%(objecttype) %(objectname)' | awk '$1=="blob"{print $2}'
```

seguido de `git cat-file --batch` e grep. No CI, o checkout precisa de `fetch-depth: 0`.

**5. Scanner: não cobre o segredo do próprio app**
O próprio `main.rs` cita TOML e `password_file` como fontes de senha ("No password from env/.env/file/TOML"). Mesmo assim, um `rust-microsip.toml` real com `password = "..."`, ou o arquivo apontado por `password_file` (com qualquer nome), passa no scan. `token_pattern` só cobre tokens de nuvem, e `lab-values` só cobre senhas de laboratório.

Correção:
- Em `forbidden_path`: proibir o nome do config do app fora de exemplos.
- No conteúdo: checar `^\s*password\s*=\s*"[^"]+"` em `*.toml`, com allowlist de placeholders.

## Notas (não bloqueiam; verificar)

**6. Settings: o guard de precedência é fail-open no caminho de erro**
Em `desktop()`, `config::layer_sources(&ctx).unwrap_or_default()` deixa `sources` vazio quando falha, e o saver passa a gravar qualquer campo. Além disso, com `base = None` (config inválida ou ausente), vale confirmar que `sqlite_edits` não emite remoções (`None`) para `ca_file`/`password_file` já salvos no SQLite. Se emitir, salvar pela janela apaga o `ca_file` e rebaixa a confiança TLS para as raízes do sistema. Sugestão: recusar o save quando `layer_sources` falhar.

**7. Slow REGISTER em loop**
Se o caminho for consistentemente lento (mais de 5 s, com perda alta ou TLS/DNS lento), o refresh de 2 s se repete indefinidamente, com cerca de um REGISTER a cada 7 s. Sugestão: aplicar o refresh rápido só uma vez, quando o REGISTER anterior não foi lento.

**8. `unregister()` com REGISTER em andamento**
`account.registration` está `None` porque foi movido para a task, então nenhum `expires=0` é enviado. Se a task terminar com sucesso depois, o binding fica ativo até `expires` (até 3600 s) e as chamadas recebidas vão para um contato morto.

**9. `send_register`: cadeia de refresh morre em silêncio**
Quando `take()` retorna `Some` e `stack.registration()` retorna `Err`, a função faz `return` sem estado nem reagendamento. A cadeia de refresh morre sem aviso e a UI continua mostrando "Registered".

**10. Orçamento de shutdown**
O `Drop` espera 4 s. Por dentro, `shutdown` usa até 2 s (hangup) + 3 s (unregister), e o runtime ainda tem `shutdown_timeout(2s)`. O processo pode terminar no meio do de-registro.

**11. Possível eco de segredo em erros de parse**
Confirmar que `ConfigError` não repassa o `Display` do `toml::de::Error`, que inclui a linha-fonte. Um TOML malformado com `password = "...` vazaria em `config import`, `validate` ou `doctor`.

**12. `replace_config` deixa item órfão no keyring**
Ele limpa `secret_ref`, mas não apaga o item. O segredo fica no keyring sem referência e o usuário não é avisado.

**13. Referência do keyring é só `username@domain`**
Ela não inclui `server` nem `transport`. Uma mudança de servidor via env/TOML reaproveita a senha lembrada com outro host. É digest, mas o hash pode ser quebrado offline. Considerar incluir server e transport na referência.

**14. Journal de chamadas**
- `recover_interrupted` não aparece em `main.rs`. Confirmar que é chamado uma vez, no caminho da janela, depois de `acquire` e com store durável.
- No modo degradado (sem `XDG_RUNTIME_DIR`), uma segunda instância fecha a call aberta da primeira. O `finish_call` da primeira cai em `record_call`, que é ignorado, e o desfecho real se perde.

**15. Thread de UI**
- `audio_devices()` executa `AudioDevices::enumerate()` no thread de UI.
- `notifier.incoming_call()` e `close()` são chamados dentro de `poll()`.

Confirmar que nenhum dos dois faz chamada síncrona de D-Bus ou probe de dispositivo.

**16. Teste de áudio**
- `running.store(false)` acontece antes do envio de `AudioTestDone`. Um teste novo pode ser zerado pelo Done atrasado do anterior.
- Considerar bloquear o teste durante uma call.

**17. Sinais**
Depois do primeiro SIGTERM/SIGINT, os sinais seguintes são engolidos, porque o handler do tokio continua instalado. Se a janela não processar frames (minimizada no Wayland), só SIGKILL encerra o processo. Além disso, SIGINT sai com 143 em vez de 130.

**18. Lacunas menores do scanner**
- `home_pattern` não pega `file:///home/<user>` nem caminhos precedidos de caractere alfanumérico (`-L/home/<user>`, strings contíguas em `.rodata` de ELF).
- O próprio script fica fora da varredura de conteúdo.
- `lab-values` exige pelo menos 12 caracteres.
- Faltam padrões como `sk_live_`, PuTTY key, `.git-credentials`, `.npmrc`, `.pypirc` e `*.kdbx`.

**19. CI: pontos que dependem de arquivos não mostrados**
- Confirmar que o `Dockerfile` fixa `FROM` por digest e usa `cargo --locked`.
- Considerar Dependabot para manter os SHAs das actions atualizados.

**20. Remoção do desktop entry**
`RemoveLinkHandler` apaga o que estiver no caminho, mesmo que não tenha sido criado pelo app.

## Verificado sem problema
- **CI:** checkout fixado por SHA (v4.2.2), `permissions: contents: read`, `persist-credentials: false`, gatilho `pull_request` (não `_target`), sem cache e sem secrets.
- **Storage:** chaves de configuração restritas a whitelist, sem chave de senha, SQL de coluna derivado da whitelist, transações tudo-ou-nada.
- **`install_desktop_entry`:** `create_new` + `rename` é seguro contra symlink.
- **Segredos:** `Job` e `JobResult` só carregam `SecretString` (Debug redigido); o worker do keyring é serializado e não bloqueia a UI.

## Round 2

- Prompt SHA-256: `c5c4dee7a9ca65cd00992bbea32922e6afb6027483c3ba943a5d127d199a6213`; raw JSON SHA-256: `17fbcdf03cd803ae4f33eb38a43f46eb9d95f91ca929e4a80b8b72cbde47589f`.

### Verbatim result

APPROVE

Revisão estática apenas. Não afirmo nada sobre comportamento em runtime. Os resultados de runtime citados nas respostas são do autor e não foram verificados por mim.

## Blockers do round 1

1. **BYEs adiados.** Resolvido no código. `unregister()` drena `pending_byes` enquanto o stack ainda está vivo, antes de `account.take()`, `endpoint.shutdown()` e `stop.cancel()`. A drenagem usa `JoinSet` com timeout de 1 s. `register()`, `Command::Unregister` e `shutdown()` passam todos por `unregister()`. O orçamento soma 1 s (hangup) + 1 s (BYEs) + 1,5 s (unregister) = 3,5 s, abaixo dos 4 s do `Drop`.

2. **Cadeias de refresh.** Resolvido. `schedule()` incrementa `refresh_seq`, e `RefreshRegistration` exige `seq == refresh_seq` e `generation == self.generation`. Verifiquei os caminhos do probe por inatividade:
   - **Probe com sucesso ou falha transitória:** reagenda e invalida o timer anterior.
   - **Probe com `Unauthorized`:** não reagenda, e o timer anterior continua válido. A cadeia não morre nem se duplica.
   - **Timer disparado com probe `in_flight`:** é consumido, e o resultado do probe reagenda.

3. **`git grep -I`.** Resolvido. O conteúdo vem dos blobs via `cat-file --batch` e é verificado com `grep -a`. Há uma segunda passada sem bytes NUL, que cobre UTF-16.

4. **Histórico.** Resolvido. O scan cobre blobs alcançáveis por `rev-list --objects --all` e paths de `git log --all -m --no-renames`. Os dois jobs de CI usam `fetch-depth: 0`. A allowlist de `personal-home` é restrita à categoria, ao blob id e à condição "fora do índice".

5. **Segredo do app.** Resolvido na forma canônica: `password = "..."` em `*.toml` e nomes de arquivo com `password`/`secret`. A exceção para exemplos exige path de exemplo **e** a linha marcadora. Conferi que os padrões de conteúdo não casam com o próprio script, e o script agora é varrido.

## Notas restantes (não bloqueiam)

1. **`toml_password` cobre só string básica no início da linha.** Escapam formas TOML válidas:
   - string literal: `password = 'x'`
   - chave pontilhada: `account.password = "x"`
   - tabela inline: `account = { password = "x" }`

   Sugestão: `(^|[[:space:]{,.])password[[:space:]]*=[[:space:]]*("[^"]+"|'[^']+')`, mais um caso negativo no self-test.

2. **`scan_tree` (artefatos e AppImage) não aplica `toml_password` nem `secret_name`.** Um `rust-microsip.toml` real empacotado no AppImage passa, porque `forbidden_path` não cobre esse nome.

3. **Clone raso só gera `WARN`.** No uso local como pre-push, um clone raso faz o scan cobrir só parte do histórico e ainda assim resultar em PASSED. Sugestão: falhar, salvo override explícito.

4. **Alvos de symlink só são checados no índice**, não no histórico.

5. **BYEs adiados sem tentativa automática.** Quando as tentativas de registro se esgotam (`MAX_REGISTRATION_ATTEMPTS`) ou o probe retorna `Unauthorized`, `pending_byes` fica parado até uma ação do usuário. Se o usuário re-registra com a rede ainda fora, os BYEs recebem uma única tentativa de 1 s e são descartados. É limitado e aceitável, mas vale documentar junto com o item 8 do round 1.

6. **`config_import --apply`:** se `store.secret_ref()` falhar, o erro é engolido (`.ok().flatten()`). O item do keyring fica órfão sem a nota prometida. Sugestão: emitir a mesma nota quando a leitura da referência falhar.

7. **Teste de áudio durante uma call.** Continua permitido (segunda parte do item 16 do round 1). Abrir um segundo `DeviceMedia` com dispositivos `hw:` pode falhar com EBUSY ou disputar o dispositivo com a call. Sugestão: recusar o teste enquanto houver call ativa.

8. **AppImage:** uma falha de `unsquashfs` ou da asserção ELF dentro do `while` em pipeline aborta o script via `set -e`, sem imprimir `appimage.log`. O resultado é fail-closed, mas sem diagnóstico.

## Sem regressão observada
- O `Sink` continua sem descartar snapshots finais nem `Stopped`.
- `send_register` não deixa mais a cadeia morrer em silêncio: define `Failed` e agenda nova tentativa em 30 s.
- O saver recusa salvar sem proveniência, grava apenas `WINDOW_FIELDS`, e os testes cobrem a preservação de `ca_file`, `password_file` e `expires`.
- A referência do keyring inclui server, porta e transport. Uma referência antiga ou divergente não é usada.
- A recuperação de calls interrompidas só ocorre com o lock de instância única e store durável.
- No segundo sinal, o processo sai na hora com 143/130.

