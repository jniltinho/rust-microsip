# gonnect-rust

Proposta para um softphone desktop nativo em Rust, inspirado no [GOnnect](https://github.com/gonicus/gonnect), com primeiro release voltado ao Linux.

**Estado: planejamento, sem aplicativo ou release compilado.** Não é uma distribuição oficial da GONICUS.

## Proposta

[Proposta OpenSpec](openspec/changes/create-native-rust-linux-release/proposal.md) · [Arquitetura](openspec/changes/create-native-rust-linux-release/design.md) · [Tarefas](openspec/changes/create-native-rust-linux-release/tasks.md) · [Plano de testes](openspec/changes/create-native-rust-linux-release/test-plan.md) · [Fontes e evidências](docs/reference-sources.md)

O primeiro release proposto inclui:

- Interface equivalente às telas essenciais do GOnnect, com temas claro, escuro e do sistema.
- Registro SIP e chamadas de voz reais, dispositivos de áudio, DTMF, espera e transferência cega.
- Configuração TOML ou `.env`, ajustes pela UI e SQLite embutido, sem servidor de banco.
- Contatos locais/CSV, favoritos, histórico e integração básica ao desktop Linux.
- Um executável com recursos da aplicação incorporados; as dependências do sistema Linux serão explicitamente auditadas. AppImage adicional não substitui silenciosamente esse requisito.
- Build Docker sem instalar o toolchain Rust no host e laboratório Asterisk descartável em Docker.

## Revisar e aplicar depois

Requer OpenSpec CLI 1.10.0 para os comandos de planejamento:

```sh
openspec status --change create-native-rust-linux-release
openspec validate create-native-rust-linux-release --strict --no-interactive
```

A implementação só começa após aprovação da proposta. No Codex: `$openspec-apply-change`; no Claude, usar o fluxo OpenSpec instalado ou pedir para aplicar a mudança.

O comando de build Docker e o laboratório são contratos planejados, descritos no design; **ainda não existem Dockerfile, compose ou binário Rust neste repositório**.

## Referências locais e skills

`base/` é apenas material local, ignorado pelo Git e não necessário para consultar a proposta num clone novo. As referências reproduzíveis estão documentadas por URL e revisão, sem incluir fontes, AppImages, logs pessoais ou bancos de dados.

Skills Rust instaladas no projeto: Apollo `rust-best-practices`, `rust-async-patterns` e `rust-testing`, compartilhadas entre Codex e Claude. São apoio ao desenvolvimento, não dependências do aplicativo. [Proveniência](docs/development-skills.md).
