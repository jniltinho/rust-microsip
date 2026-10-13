# Claude static secure controls review — 2026-10-13

Sanitized, tools-disabled static review only; not runtime, G0 or release approval. Raw JSON and prompt stay in ignored `artifacts/`.

## Round 1

- Prompt SHA-256: `f8222bb9516f2a82b4b2b5a61fa7f124b0a06a7bafa7c01ee69e4916db71d1af`; raw JSON SHA-256: `a764734f26ab3bf3aeb0c869b70511356dfc66862cc1df2164edd84f354d4bcd`.

### Verbatim result

APPROVE

Não encontrei bloqueador concreto de correção, segurança ou honestidade de evidência nas fontes mostradas. Esta é uma revisão estática. Ela não confirma a aceitação em runtime nem os resultados do receipt.

A revisão assume, sem verificar, o comportamento destes itens que não estão no material: `EndpointOwner::connect` (chave nova por chamada e apenas TLS), `sdes::parse` (aceita PT 101, `a=sendrecv` e exatamente um `a=crypto`), `wait_phase` (pode ser chamado de novo após terminar, como no caminho `ReferenceTone`) e a orientação de `DialogId` em `DialogState::Notify` no rsipstack.

**Pontos verificados sem bloqueio**

1. `tests/probes/secure_controls.rs` (`SecureStream::send`/`digit`): os eventos RFC 4733 passam pelo mesmo `SecureSender`, com o mesmo SSRC e uma sequência contínua. O tamanho de tag de 10 bytes é exigido. O timestamp fica fixo por evento, com marker no início. Não há caminho em claro.
2. `tests/probes/secure_controls.rs` (`window`): só aceita pacotes da origem negociada que passam na autenticação SRTP. Falhas de autenticação, replay ou pacote inválido são descartadas. Qualquer outro erro, ou RTP autenticado malformado, aborta. RTP em claro do PBX só derruba as métricas e falha os gates, ou seja, o comportamento é fail-closed.
3. `tests/probes/secure_controls.rs` (`reoffer`): rejeita mudança de chave do PBX, de endereço RTP/RTCP e de direção. O tag 1 é exigido via `parse_directed`, que repassa ao parser estrito. O `SecureSender` e o `ReceiverBank` continuam entre as re-ofertas, sem reset. Por isso não há reuso de keystream com a mesma chave. Também não há downgrade possível, porque a resposta passa pelo parser RTP/SAVP estrito.
4. `tests/probes/secure_controls.rs` (`NotifyLog`): um NOTIFY de outro dialog, sem `arm`, malformado, com `terminated` provisório ou recebido após o final retorna 481 e marca `failed`. `wait` falha quando há falha ou `id` divergente. O padrão `notified()` antes do lock está correto para `notify_waiters`.
5. `tests/probes/secure_controls.rs` (`AckLog`): se o limite de 16 for atingido, o ACK não é encontrado e `reoffer` falha. O comportamento é fail-closed.
6. Harness: os contadores 39→40 e 5→6, os hashes em `secure_call_inputs.json` (`probe_files` e `candidate_inputs`) e as sentinelas de `check-context.sh` estão consistentes entre si.

**Não bloqueantes (recomendo corrigir antes de arquivar)**

7. Receipt `.md`, linha da transferência: diz "ratio ≥ 1813.23", mas o JSON tem `1813.2252…` (PCMU). Esse limite inferior está arredondado para cima. Use "≥ 1813.22".
8. `tests/probes/secure_controls.rs` (`NotifyLog::wait`): se os NOTIFYs vierem sem `id=`, `event_ids` fica vazio e a correlação acontece apenas por dialog armado. Para o primeiro REFER do dialog isso é aceitável pela RFC 3515, mas o receipt diz "correlated" sem registrar se o `id` existia e batia com o CSeq. Recomendo emitir um booleano `refer_id_matched` no JSON e fazer o observer exigir esse campo, ou então qualificar a frase no receipt.
9. JSON do receipt: os valores de `resumed` são iguais aos de `before`, e os de `before` da transferência são iguais aos do hold. A causa é determinística, não cópia. O gerador usa `sample % 8000`, ou seja, um período de 50 frames, e 100 frames cobrem exatamente dois períodos. Assim, o RMS e a razão entre os tons não mudam com o alinhamento. A diferença `1670.404949473175` contra `…753` vem da ordem da soma. Vale uma nota curta no receipt para um leitor não suspeitar de cópia manual. Isso também significa que essas métricas, sozinhas, não distinguem uma janela da outra. O que garante que a mídia é recente é a proteção anti-replay do SRTP.
10. `tests/probes/secure_call.rs` (`with_allows`): os dois endpoints agora anunciam `REFER`, mas o diff não mostra uma rejeição explícita para REFER recebido. Não é alcançável no laboratório, mas um ator de produção deve responder 4xx de forma explícita.
11. `tests/probes/secure_controls.rs`: `response.body.clear()`, `answer.body.clear()` e `reoffers[i].to_vec()` não apagam o material de chave da memória. Isso já aparece como "own/SDK key erasure" nos gates abertos. Apenas registro.
12. `docs/secure-call-experiment.md`: o texto deixou de citar "secure call controls" na lista do que não está concluído, mas também não cita "application adoption", que o próprio receipt exclui. Sugiro acrescentar essa ressalva.

