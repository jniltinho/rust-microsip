# Claude static review: production engine and desktop slice — 2026-10-13

Sanitized, tools-disabled static reviews only; not runtime, G0 or release approval. Raw JSON and prompts stay in ignored `artifacts/`.

## Round 1

- Prompt SHA-256: `1b9e6a71410e7e0fbea9d73853266b41d8a15481908769d9c59aa49323f602e9`; raw JSON SHA-256: `c56e54b7bf8c576ba365147e71c406b8e7eb587ea98479e9b1d2c097039dd8fa`.

### Verbatim result

BLOCK

Esta é uma revisão estática dos arquivos colados. Não reivindico aceitação em runtime. Também não vi as fontes de `rsipstack`, `rust_microsip_media` (`MasterKey`, `SecureSender`, `ReceiverBank`, `RtpPacket::parse`), da UI nem do `doctor`. Supus que fazem o que o nome promete.

## Bloqueadores

**1. Em modo TLS/SRTP o UA aceita requisições SIP por UDP em texto claro.** Arquivo `crates/telephony/src/engine/sip.rs`, em `Stack::start` (`transport.add_transport(udp.into())`) e em `dispatch` (filtro `from_peer`).
- O transporte UDP é adicionado sempre, inclusive com `Transport::Tls`.
- O `dispatch` só compara o IP de origem com `server.ip()`. Não verifica o transporte nem a conexão.
- Cenário: um atacante na LAN, ou qualquer um que consiga forjar o IP de origem, varre as portas UDP com um INVITE `RTP/SAVP` cujo `Via`/`From` aponta para o PBX.
  - O `incoming()` aceita a chamada e o UI mostra `secure: true`.
  - Se o usuário atender, o 200 OK leva a nossa chave SDES em claro por UDP.
  - Além disso, a mídia vai para o `c=`/`m=` escolhido pelo atacante.
  - A garantia "SRTP só sobre TLS" (`validate`) é contornada.
- Correção:
  - Registrar o transporte UDP apenas quando `settings.transport == Udp`.
  - No `dispatch`, exigir que `tx.connection` seja do transporte configurado. Para TLS/TCP, que seja a conexão estabelecida no `lookup`.
  - Em UDP, comparar também a porta (`SocketAddr` completo), não só o IP.

**2. `answer()` gera um SDP de answer com codecs errados, o que causa áudio unidirecional.** Arquivo `crates/telephony/src/engine/mod.rs`, em `Actor::answer`.
- `call.offer.take()` roda antes de `call.describe(...)`.
- Dentro de `describe`, `self.offer.is_none() && self.media.is_none()` é verdadeiro, então o answer sai com `OFFER_CODECS` (`0 8`), e não com o codec negociado.
- Cenário: um PBX que prefere alaw (Asterisk com `allow=alaw,ulaw`, comum no Brasil) oferece `8 0 101`.
  - Respondemos `0 8 101` e usamos `call.codec = Pcma`.
  - O peer passa a enviar PT 0, que o `media::run` descarta (`payload_type != codec.payload_type()`).
  - O resultado é silêncio na recepção.
  - Se a offer lista só `8`, o answer inclui `0`, que não foi oferecido (viola o RFC 3264).
- No mesmo ponto: `event_pt.or(Some(EVENT_PT))` adiciona `101 telephone-event` mesmo quando a offer não tinha. Isso também viola o RFC 3264.
- O teste com Asterisk provavelmente passou porque a offer começava com `0`.
- Correção: chamar `describe` antes do `take()`, ou passar `codecs: &[call.codec]` explicitamente. No answer, só incluir `event_pt` se `offer.event_pt.is_some()`.

**3. O `ca_file` é lido sem limite e sem checagem de permissão, e falha aberto.** Arquivo `apps/rust-microsip/src/main.rs`, em `desktop()`, no `std::fs::read(path)`.
- Leitura sem limite: `ca_file = /dev/zero` ou um arquivo enorme esgota a memória antes do teste `pem.len() <= 4 MiB`. Um FIFO bloqueia a thread do UI.
- Sem checagem de permissão: o CA define em quem o TLS confia. Um bundle gravável por grupo/outros permite que outro usuário local injete uma CA, faça MITM no TLS e leia as chaves SDES. Isso contraria a política declarada em `crates/config/src/lib.rs` ("arquivos graváveis por grupo/outros são rejeitados").
- Falha aberta: se a leitura falha, só sai `Notice::InvalidState` e o `Register` segue com as raízes do sistema. Quem configurou um CA privado, como forma de pinning, perde esse pinning silenciosamente.
- Correção:
  - Ler via `files::read_bounded(path, 4 MiB, false)`, que já rejeita não-regular, `go-w` e excesso de tamanho. Expor isso como helper público do crate `config`, ou resolver o PEM dentro de `resolve`.
  - Se `ca_file` estiver configurado e falhar, não enviar `Command::Register`.

