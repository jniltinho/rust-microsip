# Claude static rekey call review — 2026-10-13

Sanitized, tools-disabled static review only; not runtime, G0 or release approval. Raw JSON and prompts stay in ignored `artifacts/`.

## Round 1

- Prompt SHA-256: `4244ee2974398a902c5318c0aea21b2e3eb769e2f41cd3b34f254f42ef3b273f`; raw JSON SHA-256: `6b10d8ca42e1426157170f50555da7a358e9b79e7036be9930bbc61f8dda8500`.

### Verbatim result

BLOCK

1. **`tests/probes/secure_call.rs` (`measure`, braço `rekey` do `select!`) e receipt: a afirmação "index zero" não corresponde ao código.** Na troca, o código substitui apenas `sender` e `ssrc`. As variáveis `sequence` e `timestamp` continuam com os valores anteriores. `SecureSender::new(REKEYED_SSRC, key)` não recebe base de sequência, então o índice SRTP inicial do novo SSRC é `ROC=0 ‖ seq corrente`, e não zero. Com os números do receipt (o `peer_highest_sequence` do caller é 65749 perto de 6 s), o novo SSRC começa próximo de 65.7xx e faz rollover logo em seguida.

   A propriedade de segurança se mantém: a chave é nova e aleatória, então não há reuso de (key, index). O texto da evidência, porém, está errado nos seguintes pontos:
   - o doc de `REKEYED_SSRC` ("starts a new SSRC at index zero");
   - o comentário "the index restarts on a new SSRC";
   - o receipt ("fresh key + fresh SSRC + index zero" e a linha "New SSRC, index zero" da tabela de discovery, que veio de build não pinado e cujo comportamento não é verificável);
   - `rekey_policy` no JSON.

   Correção mínima: trocar a redação por "ROC zero, sequência continuada" em todos esses pontos. A alternativa é zerar ou randomizar `sequence` na troca, recompilar e repinar os hashes.

2. **Observer/probe: a frase "callee kept hearing the caller's tone after the switch" não tem gate.** `meter` agrega a chamada inteira. Se o callee recebesse silêncio depois do switch, o RMS total cairia para cerca de 3,4k, ainda acima do limite de 1000, e o power ratio continuaria ≥20. O RMS mínimo gravado (5635.86) sustenta a frase numericamente, mas nada a impõe. Não bloqueia. Recomendo medir RMS e ratio só depois do switch no callee e exigir limites para eles.

3. **`secure_call.rs` (`renegotiate`): `let initial = response.body.clone()` cria mais uma cópia não zeroizada da linha de chave do PBX.** Essa cópia vive durante toda a chamada. Envolver em `Zeroizing`. O `.to_vec()` da oferta com a nova chave segue a limitação já declarada na ADR 0011, então não é achado novo.

4. **`secure_call.rs`: a resposta do rekey compara só `answer.address` com `remote.address`, sem comparar o endereço de controle.** Se o PBX movesse a porta RTCP, o SRTCP iria para a porta antiga. O gate `peer_reports_for_rekeyed_source > 0` pegaria isso indiretamente. Não bloqueia. Uma comparação explícita custa uma linha.

5. **`ControlMeasurement::received_after_rekey` mede RTP, não RTCP, e é incrementado antes de `tracker.observe`.** Por isso inclui pacotes tardios. No caller, o valor prova continuidade do fluxo PBX→caller sob a chave do PBX, que não mudou, e não prova a nova chave. No callee, ele usa a flag compartilhada `switched`. Os docs do campo descrevem isso corretamente. Fica como nota de nomenclatura e não bloqueia.

**Verificado sem problemas:**
- O fail-closed funciona: falha no `renegotiate` derruba `rekey_tx`, o que gera `ok_or(Signaling)` no caller, e `!switched` gera erro de Media.
- A troca do lado (`'second' if reverse`) no observer está correta.
- A oferta plaintext não leva chave na linha.
- A versão em `o=` é 1→2→3.
- O 488 é exigido.
- Uma mudança de chave do PBX falha.
- Os pins de `sdes.rs`, `secure_call.rs` e dos dois `secure_call` example são consistentes entre `probe_files_sha256` e `candidate_inputs_sha256`.

Esta é uma revisão estática. Não afirmo aceitação em runtime.

## Round 2

