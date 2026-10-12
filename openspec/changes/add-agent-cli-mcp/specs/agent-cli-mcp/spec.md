## Purpose

Permitir operação local por terminal e agentes com CLI e MCP verificáveis, mantendo um único proprietário de telefonia e armazenamento e autorização explícita para efeitos externos.

## ADDED Requirements

### Requirement: Proprietário único e modos locais
O executável SHALL oferecer GUI, service headless explícito, cliente CLI e servidor MCP stdio. CLI/MCP SHALL conectar ao proprietário do perfil via IPC local autenticado pelo usuário do SO, sem abrir SQLite, registrar SIP ou capturar áudio por conta própria. Sem proprietário SHALL retornar engine_unavailable, sem iniciá-lo silenciosamente.

#### Scenario: AG-01 múltiplos clientes
- **WHEN** GUI ou service é proprietário e dois clientes CLI/MCP consultam o perfil, ou nenhum proprietário existe
- **THEN** há no máximo um registro SIP/worker SQLite; clientes recebem snapshots coerentes ou engine_unavailable sem spawn oculto; GUI iniciada contra service informa engine_owned_by_service, URI externa informa uri_confirmation_unavailable sem discagem e service sem grant informa grant_required

### Requirement: CLI estruturado e comandos reais
O cliente CLI SHALL oferecer status, devices list, contacts search, history list, config show sanitizado e controles call dial/answer/reject/hangup/dtmf/mute/hold/transfer e dnd. SHALL suportar saída humana e JSON v1, filtros/paginação limitada, IDs de sessão e códigos de saída documentados. Config validate/import/export e doctor planejados na mudança desktop SHALL permanecer compatíveis e sem segredo; alterações de configuração pelo MCP ficam fora deste release.

#### Scenario: AG-02 contrato CLI
- **WHEN** um operador consulta estado e depois executa controle autorizado em uma sessão válida ou inexistente
- **THEN** recebe JSON v1 ou saída humana coerente, resultado assíncrono rastreável e código de saída distinto para sucesso, validação, autorização, indisponibilidade e timeout

### Requirement: MCP stdio interoperável
O servidor SHALL negociar versão MCP compatível, anunciar tools com schemas estritos e oferecer capacidades equivalentes à CLI, com leituras e controles separados. stdout MUST conter apenas mensagens MCP; logs sanitizados SHALL ir para stderr. HTTP/SSE, shell arbitrário e abertura arbitrária de arquivos MUST NOT ser expostos.

#### Scenario: AG-03 protocolo e limites
- **WHEN** um cliente MCP inicializa, lista tools, chama uma leitura válida e envia argumento desconhecido, mensagem excessiva ou requisição inválida
- **THEN** recebe respostas de protocolo corretas e erros limitados, sem logs em stdout, panic, acesso a arquivos ou execução shell

### Requirement: Autorização local e proteção de dados
O proprietário SHALL negar mutações por default. Leituras default SHALL limitar-se a status resumido e diagnóstico não pessoal; contatos, histórico, dispositivos identificáveis e config exigem concessão de leitura local. Cada mutação SHALL exigir aprovação humana no proprietário ou concessão emitida pela GUI antes da sessão headless, fora do canal CLI/MCP do agente, com escopo, destinos/sessões permitidos, prazo, limites e revogação. Tool annotations e campos confirm/yes do agente MUST NOT ser autorização. Service sem UI SHALL exigir concessão humana prévia single-use no bootstrap. TTY/pty MUST NOT criar aprovação; shell irrestrito/GUI automation no mesmo UID SHALL ser declarados fora do boundary, não impedidos por promessa falsa. Credenciais SIP, áudio e referências de segredo MUST NOT ser retornados.

#### Scenario: AG-04 negar e conceder
- **WHEN** agente solicita discagem ou histórico sem permissão, tenta autoaprovar via confirm/yes ou cli approve em pty e depois usa concessão válida, expirada ou revogada
- **THEN** a autoaprovação é rejeitada (não existe cli approve); aprovação GUI usa intenção pendente e operation_get, ou grant prévio ativo, e somente operações dentro dessa autorização executam, sem chamada, captura, exposição de dados ou alteração na negação

### Requirement: Idempotência, concorrência e auditoria
Com armazenamento/ledger indisponível, mutações MUST falhar antes de qualquer efeito. Mutações SHALL exigir chave de idempotência e sessão/versão alvo quando aplicável; retransmissão da mesma intenção MUST NOT duplicar chamada, DTMF ou transferência. Namespace SHALL ser global por perfil/epoch/chave, não por grant/cliente. Reuso da chave com payload ou escopo diferente SHALL falhar. Mutações de epoch antiga SHALL retornar stale_epoch sem efeito; operation_get SHALL retornar resultado persistido ou outcome_unknown por 24 h somente com leitura vigente após restart, depois result_expired. Operações SHALL ser limitadas, com timeout e identificador de resultado; timeout do cliente MUST NOT provocar retry automático. Auditoria local SHALL registrar decisão/resultado sanitizados sem dígitos DTMF, segredos ou número completo por default.

#### Scenario: AG-05 repetição e corrida
- **WHEN** CLI e MCP repetem dial/dtmf/transfer com a mesma chave, reusam chave com outro payload ou disputam sessão já encerrada ou consultam resultado após crash/restart
- **THEN** há no máximo um efeito por intenção aceita, conflito explícito ou resultado terminal estável, sem segundo diálogo SIP; resultado antigo continua consultável com novo grant de leitura e nunca reexecuta; DTMF/PIN não pode ser recuperado por hash simples no DB

### Requirement: Telefonia comprovada e encerramento independente
O aceite SHALL testar controles CLI e MCP no artefato Linux exato contra Asterisk e verificar voz bidirecional com dispositivos reais no modo proprietário GUI e service. Encerrar cliente CLI/MCP SHALL liberar sua conexão sem derrubar o proprietário ou chamada já aceita; sair do proprietário SHALL seguir o shutdown seguro desktop.

#### Scenario: AG-06 operação real e desconexão
- **WHEN** cliente autorizado estabelece e controla chamada no laboratório e encerra sua conexão
- **THEN** o PBX confirma os efeitos, a chamada continua no proprietário até encerramento explícito e o recibo distingue protocolo, mídia sintética e áudio humano real