## Verificação obrigatória (não posso confirmar estaticamente)

**4. Validação TLS dentro do `rsipstack`.** Arquivo `sip.rs`, `TlsConfig { ca_certs, sni_hostname }`.
- O destino é `sip:{IP resolvido}`. É preciso confirmar que o rustls valida cadeia **e hostname** contra `settings.server`, e não contra o IP nem com um verificador permissivo.
- Também é preciso confirmar que um PEM inválido em `ca_certs` gera erro, e não um root store vazio ou um "no verify".
- Faltam dois testes negativos: certificado com hostname errado e CA de lixo. Ambos devem resultar em `RegistrationFailure::Certificate`. A execução E2E positiva não cobre isso.

## Não bloqueadores

**5. Chamadas fantasma sem limite de tempo.**
- Não há timeout de inatividade RTP, session timer nem keepalive. Se a rede cair e o BYE se perder, a chamada fica `Established` para sempre, segue enviando o microfone e bloqueia novas chamadas (`Notice::Busy`).
- Outgoing em `Terminating` (hangup antes do 1xx, com CANCEL rejeitado) depende só da resposta final. Não há prazo local, e um novo `Hangup` é ignorado.
- Sugestão: deadline local de cerca de 32 s em `Terminating` e encerrar com `Failed` após N segundos sem RTP válido quando `receive` estiver ativo.

**6. O `media::run` falha fechado mas deixa a chamada viva.**
- `SecureSender::new` falhando, ou `SecureError::Closed`, encerra a task, mas a chamada continua `Established` e `secure`, sem áudio.
- Sugestão: notificar o actor e encerrar com `EndReason::Failed`.

**7. re-INVITE ou UPDATE sem SDP recebe 488.** Em `dialog_state`, `DialogState::Updated`.
- Refresh de session timer (FreeSWITCH, SBCs) e delayed offer recebem 488, e alguns servidores derrubam a chamada.
- Sugestão: corpo vazio → responder 200 com o SDP atual como offer. UPDATE sem corpo → 200 sem corpo.

**8. Parser SDP com flag `extra_media` pegajosa.** Arquivo `sdp.rs`, em `parse`.
- Com `m=video` antes de `m=audio`, os `c=`, `a=crypto` e direção do bloco de áudio são ignorados.
- SRTP falha fechado. RTP plano usa o `c=` de sessão e ignora `sendonly`/`inactive`.
- Correção: marcar "dentro da seção escolhida" a cada `m=`, em vez de um flag que nunca é limpo.

**9. Endereços de mídia não sanitizados.**
- `rtp` aceita loopback, multicast e broadcast. Um `0.0.0.0` com `recvonly` faria `send_to(0.0.0.0:p)`, que no Linux vai para localhost.
- Sugestão: rejeitar unspecified, multicast e broadcast, a não ser como hold `inactive`.

**10. Correlação de diálogo fraca.** Em `dialog_state`, `ours`.
- Com `dialog_id == None`, qualquer evento é aceito. Fora isso, só o `call_id` é comparado.
- Um `Updated` atrasado da chamada anterior, chegando logo após um novo `Dial`, seria respondido com o SDP da nova chamada.
- Sugestão: com `None`, aceitar só `Calling`/`Trying`. Nos demais casos, comparar o `DialogId` inteiro.

**11. Direção de mídia inconsistente.**
- No ramo de rekey de `reinvited`, `send/receive = !held` ignora `remote.flow` (um answer `inactive` ao resume ainda transmite).
- Em `Updated` com hold local, sempre respondemos `sendonly`, mesmo a uma offer `sendonly`/`inactive`. O correto é `inactive`.
- Hold enviado duas vezes dispara dois re-INVITEs, porque não há estado intermediário.
- Não há caso de áudio tocando em hold: todos os caminhos zeram `receive` com `local_flow != SendRecv`.

