## Purpose

Permitir configuração local simples por arquivo ou ambiente e persistência SQLite, com precedência verificável, proteção de credenciais e preservação de dados em falhas.

## ADDED Requirements

### Requirement: Precedência determinística
O aplicativo SHALL resolver cada campo na ordem flags CLI > ambiente do processo > `.env` explícito > TOML explícito ou XDG > SQLite > defaults, distinguindo ausência de valor inválido. `.env` SHALL ser carregado apenas via caminho explícito, nunca por busca automática no diretório atual. SHALL mostrar origem e efetividade dos ajustes sem revelar segredos.

#### Scenario: CFG-01 precedência e origem
- **WHEN** o mesmo campo aparece em todas as fontes
- **THEN** vence a fonte de maior precedência, a UI indica sua origem e nenhuma override temporária é gravada automaticamente no SQLite

#### Scenario: CFG-02 arquivo inválido
- **WHEN** um arquivo explicitamente solicitado está ausente ou contém tipo, enum ou campo desconhecido
- **THEN** a validação falha antes de registro SIP e informa campo/caminho sem mostrar o conteúdo secreto, sem fallback silencioso

### Requirement: Persistência e edição
O aplicativo SHALL salvar preferências e configuração não secreta em SQLite embarcado nos diretórios XDG e oferecer edição validada na UI. Campos sobrepostos por fonte superior SHALL indicar a sobreposição e impedir uma falsa confirmação de mudança efetiva.

#### Scenario: CFG-03 ajuste durável
- **WHEN** um campo não sobreposto é salvo pela UI e o aplicativo reinicia
- **THEN** o novo valor reaparece sem servidor de banco ou configuração externa obrigatória

#### Scenario: CFG-04 override externo ativo
- **WHEN** o usuário tenta editar na UI um campo fixado por ambiente ou TOML
- **THEN** recebe indicação da fonte que deve alterar e o aplicativo não afirma que uma mudança ineficaz foi aplicada

### Requirement: Segredos fora do banco
O aplicativo MUST NOT gravar senhas SIP em claro em SQLite, logs, exports ou diagnósticos. SHALL guardar apenas referência ao Secret Service quando disponível; na ausência dele SHALL aceitar segredo efêmero de ambiente/arquivo explícito privado ou pedir senha por sessão, sem impedir uso básico. Arquivos privados e banco SHALL usar permissões 0600 e diretórios 0700.

#### Scenario: CFG-05 credencial e ausência de keyring
- **WHEN** a senha é fornecida com ou sem Secret Service disponível
- **THEN** o registro pode ser realizado, o SQLite contém no máximo referência ao segredo e um export não contém senha, token ou chave de mídia

#### Scenario: CFG-06 arquivo de segredo inseguro
- **WHEN** um arquivo `.env` ou TOML contendo segredo é legível por outros usuários
- **THEN** o aplicativo recusa seu uso e explica como corrigir permissões sem imprimir o segredo

### Requirement: Migração e falhas preservam dados
O aplicativo SHALL usar migrações versionadas e transacionais, backup consistente antes de migrar e recusar banco de versão futura. Falta de espaço, lock, corrupção ou erro de migração MUST preservar o arquivo existente e mostrar caminho de recuperação, nunca recriá-lo silenciosamente.

#### Scenario: CFG-07 migração interrompida
- **WHEN** o processo é interrompido durante uma migração
- **THEN** a próxima abertura encontra versão anterior íntegra ou versão nova completa, com backup recuperável e sem estado parcialmente aplicado

#### Scenario: CFG-08 banco indisponível ou futuro
- **WHEN** o banco está bloqueado, corrompido, sem espaço ou tem versão futura
- **THEN** o aplicativo informa a falha, não apaga dados e não registra conta antes de resolver o armazenamento obrigatório

### Requirement: Importação e exportação conscientes
O aplicativo SHALL exportar TOML não secreto e importar configuração por confirmação explícita e transação, preservando fontes externas. Configuração INI antiga do GOnnect MUST NOT ser lida ou convertida automaticamente.

#### Scenario: CFG-09 importação sem tocar o legado
- **WHEN** o usuário importa TOML válido e depois exporta sua configuração
- **THEN** vê uma prévia sanitizada, pode confirmar a gravação no SQLite e os arquivos do aplicativo instalado permanecem intactos
