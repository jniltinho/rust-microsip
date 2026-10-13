# Claude static review: Linux integration and persistence batch — 2026-10-13

Sanitized, tools-disabled static reviews only; not runtime or release approval. Raw JSON and prompts stay in ignored `artifacts/`.

## Round 1

- Prompt SHA-256: `8ebc9591434c80f691380374a0fe999df8d58138ea38324f1bc5c7f65b00cdb9`; raw JSON SHA-256: `35209afaab2129182dbc8507d1f1bd4809ee521f729d2e6713ada5219c7d9448`.

### Verbatim result

BLOCK

Esta é uma revisão estática do código colado. Não executei nada e não estou declarando aceitação em runtime. O código da UI (`ui::run`, o tratamento de `External::Dial`) não estava no lote. A garantia de que nada é discado sem confirmação depende dele e, aqui, só está sustentada pela evidência de runtime que você descreveu.

## Bloqueadores

1. **A instância única falha aberta e quebra o contrato do LIN-03** (`apps/rust-microsip/src/main.rs`, `desktop()`, braço `Err(error)` de `acquire`)
   - O módulo `instance.rs` afirma que qualquer processo não-dono "must then exit without registering SIP or opening the database". O doc de `default_runtime_dir` diz ainda que "the caller should refuse to start".
   - Mesmo assim, `main` trata qualquer erro de `acquire` apenas com um aviso no stderr. Em seguida registra SIP, abre o ALSA e abre o SQLite.
   - O caso concreto é `InstanceError::NoResponse` ou `Protocol`. Ele só ocorre quando `try_lock` retornou `WouldBlock`, ou seja, há um dono vivo segurando o lock. O resultado garantido é uma segunda instância completa:
     - registro duplicado da mesma conta;
     - o PBX faz fork das chamadas recebidas para dois processos;
     - dois escritores no mesmo banco;
     - duas janelas atendendo o mesmo `CallId`.
   - O caminho é alcançável sem defeito no dono. `accept_loop` atende um cliente por vez. O timeout de 2 s vale por `read`, não por conexão, então um cliente lento (`take(512)` × 2 s por leitura) mantém a thread ocupada. O forwarder seguinte expira em 2 s e cai em `NoResponse`.
   - `Insecure` e `Io` também caem nesse braço. Um diretório de runtime que ficou `0755` faz todo lançamento virar instância completa, para sempre e de forma silenciosa.
   - Correção mínima: `NoResponse | Protocol` devem encerrar com código ≠ 0, como já acontece com `Busy`/`Invalid`. Para `NoRuntimeDir`/`Insecure`/`Io`, decidam entre seguir o doc (recusar) ou documentar explicitamente o modo degradado. Hoje o código e o doc se contradizem.

## Notas (não bloqueiam)

