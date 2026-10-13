# Claude static SRTCP call review — 2026-10-13

Sanitized, tools-disabled static review only; not runtime, G0 or release approval. Raw JSON and prompts stay in ignored `artifacts/`.

## Round 1

- Prompt SHA-256: `a336168deb7511da0b1c8d7d23667ed77707574b537d1609c8ef4847be057104`; raw JSON SHA-256: `1b40882b1ef7ee4143478af8da00ea2129bb4dd47db4e22a4df7627ef9b1647c`.

### Verbatim result

BLOCK

**Bloqueante (honestidade de evidência)**

1. **`tests/probes/observe_secure_call.py` (`elf=work/...`, cerca da linha 35): o hash do ELF pode não corresponder ao binário executado.** O observer agora recebe o diretório (`srtcp-call-work`) e calcula o hash de `artifacts/<name>/target/.../secure_call`. Antes, esse caminho era fixo em `secure-call-work`. Isso indica que a montagem do binário em `docker/asterisk/compose.calls-g0.yaml`, ou no comando de execução fora do diff, também é fixa. O diff não mostra parametrização dessa montagem, nem uma comparação do source do mount com `elf` dentro de `hardened`.
   - Consequência: a identidade `5427e8e5…` do recibo pode estar ligada a um arquivo diferente do que rodou.
   - As asserções de `srtcp_tested` e `control` só pegam um binário antigo sem SRTCP. Não pegam um binário divergente com SRTCP.
   - Desbloqueia se: o código oculto já verifica `mount.Source == elf` (é só mostrar), ou se a montagem passar a ser derivada de `name` com essa verificação.

2. **Escopo incompleto para as alegações do recibo.** O diff não inclui `tests/probes/secure_call_inputs.json`, com `probe_files_sha256` contendo `rtcp.rs` e os 39 `candidate_inputs_sha256`. Sem essa atualização, `prepare_secure_call.py` falha no allowlist. Também faltam as sentinelas do controle Docker-context ("`rtcp.rs` added to the probe sentinels") e o recibo JSON. As afirmações "binds 39 inputs" e "controls passed with rtcp.rs" não são verificáveis nesta revisão.

**Não bloqueantes (segurança e correção)**

3. **`tests/probes/srtp_adapter.rs`, `ReceiverBank::receive`: um erro de um contexto descartável fecha a época inteira.** No caminho de fonte desconhecida, ainda antes de autenticar, um `Err(SecureError::Provider)` vindo de um `SecureReceiver` temporário cai no `matches!(…Provider…) → self.close()`. Se algum pacote forjado conseguir provocar `Provider` antes da autenticação, um único datagrama mata todos os contextos e o histórico de replay. Isso contradiz o contrato "known-source state remains intact". Correção mínima: só escalar para `close()` quando o erro vier de um contexto retido.

4. **`tests/probes/secure_call.rs`, `measure`: abortos pré-autenticação.** Os testes `length!=182` (RTP) e `!(22..=1514)` (RTCP) retornam `Err(Media)` antes de qualquer autenticação. Basta um datagrama falsificado com o endereço do PBX para derrubar a chamada. Isso é inconsistente com a nova postura de `continue` para erros de autenticação e replay. Aceitável no laboratório com rede interna, mas é DoS não autenticado.

5. **`tests/probes/observe_secure_call.py`, chave `peer_lsr_and_rollover_independently_reported`, e a doc de `SecureCallReport` ("independently observed").** Os valores vêm do PBX, mas são decodificados e filtrados pelo próprio binário sob teste. O observer só lê o JSON emitido por ele. Não há verificação independente, como estatísticas RTCP do Asterisk via CLI. Sugiro renomear para `peer_lsr_and_rollover_reported_by_probe` ou adicionar uma checagem pelo lado do PBX.

6. **Lista de negativos no `secure_call_example.rs`.** `per_endpoint_actual_packet_negatives` não lista os negativos SRTCP (tag, index, E-bit, clear, replay). Eles existem apenas como booleano agregado (`packet_negatives_verified`). O recibo os enumera individualmente.

7. **Doctest `compile_fail` de `ReceiverBank: !Clone`.** Ele não fixa o código de erro, então pode passar por outro motivo, como um caminho de módulo errado. Recomendo `compile_fail,E0277`.