- Prompt SHA-256: `455be6d0ead772cf0c158cae979ebf55e204c2ac4a6dd1af59c2f054e51b6986`; raw JSON SHA-256: `ff034d747aaf2f156eddf62b96ad7a50e3ae85c90164c68ae046f46c3c15af11`.

### Verbatim result

BLOCK

1. **Bloqueante de honestidade da evidência: a afirmação "o novo stream cruza um rollover sob a nova chave" não tem gate e os números gravados indicam que ela é falsa. Corrijo também a leitura que fiz no round 1.** A afirmação aparece em quatro lugares: doc de `REKEYED_SSRC`, receipt ("about 65.7k at the switch… crosses a rollover under the new key a few seconds later") e `rekey_policy` no JSON.
   - `peer_highest_sequence` é o número estendido de RFC 3550 (ciclos·65536 + seq). O valor 65749 não pode ser uma sequência bruta de 16 bits. Ele equivale a 1 ciclo + 213, então "~65.7k no switch" é impossível. A leitura "perto de 6 s → rollover logo em seguida" do round 1 estava errada.
   - Os números do callee, que não faz rekey, permitem estimar o início. 66250 = wrap + 714, e o callee recebeu 3 reports em 16 s. Isso coloca a sequência inicial a algumas dezenas de pacotes do wrap, consistente com o `timestamp` inicial `u32::MAX - 3200`. Nesse cenário, o caller faz wrap em ~0,5–1 s, e o 65749 (wrap + 213, ~4–5 s) vem do report pré-switch sobre o SSRC antigo. No switch (~pacote 300), a sequência bruta fica em ~260. O novo SSRC começa em ROC 0 com seq ~260 e **não** faz rollover durante a chamada.
   - O próprio receipt confirma isso na seção de discovery: "re-INVITEd with a fresh key **after the caller's sequence had rolled over**". Isso contradiz a seção de execução.
   - O gate `peer_highest_sequence > 65535` é um `max` que agrega os blocos do SSRC antigo e do novo. Por isso não prova rollover sob a nova chave.
   - Não verifiquei o valor inicial de `sequence`, porque ele não aparece no diff. A contradição interna e o significado de `peer_highest_sequence` bastam para bloquear.
   - Correção mínima: redigir como "ROC zero, sequência continuada (já após o wrap, poucas centenas no switch); o novo stream não faz rollover na chamada; a evidência de rollover vem só do SSRC pré-switch". Remover o "about 65.7k". Alternativa: posicionar a sequência perto do wrap no switch e criar um gate separado, `highest_sequence > 65535`, só para blocos de `REKEYED_SSRC`. Nos dois casos é preciso repinar.
   - A propriedade de segurança se mantém: a chave é nova, então não há reuso de (key, index).

2. **Não bloqueia. A cópia `Zeroizing` não elimina a cópia original.** `response` continua no escopo durante toda a chamada com `response.body` não zeroizado. Mover em vez de clonar resolve: `Zeroizing::new(std::mem::take(&mut response.body))`, depois do último uso de `response.body`. O receipt diz apenas que "the initial PBX answer used for the key-line comparison is held in Zeroizing". Isso é literalmente correto, mas sugere mais do que entrega.

3. **Não bloqueia. "The echo proves … received RTP from the new source" é mais forte do que o gate.** O LSR ecoado prova que o PBX autenticou o SR sob a nova chave. Um report block sobre o SSRC não prova recepção de RTP. Quem prova a decifração do RTP com a nova chave é o gate pós-switch do callee (RMS/ratio e `received_after_rekey ≥ 200`). Sugiro atribuir a prova a esse gate.

**Verificado sem problemas:**
- Os itens 2, 3 (parcial) e 4 do round 1 foram resolvidos. O `after_meter` só recebe pacotes não tardios depois de `switched`, e `finish()` impõe os limites nos dois endpoints. O observer verifica `post_rekey_*` e `received_after_rekey ≥ 200`. A resposta do rekey compara `address` e `control`.
- No observer, `'second' if reverse` aplicado a `peer_reports_for_rekeyed_source` está correto.
- `rekeyed_sr` só contém SRs enviados sob `REKEYED_SSRC`, então o LSR não colide com SRs antigos.
- O fail-closed continua funcionando: o `rekey_tx` cai, o caller recebe `Signaling` e o callee gera `!switched → Media` dentro do timeout de 30 s.
- O `o=` usa as versões 1/2/3 e a oferta plaintext não leva linha de chave.
- Os pins de `sdes.rs`, `secure_call.rs` e do example são consistentes entre `probe_files_sha256` e `candidate_inputs_sha256`.