2. **`current_uid()` via dono de `/proc/self`** (`instance.rs`): se o processo ficar não-dumpable (`PR_SET_DUMPABLE=0`, algo plausível num softphone que guarda senha em memória, ou alguns sandboxes), `/proc/self` passa a pertencer ao root. Aí o uid retornado é 0, todo diretório é recusado como `Insecure` e, combinado com o item 1, a instância única deixa de existir. Prefiram `getuid` (rustix/libc) ou o uid de um arquivo recém-criado.
3. **Loop ocupado em `accept_loop`**: um erro persistente de `accept` (EMFILE/ENFILE) é ignorado por `if let Ok` e o `for` gira a 100% de CPU. Coloquem um `sleep` curto no braço `Err`.
4. **Bloqueio da thread de IPC por cliente lento** (mesmo uid apenas): o read timeout é por chamada, não um deadline total. Como a superfície é só do próprio usuário, não é escalada de privilégio; o efeito real é alimentar o item 1. Um `Instant` de deadline em `serve` resolve.
5. **Host do `sip:` descartado silenciosamente** (`uri.rs`): é a decisão de segurança certa, porque impede redirecionar a sinalização. Mas `sip:alice@outro.dominio` vira `alice` no PBX do usuário, e `tel:`/`sip:` aceitam códigos de serviço (`*72…`, `#`), como desvio de chamada. A confirmação é a única mitigação, então a UI deve mostrar o alvo exato e, idealmente, avisar que o domínio foi ignorado.
6. **Botões de notificação podem ficar mortos**: o status vira `Available { actions: true }` assim que a thread `notifier-actions` é criada, antes do `AddMatch`. Se `listen` falhar, ou se o daemon reiniciar e o unique name mudar (o filtro de `sender` fica preso ao dono antigo), os botões aparecem mas não fazem nada. É falha segura, porque a janela continua sendo o caminho de atendimento. Quanto a spoofing, está correto: o filtro por dono, o mapeamento por id e a checagem `ringing == action.call` em `main` impedem atender uma chamada que não está tocando.
7. **Notificações órfãs** (`main.rs`, `EngineBackend::notify`): `ringing: Cell<Option<CallId>>` rastreia uma única chamada. Com duas chamadas recebidas sobrepostas, a primeira nunca recebe `close`: a notificação fica até 60 s e a entrada em `ids` vaza. A ação nela é ignorada pelo gate, então não há risco de segurança.
8. **Merge de CSV ignora o limite por contato** (`Store::import_csv`): ao fundir com um contato existente, `MAX_NUMBERS_PER_CONTACT` não considera os números já gravados. Com 30 existentes mais 32 importados, o contato fica com 62. O invariante é quebrado sem corromper nada.
9. **Amplificação de memória no CSV**: uma linha de ~10 MiB só de vírgulas gera ~10 M de `String` vazias (~250 MB). Não há panic, e a entrada é um arquivo escolhido pelo usuário. Limitar os campos por linha (por exemplo, a 3) e rejeitar cedo resolve.
10. **Retenção automática com relógio errado** (`open_storage` → `prune_older_than(90)`): um RTC adiantado no boot apaga permanentemente todo o histórico, sem confirmação. Considerem não podar quando `now_utc()` estiver muito à frente do `MAX(started_utc)`.
11. **TOCTOU em `create_private_file`**: já está marcado com `ponytail:`. Fica restrito ao mesmo usuário porque o diretório pai é do usuário. Um diretório pai existente com modo amplo não é verificado nem apertado, mas o arquivo `0600` cobre isso.
12. **Caracteres bidi** em nome de contato, rótulo e `remote`: o storage aceita esses caracteres (só rejeita controle), enquanto `sanitize_caller` os remove. Isso permite spoofing visual na lista de contatos e no histórico. Vale aplicar o mesmo filtro na exibição.

## Verificado sem achados

- **SQL**: todas as queries são parametrizadas, e o `LIKE` escapa `%`, `_` e `\` com `ESCAPE '\'` correto. Não encontrei injeção.
- **Parsing de link**:
  - o tamanho é limitado antes de alocar;
  - só ASCII imprimível é aceito;
  - `;` e `?` (parâmetros e headers) são rejeitados antes de decodificar;
  - o percent-decode é de passada única e rejeita CR/LF/NUL/espaço;
  - o resultado passa por `DialTarget::parse`.
  
  Não há injeção de header SIP nem redirecionamento de host. `sip:--config=…` não vira opção, porque `is_link` captura o argumento antes.
- **Protocolo IPC**: o tamanho é limitado, exige UTF-8 e forma canônica (`as_str() == target`), e o valor nunca é ecoado.
- **IPC**:
  - o diretório é `0700`, do dono, sem symlink;
  - o socket é `0600`;
  - um não-socket nunca é removido;
  - a eleição usa `flock`, liberado pelo kernel quando o processo morre.
  
  Outro usuário não alcança o socket. A ausência de `SO_PEERCRED` é aceitável enquanto essa garantia depender do diretório privado.
- **Arquivos e backup**:
  - o DB é `0600` e o diretório é criado `0700`;
  - um backup existente nunca é sobrescrito (`create_new`);
  - um backup parcial é removido;
  - um schema futuro é recusado sem gravação (a checagem ocorre antes de `journal_mode = WAL`);
  - uma falha de abertura cai para memória sem apagar o arquivo.
- **Panics**: não encontrei indexação ou `unwrap` alcançável com entrada hostil em `uri.rs`, `csv.rs`, `instance.rs` ou `notify.rs`.
- **Segredos**: senha não é aceita na CLI, os erros não ecoam argumentos, URIs nem dados do usuário, e `Debug` de `Store`/`Listener`/`Notifier` é opaco.

## Round 2

- Prompt SHA-256: `fc5a85c0c3a0aa1e5241ade1457f32404b035e82ae49de90d652f8693bf64954`; raw JSON SHA-256: `e5620a515164e0c01b9ac6550b9fcde7bd0e93a0c22fcd85a891fd36016c8cb3`.

### Verbatim result

APPROVE

Esta é uma revisão estática do lote colado. Não executei nada e não declaro aceitação em runtime. `ui::run` e o tratamento de `External::Dial` continuam fora do lote, então a garantia de que nada é discado sem confirmação segue apoiada só na evidência de runtime que vocês descreveram.