**Revisado sem achados:** os limites e o parsing de `rtcp.rs` (padding apenas no final, CNAME exigido e único, alinhamento zerado, no máximo 31 blocos, no máximo 1500 bytes), o trailer SRTCP (+14, offsets de index e E-bit), a extração de LSR (`ntp>>16`), a paridade de porta par/+1 em `sdes.rs`, `bind_media`, o `AuthBudget` (tempo retrocedendo, capacidade checada antes do orçamento, sem evicção) e o limite de `MediaTrackers`.

Não afirmo aceitação em runtime. A revisão foi estática, e as contagens de testes e as medições do recibo não foram verificadas.

## Round 2

- Prompt SHA-256: `d773685ca669ba70153779ab7c48757ca61ffe68830f613ff3ba6b202ec7c0d8`; raw JSON SHA-256: `bcd1591754f4314c89f1cf0749ce6416c765cc98d8622d784372587b8f4538f6`.

### Verbatim result

APPROVE

O escopo é o material mostrado nesta rodada. Não há aceitação em runtime: contagens de testes, medições, hashes de log e o ELF `5427e8e5…` não foram verificados.

**Situação dos achados da rodada 1**

1. **Resolvido (identidade do ELF).** O probe roda via `docker run` com `--mount type=bind,source={elf},target=/probe,readonly`, e `elf` é derivado de `name`. Antes de `hardened=True`, o observer confere `sha256sum /probe == elf_hash` dentro do container. Ele também exige exatamente os binds `/lab` e `/probe`, ambos read-only. O `AssertionError` não é capturado pelo `except CalledProcessError`, e `assert hardened` precede o `save`. A troca do arquivo entre o hash e a montagem seria detectada. A escrita in-place no executável em uso esbarra em `ETXTBSY`.
2. **Resolvido no que é mecânico.** As entradas de `probe_files_sha256` batem byte a byte com as entradas correspondentes de `candidate_inputs_sha256`: `rtcp.rs` `132c…`, `sdes.rs` `9bad…`, `secure_call.rs` `6e64…`, exemplo `87bb…` e `srtp_adapter.rs` `c355…`. Exatamente uma entrada nova (`crates/telephony/src/rtcp.rs`) é coerente com `len(prepared)==39`. `check-context.sh` copia `rtcp.rs` e o inclui nas sentinelas.
3. **Aceito como limitação documentada.** Pelo mapeamento descrito, `Provider` só nasce de `UnsupportedProfile`, ou da construção do contexto, e não do conteúdo do pacote. A frase do recibo está coerente com isso.
4. **Aceito como limitação documentada.**
5. **Resolvido.** A chave foi renomeada e a limitação está explícita no recibo.
6. **Aceito como limitação documentada.**
7. **Aceito como limitação documentada.**

**Não bloqueantes restantes**

1. **O recibo JSON (`g0-srtcp-call-2026-10-13.json`) continua fora do material revisado.** Ele foi pedido no item 2 da rodada 1, mas só o `.md` veio. O observer mudou depois da primeira execução, com a chave renomeada, e foi re-executado. Por isso, o JSON precisa refletir três coisas:
   - o hash atual de `observe_secure_call.py`;
   - o `-proof.json` com `peer_lsr_and_rollover_reported_by_probe`;
   - os hashes de log da nova execução.
   
   As alegações "binds … harness hashes … log hashes" não foram verificadas aqui.
2. **Pequena imprecisão no recibo.** Ele diz que as negativas SRTCP são "asserted individually with exact typed errors". Mas os dois casos abaixo usam apenas `.is_ok()`, sem erro tipado exato:
   - o pacote claro valid-shaped: `receiver.unprotect_rtcp(&clear,now).is_ok()`;
   - o caso análogo no RTP, com `&send`.
   
   Além disso, `authenticated_sources()!=known` não é rechecado depois dos testes de replay e de pacote claro no RTCP. Sugiro trocar a frase por "exact typed errors for tag, index and E-bit; clear and replay rejected".
3. **Mecanismo de inclusão de `rtcp.rs` não visível.** O hash de `crates/telephony/src/lib.rs` (`6e3af2c7…`) não mudou. Então `rtcp.rs` provavelmente entra por `mod`/`#[path]` em `secure_call.rs`, o mesmo mecanismo já usado por `sdes.rs`. Isso não aparece no material, mas a compilação offline declarada cobre o ponto. Se o build tivesse falhado, não haveria ELF.

