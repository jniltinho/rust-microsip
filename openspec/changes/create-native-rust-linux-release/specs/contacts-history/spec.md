## Purpose

Permitir que o usuário encontre contatos, acompanhe chamadas e mantenha favoritos localmente, sem exigir serviços externos ou compartilhar sua agenda.

## ADDED Requirements

### Requirement: Contatos e favoritos locais
O aplicativo SHALL criar, editar, remover e pesquisar contatos locais com nome e múltiplos números, mantendo favoritos persistentes. Selecionar um contato SHALL preencher uma chamada proposta, sem discagem automática inesperada.

#### Scenario: CH-01 contato e favorito
- **WHEN** um contato com dois números é salvo como favorito e o aplicativo reinicia
- **THEN** contato e favorito reaparecem e o usuário pode escolher o número antes de confirmar a chamada

### Requirement: CSV validado e atômico
O aplicativo SHALL importar CSV UTF-8 com colunas nome e número, oferecer prévia/contagem de duplicatas e rejeitar conteúdo inválido sem importação parcial. SHALL impor limite de 10 MiB e 100 mil registros por arquivo.

#### Scenario: CH-02 CSV inválido e duplicado
- **WHEN** o CSV contém registros repetidos, números inválidos ou ultrapassa os limites
- **THEN** a prévia informa os problemas e nenhuma gravação parcial acontece; arquivo válido só é aplicado após confirmação

### Requirement: Histórico coerente
O aplicativo SHALL registrar direção, destino, início, atendimento, fim, resultado e duração da chamada, persistir chamadas perdidas e permitir filtros e rediscagem confirmada. Uma sessão recuperada após crash SHALL indicar interrupção, nunca atendimento fictício.

#### Scenario: CH-03 resultados e recuperação
- **WHEN** ocorrem chamada atendida, perdida, cancelada e interrupção abrupta do processo
- **THEN** cada sessão aparece uma única vez com resultado correto e a duração considera apenas tempo efetivamente estabelecido

### Requirement: Busca e privacidade
O aplicativo SHALL pesquisar nome e número localmente em contatos, favoritos e histórico, sem enviar consultas a serviço externo. SHALL permitir apagar histórico e configurar retenção, com default de 90 dias.

#### Scenario: CH-04 busca e retenção
- **WHEN** o usuário busca um contato e depois confirma exclusão do histórico ou expira a retenção
- **THEN** a busca não gera tráfego externo e as entradas removidas deixam de aparecer, sem prometer apagamento forense de backups