**12. Transfer `Pending` sem NOTIFY final fica preso para sempre** e bloqueia novos transfers. Sugestão: timeout que leve a `Failed(408)`.

**13. `Sink` quebra a promessa "snapshots finais nunca são descartados".** Sem notices no backlog, `pop_front()` descarta snapshots. Na prática só acontece se o UI parar de fazer poll.

**14. Segredos fora de `Zeroizing`.**
- `Credential.password` (String do `rsipstack`, clonada) e `offer.to_vec()` com a chave SDES não são zerados. Isso é aceitável.
- Porém, se um `tracing-subscriber` for adicionado, os logs debug do `rsipstack` imprimem o SDP com `a=crypto` e possivelmente o `Credential` via `Debug`.
- Sugestão: documentar e filtrar o target `rsipstack` em `warn` ou acima.

**15. Config: dono e corrida.** Arquivo `crates/config/src/files.rs`.
- `read_bounded` não exige dono: um TOML `0644` de outro usuário não-root é aceito, e esse usuário pode reescrevê-lo, por exemplo trocando o registrar. Sugestão: exigir `uid == ctx.uid || uid == 0`.
- Uma troca por FIFO entre o `metadata` e o `open` bloqueia o `open`. Sugestão: `custom_flags(O_NOFOLLOW | O_NONBLOCK)` e checar o tipo pelo handle.

**16. Validações de entrada.**
- O `display_name` aceita `\`, o que gera `"abc\"` e quebra o header From.
- `user_part` não filtra caracteres de formatação Unicode (bidi, U+202E), o que permite spoofing visual do caller ID.
- `#` em `DialTarget` vai sem escape para `sip:` e Refer-To. Hoje isso falha de forma limpa com `InvalidState`.

**17. Shutdown e answer.**
- Hangup (2 s) + unregister (3 s) passa do limite de 4 s do `EngineBackend::drop`. O shutdown então retorna `false`, o que é inofensivo.
- Em `shutdown()`, uma outgoing em `Dialing` sem dialog não recebe CANCEL.
- Quando `accept()` falha em `answer()`, a chamada termina localmente sem resposta final explícita ao INVITE (deveria ser 500).

## O que está correto (não precisa mexer)

- **Política SRTP sem downgrade:** offer, answer, re-INVITE e UPDATE passam todos por `sdp::parse(policy)`, e um SAVP sem `a=crypto` válido é rejeitado.
- **Filtro de origem RTP:** RTP só é aceito do endereço negociado, sem latching.
- **Limites contra entrada hostil:** o jitter buffer é limitado e há limites de tamanho e de linhas no SDP. Não encontrei pânico alcançável a partir da rede nos arquivos revisados.
- **Mute e hold:** mute zera a captura e hold zera envio e recepção.
- **Redação de segredos no `Debug`:** `Command`, `DialTarget` e `EffectiveConfig` redigem segredos.
- **CLI:** rejeita senha na linha de comando e não ecoa argumentos.
- **Senha em arquivo:** `password_file` e `.env` com senha exigem modo `0600`, dono correto e não ser symlink.
- **`save`:** a escrita é atômica com `0600` e nunca grava a senha.

## Round 2

- Prompt SHA-256: `06a23bc1222158ac31ba39e7eef91596e29ec4cb0106732825a36d1f921f2dc3`; raw JSON SHA-256: `96bde1eb1dd88bf0ede699b0efedc24cb70c5fc7354aa2f4025636e3b62e7501`.

### Verbatim result

BLOCK

Revisão estática dos arquivos colados, sem aceitação em runtime. Tentei consultar o fonte local do `rsipstack` 0.5.13 para confirmar o item 1, mas não consegui lê-lo de forma confiável nesta sessão. As conclusões sobre o comportamento dele seguem como inferência.

## Bloqueador

**1. Regressão provável: em contas UDP, todas as requisições recebidas podem levar 403.** Fica em `crates/telephony/src/engine/sip.rs`, no `dispatch` (`from_peer`).

- **O filtro:** em UDP, ele exige que `connection.get_remote_addr()` seja igual a `peer`.
- **Por que isso falha:** o `SipConnection::Udp` é o socket de escuta compartilhado e não tem um peer por conexão. A origem real de cada datagrama chega à parte em `TransportEvent::Incoming(msg, conn, from)`. Ela costuma ir para os parâmetros `received`/`rport` do Via, e não para a conexão.
- **Consequência:** se `get_remote_addr()` devolver `None`, ou o endereço local, para UDP, toda requisição recebe 403.
  - Isso vale para INVITE, BYE dentro do diálogo, re-INVITE, NOTIFY e OPTIONS.
  - Chamadas recebidas deixam de funcionar.
  - O BYE remoto é recusado, e a chamada só termina pelo timeout de inatividade de 30 s, se houver.
