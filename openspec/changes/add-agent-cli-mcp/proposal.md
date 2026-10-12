## Why

Operadores e agentes precisam consultar estado e controlar o rust-microsip sem automatizar pixels. Um cliente CLI e um servidor MCP locais devem usar os mesmos serviços e limites do desktop, evitando registros SIP duplicados, gravações diretas no banco e chamadas inesperadas.

## What Changes

- Acrescentar modos do mesmo executável: `rust-microsip cli ...`, `rust-microsip mcp --stdio` e `rust-microsip service` headless opcional iniciado explicitamente.
- Manter um processo proprietário por usuário/perfil, responsável por SIP, áudio, SQLite e autorização; CLI/MCP são clientes via IPC protegido.
- Consultar status, dispositivos, contatos, histórico e configuração sanitizada; controlar discagem, atender/recusar/encerrar, DTMF, mute, espera, transferência cega e DND.
- Default MCP somente leitura; controles exigem aprovação humana por operação ou concessão local explícita, limitada e revogável.
- Oferecer saídas JSON versionadas, erros tipados, idempotência de mutações e auditoria sanitizada.
- Incluir teste real CLI/MCP com Asterisk e documentação de conexão a agentes sem instalar/configurar clientes automaticamente.

## Capabilities

### New Capabilities

- `agent-cli-mcp`: CLI e MCP stdio, proprietário único, autorização, privacidade e contratos testáveis.

### Modified Capabilities

Nenhuma spec canônica existente. Complementa a mudança desktop ainda não implementada.

## Impact

Depende de [create-native-rust-linux-release](../create-native-rust-linux-release/proposal.md), especialmente G0, domínio, IPC, plataforma e configuração. Planejar crates application/control/cli/mcp e modos do mesmo binário, sem daemon obrigatório, HTTP público, dependência de Node ou chamada de LLM embutida. Esta extensão integra o candidato Linux solicitado, após G0; publicação do conjunto depende dos gates das duas mudanças. Não habilita gravação, transcrição, PSTN automático, multi-chamadas ou consulta de segredos. Artefatos são somente planejamento; nenhum CLI/MCP implementado.