Esta é uma revisão estática. Não afirmo aceitação em runtime.

## Round 3

- Prompt SHA-256: `44b75225e8ce83b46f76250af57250e27823d05a0025a32ff6dab629ae8dc19d`; raw JSON SHA-256: `050a536349d4fe3c3e21f4bc10cbde1ad21b1faade4a3e9a68c86fdf55670b21`.

### Verbatim result

APPROVE

1. **Resolvido: o claim de rollover sob a nova chave agora tem gate e é aritmeticamente consistente.** No rekey, `sequence=65500` é atribuído ao `REKEYED_SSRC`. O wrap acontece após cerca de 36 pacotes, ou ~0,7 s. Com 499 pacotes depois do switch, o valor estendido final esperado é ≈ 65536 + 463 = 65999. Os valores reportados, 65948–65949 (ciclo 1, seq ≈ 412, ~448 pacotes após o switch), batem com o último RR do PBX cerca de 1 s antes do fim.
   - O novo campo só agrega blocos com `block.source == REKEYED_SSRC` cujo LSR pertence a `rekeyed_sr`, ou seja, SRs enviados sob a nova chave. Isso exclui o SSRC antigo.
   - O gate fica no probe, logo após o `join!`, e no observer, só para o caller (`'second' if reverse`). Está correto.
   - Agora o doc de `REKEYED_SSRC`, o texto de execução do receipt e `rekey_policy` são verdadeiros. A seção de discovery descreve outra configuração (sequência continuada) e não contradiz mais a execução.
   - A segurança se mantém: chave nova e SSRC novo, sem reuso de (key, index).

2. **Resolvido: `std::mem::take(&mut response.body)` em `Zeroizing`.** O take acontece depois do último uso de `response.body` em `sdes::parse`. Restam cópias que não são zeroizadas, mas já estão declaradas como cópias de SDK ou provider (ADR 0011): `accepted.body`, o `Vec` de `to_vec()` entregue ao `reinvite` e o `remote` retornado por `parse`. Isso não bloqueia.

3. **Não bloqueia: a atribuição "at both endpoints" ainda é um pouco mais forte que a evidência.** O receipt diz que a decifração do RTP sob a nova chave é estabelecida pelo meter pós-switch "at both endpoints". Quem prova o uso da nova chave é só o callee: o PBX precisou decifrar o RTP do caller (novo SSRC e nova chave) para reencaminhar o tom, e o ratio ≥ 20 garante que é o tom remoto. O meter do caller só prova que o RTP de entrada continuou sob a chave do PBX, que não mudou. Sugestão: "established by the callee's gated post-switch tone meter; the caller's meter shows continued inbound media under the unchanged PBX key".

4. **Não bloqueia: `received_after_rekey` conta pacotes antes de `tracker.observe`.** O contador inclui pacotes tardios e alguns pacotes da chave antiga ainda em trânsito no instante do switch. Com o gate ≥ 200 e 499 observados, a margem é ampla. `after_meter` só recebe pacotes não tardios, então o gate de RMS/ratio não é afetado.

5. **Observação, sem ação necessária.** O caller recebeu 800/800 pacotes, sem perda, com `authenticated_sources = 1`. Isso descarta, nesta execução, que o PBX tenha reiniciado o ROC do próprio stream de saída com a mesma chave e o mesmo SSRC. Se tivesse reiniciado, o receiver do caller falharia na autenticação. A troca de SSRC do PBX em direção ao callee (`authenticated_sources = 2`) usa a mesma chave com outro SSRC, então o IV é distinto e não há reuso de keystream.

**Verificado sem problemas:**
- Os pins de `sdes.rs`, `secure_call.rs` e do example são idênticos em `probe_files_sha256` e em `candidate_inputs_sha256`.
- O fail-closed continua valendo: `rekey_tx` cai → `Signaling` no caller, e no callee `!switched → Media`, tudo dentro do timeout de 30 s.
- O `plaintext()` compartilhado remove todas as linhas `a=crypto:` e tem teste.
- A comparação da linha de chave não decodifica nem loga a chave.
- O `o=` usa as versões 1/2/3.

Esta é uma revisão estática. Não afirmo aceitação em runtime.

