# Skills de desenvolvimento

Instalação local em 2026-10-12, solicitada pelo usuário. Arquivos em `.agents/skills/`; links relativos em `.claude/skills/`. `skills-lock.json` registra as origens e hashes fornecidos pelo instalador. Não são dependências de runtime ou build do futuro aplicativo.

| Skill | Origem | Uso nesta proposta |
|---|---|---|
| rust-best-practices | https://github.com/apollographql/skills/tree/main/skills/rust-best-practices | Ownership, erros tipados, clippy, rustdoc e separação de APIs |
| rust-async-patterns | https://github.com/wshobson/agents/tree/main/plugins/systems-programming/skills/rust-async-patterns | Tokio separado da UI, canais limitados, cancelamento e shutdown |
| rust-testing | https://github.com/affaan-m/ecc/tree/main/skills/rust-testing | TDD, testes unitários, integração e propriedades |
| openspec-* | https://github.com/Fission-AI/OpenSpec | Proposta, specs, design e tarefas; geradas por OpenSpec 1.10.0 |

As instruções gerais do repositório têm precedência. Exemplos das skills não devem ser copiados mecanicamente: o projeto não adotará `unwrap()` em produção, tarefas ilimitadas ou callbacks de áudio bloqueantes. Sugestões de bibliotecas exigem auditoria de licença e validação técnica na implementação.

O REA (https://github.com/morluto/rea) foi consultado, mas **não instalado**: a análise de código disponível e o graph MCP bastaram nesta etapa. A autorização condicional do usuário não exige alterar configurações globais do Grok, Claude ou Codex sem necessidade.

Os textos MIT das três skills Rust estão ao lado de cada skill. As referências foram normalizadas apenas em whitespace final; o lock mantém os hashes da distribuição upstream capturada pelo instalador.