- **Evidência apresentada:** só cobre TLS ("TLS/SRTP incoming calls still pass over the TLS connection").
- **Para liberar:** basta um destes:
  - Teste em runtime com conta UDP: chamada recebida atendida, depois BYE remoto encerrando com `EndReason::Normal`.
  - Comparar a origem pelo `received`/`rport` do Via do topo, que o `rsipstack` preenche com a origem real, em vez de usar `get_remote_addr()`. Mantenha a checagem do tipo de transporte.
- **TLS e TCP:** em princípio estão corretos. Não há listener, e a única conexão é a de saída para `server`.

Os bloqueadores 2 (codecs no answer) e 3 (`ca_file`) da rodada 1 estão corrigidos no código mostrado. O item 4 foi aceito com base na evidência negativa de runtime.

## Notas (não bloqueiam)

1. **CA pode ainda falhar aberto pela UI.** O fail-closed existe só no `desktop()` da inicialização.
   - Se a UI enviar `Command::Register` depois (botão de reconectar, conta editada), o `custom_ca` continua `None` e o engine usa as raízes do sistema.
   - Não vi o código da UI. Sugestão: guardar no engine um estado `System | Custom(pem) | Broken`, em que `Broken` faz o `Stack::start` devolver `Certificate`.

2. **O timeout de inatividade não bate com a resposta dada.** A resposta diz "30 s sem RTP autenticado", mas `heard` só é atualizado para pacotes com o PT negociado e payload de exatamente 160 bytes.
   - Um peer com ptime 30, ou que manda só CN (PT 13) ou eventos RFC 4733, tem a chamada derrubada aos 30 s, quando antes só perdia o áudio.
   - Sugestão: atualizar `heard` logo depois de `unprotect_rtp`/`RtpPacket::parse` bem-sucedidos vindos de `sender.remote`.

3. **Corrida no `TransferDeadline`.** O prazo não carrega uma geração.
   - Cenário: um REFER falha em t=0, o usuário tenta de novo em t=25 e, em t=30, o segundo é marcado `Failed(408)`.
   - Sugestão: incluir um contador de transfer no `Internal::TransferDeadline` e compará-lo antes de agir.

4. **`#` sem escape no `Refer-To`.** O `transfer` monta `<sip:{target}@{domain}>` sem passar por `Stack::uri`, então o `#` não vira `%23`. Isso contradiz a resposta 16. A falha é limpa, com o transfer indo para `Failed`.

5. **`reinvited` não confere o codec do answer.** Se o peer responder ao hold ou ao resume com um codec diferente de `call.codec`, continuamos a enviar o antigo. Sugestão: tratar isso como falha do re-INVITE, igual ao `Updated`, que já faz essa checagem.

6. **re-INVITE sem corpo.** O answer chega no ACK e é ignorado. Se o peer mudar endereço, chave ou direção nesse ACK, a mídia fica dessincronizada. Para refresh de session timer isso é aceitável.

7. **`event_pt` em re-offers recebidos.** Respondemos com o PT do primeiro diálogo mesmo que a nova offer use outro PT, ou nenhum. É uma violação menor do RFC 3264.

8. **Answer com vários codecs na chamada de saída.** Usamos o primeiro e descartamos os demais PTs listados no answer. Se o peer enviar o segundo, o áudio fica unidirecional. É raro com Asterisk e FreeSWITCH.

9. **`current_uid()` via dono de `/proc/self`.** Num processo non-dumpable, esse dono é root, e então os próprios arquivos do usuário seriam rejeitados. O problema é anterior a esta rodada e o caso é raro.

## Confirmado nesta rodada

