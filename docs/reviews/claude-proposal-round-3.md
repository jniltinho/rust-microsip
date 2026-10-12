# Revisão Claude CLI — rust-microsip, CLI/MCP e web

Data: 2026-10-12. Somente planejamento sanitizado.

# Veredito: **REVISE**

A proposta desktop continua coerente após a primeira revisão, e o recorte web como release posterior está bem delimitado. Os bloqueadores estão na extensão CLI/MCP e na forma como ela interage com o desktop: a fronteira de autorização, a semântica de idempotência e restart e o proprietário headless.

---

## Bloqueadores

### B1. A aprovação por TTY não fica fora do canal do agente
**Onde:** `add-agent-cli-mcp/design.md`, seção "Autorização…": *"CLI humano pode aprovar via terminal TTY local dedicado; comando approve/grant não é tool MCP"*. Também *"concessão criada explicitamente pelo humano antes do agente"*.

**Problema:** agentes como Claude Code e Codex têm shell com o mesmo UID. Eles podem executar `rust-microsip cli approve`/`grant` e alocar um pty com `script` ou `expect`. Ter TTY não prova presença humana. Com isso, o requisito confirmado ("autorização fora do canal do agente") e o cenário AG-04 ("tenta autoaprovar") passam a afirmar uma proteção que não existe.

**Correção mínima:**
- Restringir aprovação por operação à GUI do proprietário.
- Para o modo `service`, permitir a criação de grant apenas antes da sessão do agente, pela GUI ou pelo comando CLI.
- Declarar no threat model e em AG-04 que um agente com shell irrestrito no mesmo UID está fora da fronteira e equivale ao próprio usuário.
- Acrescentar a AG-04 o caso "agente invoca `cli approve` via pty" com o resultado esperado: GUI exigida, ou "fora do modelo" documentado na doc de configuração de agentes.

### B2. O namespace de idempotência contradiz AG-05 e o fluxo de aprovação fica indefinido
**Onde:** `design.md`, seção "Idempotência": *"Ledger … associa chave, perfil, escopo autenticado, hash do payload"*. Spec AG-05: *"CLI e MCP repetem dial … com a mesma chave → no máximo um efeito"*. Seção "Autorização": *"aprovação … single-use vinculada ao payload/request_id"*. Prazo do cliente: 5 s.

**Problemas:**
1. Se a chave inclui o escopo autenticado, CLI e MCP com grants diferentes caem em namespaces distintos e podem gerar dois INVITEs, o que viola AG-05.
2. O fluxo `approval_required` → humano aprova (até 60 s) → ? não está definido. Não se sabe se o proprietário executa a intenção pendente ou se o cliente reenvia. Se reenviar, o novo `request_id` quebra o vínculo da aprovação.

**Correção mínima:**
- Usar como chave `(perfil, epoch, idempotency_key)`. O mesmo key com escopo ou payload diferente retorna `conflict`.
- Vincular a aprovação a `idempotency_key + hash do payload`, não ao `request_id`.
- Especificar um dos dois fluxos e testá-lo em AG-04/AG-05:
  - (a) a intenção fica pendente no proprietário e executa após aprovação, consultável por `operation_get`; ou
  - (b) o cliente reenvia com a mesma chave.

### B3. A semântica de restart e epoch é contraditória
**Onde:** `design.md`: *"persistir aceitação/resultado … em SQLite … antes de executar"*, *"rejeitar chave … de epoch de proprietário antigo"* e *"negar tudo após restart"*.

**Problema:** o ledger é durável justamente para sobreviver a crash, mas a chave de epoch antiga é rejeitada. Fica indefinido se, após o restart, o cliente consegue obter `outcome_unknown` ou o resultado terminal, e se `operation_get` funciona entre epochs. Sem isso, o crash em AG-05 não tem resultado verificável.

**Correção mínima:**
- Após restart, uma chave de epoch anterior retorna, somente para leitura, o resultado persistido ou `outcome_unknown`, e nunca executa.
- `operation_get` consulta entre epochs dentro da janela de 24 h, sujeito a um grant de leitura vigente.
- Mutações de epoch antiga recebem um erro tipado distinto de `conflict`.
- Escrever isso como cenário explícito em AG-05.

### B4. Proprietário headless e confirmação de URI e operações
**Onde:** desktop `linux-integration/spec.md` LIN-03 (*"a janela original apresenta a confirmação"*) e LIN-02. Extensão: *"GUI e service disputam o mesmo lock"* e *"GUI mostra origem … usuário confirma ali"*.

**Problema:** quando o `service` é o proprietário, não existe janela.
- Uma URI `tel:`/`sip:` encaminhada não pode ser confirmada.
- Uma GUI iniciada depois perde o lock, e não está definido se ela vira cliente IPC, se recusa a abrir ou se mostra um aviso.
- Operações por aprovação ficam sem canal.