## Bloqueador da rodada 1: resolvido

1. **Instância única agora falha fechada** (`main.rs`, `desktop()`):
   - `NoResponse | Protocol` → exit 1.
   - `Insecure | Io` → exit 1.
   - O braço genérico `Err(error)` só alcança `NoRuntimeDir`, porque o enum tem exatamente cinco variantes e as outras quatro já são tratadas. Não há mais caminho em que um dono vivo coexista com uma segunda instância completa.
   - Os erros de `forward` também falham fechado: `EPIPE` no `write_all`, erro de leitura e socket de outro dono caem em `Io`/`Insecure`.
   - O efeito colateral é intencional e aceitável. `XDG_RUNTIME_DIR` apontando para um diretório inexistente ou de outro usuário (por exemplo, depois de `su`) agora impede a inicialização em vez de rodar sem proteção.

## Correções verificadas

2. **Itens 3 e 4**:
   - O braço `Err` de `accept` dorme 100 ms, então não há mais loop ocupado.
   - `read_line` usa um deadline absoluto e recalcula o timeout restante a cada leitura. O caso de zero é filtrado antes de `set_read_timeout`, evitando o `EINVAL` de `Some(ZERO)`.
   - O buffer fica limitado a `MAX_LINE + 127` bytes.
   - A resposta tem no máximo 11 bytes, então o write timeout não prende a thread.
   - Cada conexão ocupa a thread por no máximo cerca de 2 s.
   - O caminho de parada continua correto: no `Drop`, o `stop` é checado no topo da iteração, inclusive depois de um `Err`.
3. **Item 8**:
   - A contagem é feita depois do `INSERT OR IGNORE` e cobre os números já gravados mais os novos.
   - Um retorno antecipado descarta a `Transaction`, e o rollback do rusqlite no drop desfaz o import inteiro.
   - Um contato existente no limite cujos números importados são todos duplicados continua passando (`total` não excede o máximo), o que está correto.
4. **Item 9**:
   - Uma linha com N vírgulas aloca no máximo 4 campos, porque o `\n` empurra um campo extra depois do limite. A memória fica proporcional à entrada.
   - Com exatamente 4 campos (`a,b,c,d`), a linha não é marcada como malformada, mas é rejeitada como `ColumnCount`. Com 5 ou mais, vira `Malformed`. Os dois casos bloqueiam o import.
   - Um header malformado nunca casa com o padrão de 3 colunas: o padrão exige `!malformed`, e o excesso sempre gera 4 campos ou mais.

## Notas (não bloqueiam)

5. **A documentação ainda contradiz o código**: o doc de `default_runtime_dir` (`instance.rs`) diz que "the caller should refuse to start", mas `main` roda em modo degradado com `NoRuntimeDir`. A rodada 1 pedia escolher um dos dois. A decisão está registrada só no comentário de `main`. Atualizem o doc do crate para descrever o modo degradado.
6. **A nota 2 mudou de natureza**: com a falha fechada, um processo não-dumpable faz `/proc/self` pertencer ao root. O resultado agora é `Insecure` e recusa de inicialização, ou seja, um problema de disponibilidade e não mais de segurança. Se algo no processo chamar `PR_SET_DUMPABLE=0`, o app não abre. `getuid` continua sendo o caminho certo.
7. **Fila atrás de um cliente lento**: o deadline de 2 s do forwarder começa no `write` dele, não quando o dono começa a atendê-lo. Atrás de uma conexão que ocupe os 2 s, o forwarder recebe `NoResponse` e agora sai com código 1. É falha fechada e só alcançável pelo mesmo uid. No pior caso, o lançamento falha com mensagem e não abre uma segunda instância.
8. **Nota 7 não verificável neste lote**: a afirmação de que o engine recusa a segunda chamada com 486 depende de código de telefonia que não foi colado. `EngineBackend::notify` continua rastreando uma única chamada. Se o engine emitir `Ringing` antes de recusar, a notificação órfã volta, mas sem risco de segurança.
9. **Cosmético**:
   - `read_line` tem dois parágrafos de doc sobrepostos; o primeiro, com "UTF-8 … `None` if invalid", ficou obsoleto.
   - `accept_loop` usa `std::thread::sleep(std::time::Duration::…)` com caminho completo, embora `thread` e `Duration` já estejam importados.

## Regressões

Não encontrei regressões no lote. Os testes existentes de IPC continuam coerentes com o novo `read_line`: o caso sem `\n` seguido de `shutdown` recebe EOF → `invalid`, e a linha longa estoura `MAX_LINE` → `invalid`.