- O listener UDP só existe em contas UDP, e o tipo de transporte é verificado no `dispatch`.
- Com `negotiated`, o answer leva só o codec escolhido e o `telephone-event` apenas quando foi oferecido. O teste de regressão cobre a offer com PCMA primeiro.
- `read_ca_file` rejeita arquivo não regular, gravável por grupo/outros, de dono estranho ou acima de 4 MiB. Com um CA configurado que falha, o `Register` inicial não é enviado. PEM sem certificados resulta em `Certificate`.
- O parser SDP reconhece seções, rejeita multicast e broadcast, e trata `0.0.0.0` e porta 0 como hold.
- Falha de mídia encerra a chamada com BYE. O CANCEL tem prazo local de 32 s.
- Antes de existir o dialog id, só `Calling`/`Trying` podem reivindicar a chamada.
- O `Sink` nunca descarta snapshots finais nem `Stopped`.

## Round 3

- Prompt SHA-256: `fdd41b9a89718669775954b053232f6361c3f7dceab55282d6d9625fa039b26a`; raw JSON SHA-256: `8fffb843173b61f9ef437eac111fbedc9c416f3490ecd3125a489378ebbe08ce`.

### Verbatim result

APPROVE

Revisão estática dos arquivos colados. Não declaro aceitação em runtime. Não consultei o fonte do `rsipstack` 0.7.3. Para o item 1 da rodada 2, aceito a evidência de runtime apresentada, que era um dos critérios de liberação.

## Bloqueador da rodada 2

1. **UDP com 403 em todas as requisições: resolvido pela evidência.**
   - **Evidência:** o cenário `engine_lab` com duas engines em contas UDP cobre o INVITE recebido e atendido, e o BYE remoto encerrando com `EndReason::Normal`. Também cobre o NOTIFY do transfer, o re-INVITE do PBX e o DND 486. O app real registra, liga e remove o registro ao receber SIGTERM.
   - **Leitura:** isso exercita o caminho `dispatch` → `from_peer` para requisições fora e dentro do diálogo em UDP. Indica que, na 0.7.3, `get_remote_addr()` traz a origem do datagrama. A checagem do tipo de transporte foi mantida.
   - **Ressalva:** como esse comportamento é interno do `rsipstack`, vale manter o cenário UDP do `engine_lab` na CI. Assim, um upgrade da crate que mude a semântica é detectado.

## Notas da rodada 2, conferidas no código

- **Nota 1 (CA):** corrigida.
  - O `TrustAnchors::Unusable` é verificado dentro de `Actor::register`, então um `Register` iniciado pela UI também falha com `Certificate` em contas TLS.
  - Um lock envenenado também cai em `Unusable`, o que mantém o fail-closed.
- **Nota 2 (inatividade):** corrigida. `heard` é atualizado logo depois de `unprotect_rtp`/`RtpPacket::parse` e antes do filtro de PT e tamanho. A origem continua restrita a `sender.remote`.
- **Nota 3 (prazo do transfer):** corrigida.
  - `TransferDeadline` carrega `seq` e é comparado com `self.transfer_seq`.
  - `Referred` não carrega seq, mas não há corrida, porque um novo REFER só começa depois que o anterior saiu de `Pending`.
- **Nota 4 (`#` no `Refer-To`):** corrigida. O `Refer-To` agora é montado com `stack.uri(...)`, que faz o escape `%23`.
- **Nota 5 (codec no re-INVITE):** corrigida, com a ressalva do item 1 abaixo.

## Notas (não bloqueiam)

1. **O "refused" do item 5 é só local.**
   - Quando o 200 OK do re-INVITE chega, o `rsipstack` já mandou o ACK, então o peer considera a nova SDP em vigor.
   - Com o codec trocado, continuamos enviando o codec antigo e a fase não muda. Num hold, o peer pode achar que a chamada está em espera enquanto a UI mostra `Established`.
   - O comportamento é seguro, sem vazamento de mídia nem de chave. Para coerência, pode valer encerrar com BYE, ou mandar um novo re-INVITE com a SDP anterior.
2. **Inconsistência na inicialização com `ca_file` quebrado.**
   - No `desktop()`, um `ca_file` ilegível impede o `Register` inicial mesmo em contas UDP/TCP.
   - Já um `Register` posterior pela UI nessas contas prossegue, porque o engine ignora `Unusable` fora de TLS.
   - O erro é para o lado seguro. Basta escolher um dos dois comportamentos e documentá-lo.
3. **Liveness com RTP sem SRTP.** Qualquer datagrama parseável vindo do endereço de mídia negociado mantém a chamada viva. Isso é inerente ao RTP sem autenticação e não é regressão.
4. **As notas 6 a 9 da rodada 2** continuam aceitas como limitações conhecidas.