**Correção mínima:**
- No modo `service`, rejeitar URIs encaminhadas com erro visível no processo invocador.
- Definir o comportamento da GUI quando o `service` é o dono: sair com mensagem, ou atuar como cliente somente leitura, sem registro.
- Para `approval_required` em `service`, responder "use grant prévio".
- Acrescentar um cenário em LIN-03 ou AG-01.

### B5. O payload de DTMF vaza pelo hash durável
**Onde:** `design.md`, ledger *"hash do payload"* persistido por 24 h, contra a spec AG-05 *"sem dígitos DTMF"*.

**Problema:** sequências DTMF (PINs, senhas de URA) são curtas. Um hash simples no SQLite é revertido por força bruta trivial, o que contorna o MUST da spec.

**Correção mínima:** usar HMAC com chave aleatória por epoch mantida só em memória, ou não persistir o hash de payload em operações DTMF. Incluir esse caso no scanner de segredos (task 2.5).

### B6. A ordem de aprovação e de gates é contraditória entre as mudanças
**Onde:** desktop `tasks.md` 1.1 (*"Obter aprovação desta proposta e da extensão CLI/MCP vinculada"*, antes de G0) e CLI `tasks.md` 1.1 (*"Aprovar esta extensão junto ao desktop após G0"*). Além disso, o `test-plan.md` G0–G5 não posiciona os gates AG.

**Correção mínima:**
- Fixar uma única ordem, por exemplo: aprovar o planejamento das duas mudanças antes de G0 e iniciar a implementação CLI/MCP só após G0.
- Inserir os cenários AG-01..06 na sequência de gates do `test-plan.md`, por exemplo como G3b, antes de G4/G5.
- Fazer task 9.10 e CLI 3.5 referenciarem o mesmo gate.

---

## Observações não bloqueadoras

**Escopo e documentação**
- O `README.md` lista o que "o primeiro release proposto inclui", mas omite CLI/MCP, que agora integra o candidato Linux. Acrescentar uma linha.
- A spec AG-02 diz *"Config validate/import/export e doctor existentes"*. Trocar "existentes" por "planejados na mudança desktop", para não sugerir implementação.
- O proposal web lista os controles sem DND, mas a spec WEB-02 exige DND. Alinhar.

**Proprietário e IPC**
- O `service` "sem sessão gráfica" ainda depende de `XDG_RUNTIME_DIR`, de PipeWire/PulseAudio do usuário e do D-Bus de sessão para o Secret Service. Documentar os pré-requisitos (logind/linger) e o erro tipado quando faltarem.
- O limite de 10 req/s é por conexão. Vários processos MCP abrem várias conexões. Acrescentar um limite global no proprietário.
- `status_get` por default não pode expor o número remoto da chamada ativa. Mascarar sem grant de leitura.

**Grants**
- `call_answer` não tem escopo próprio. A allowlist de destinos cobre apenas a discagem. Definir se o answer exige grant separado ou filtro por origem.
- Um grant de leitura pode ser solicitado por um agente e aprovado sem atenção. A GUI deve mostrar escopo, prazo e quota antes da confirmação.

**Desktop**
- O G0 inclui "IPC de produção", mas o contrato IPC (handshake v1, `SO_PEERCRED`) está detalhado só na extensão. Referenciar esse contrato a partir de `design.md` §1 e da task 1.8.
- Com armazenamento indisponível (CFG-08), as mutações via CLI/MCP devem falhar antes de executar, já que o ledger é gravado antes. Explicitar isso.

**Web**
- O SIP.js registra mensagens SIP no console por padrão, inclusive Authorization/digest. Exigir nível de log configurado e incluir no teste WEB-04.
- O Asterisk não valida `Origin` do WSS de forma nativa. Tornar o reverse proxy obrigatório no guia de deploy, não "proxy/PBX".
- A credencial TURN efêmera pressupõe um emissor (REST com segredo compartilhado), ou seja, um serviço externo mesmo no "modo básico". Deixar isso explícito no WEB-05 e no guia, inclusive como ela chega ao formulário no laboratório.
- Memória WASM e JS não é zeroizável. Registrar essa limitação junto à afirmação "senha só em memória".
- `wasm-unsafe-eval` será quase certamente necessário para eframe/wasm-bindgen. Tratar como esperado no ADR, não como exceção.

**Gates**
- O smoke em Wine/Xvfb está corretamente limitado a layout. Manter essa distinção nos recibos de G3.
- A contagem de 46 cenários desktop confere: 6 + 9 + 4 + 4 + 6 + 7 + 10.
