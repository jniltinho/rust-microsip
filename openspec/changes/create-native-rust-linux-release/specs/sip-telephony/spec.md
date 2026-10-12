## Purpose

Permitir telefonia SIP de voz interoperável e previsível no primeiro release Linux, com controles reais e recuperação limitada de falhas de rede.

## ADDED Requirements

### Requirement: Registro e transportes
O aplicativo SHALL registrar uma conta habilitada por vez via UDP, TCP ou TLS, autenticar desafios 401/407, renovar registro e exibir seu estado real. Uma segunda conta habilitada MUST ser rejeitada com erro claro neste release.

#### Scenario: SIP-01 registro e renovação
- **WHEN** uma conta válida é configurada no PBX de laboratório para cada transporte
- **THEN** o registro e a renovação são confirmados pelo PBX e pela UI sem duplicação ou loop de autenticação

#### Scenario: SIP-02 configuração e autenticação inválidas
- **WHEN** a senha está incorreta ou existem duas contas habilitadas
- **THEN** o erro é informado sem vazamento de segredo, sem registro parcial silencioso e sem retry contínuo de credenciais rejeitadas

### Requirement: Ciclo de chamada
O aplicativo SHALL originar, receber, aceitar, recusar, cancelar e encerrar chamadas SIP com uma chamada simultânea. SHALL distinguir chamada atendida, ocupada, recusada, cancelada, perdida e falha; uma segunda chamada de entrada SHALL receber ocupado sem derrubar a existente.

#### Scenario: SIP-03 chamadas nos dois sentidos
- **WHEN** o aplicativo chama um segundo ramal e depois recebe uma chamada dele
- **THEN** ambos completam estabelecimento e encerramento, e cada sessão possui exatamente um resultado no histórico

#### Scenario: SIP-04 cancelamento e corrida de atendimento
- **WHEN** o usuário cancela antes do atendimento, inclusive quando o 200 OK cruza o cancelamento
- **THEN** a sessão termina corretamente sem chamada fantasma, toque persistente ou recursos retidos

#### Scenario: SIP-05 recusa e segunda chamada
- **WHEN** o usuário recusa uma entrada ou recebe outra enquanto já está em chamada
- **THEN** o PBX recebe resposta adequada e a chamada existente permanece funcional

### Requirement: Controles de chamada
O aplicativo SHALL enviar DTMF por RTP telephone-event negociado, colocar/retirar espera por renegociação SDP e oferecer DND que recusa novas entradas sem afetar chamada em curso.

#### Scenario: SIP-06 DTMF e espera
- **WHEN** o usuário envia dígitos para uma URA e alterna espera/retorno
- **THEN** a URA reconhece a sequência e o áudio retorna bidirecional sem encerrar a chamada

#### Scenario: SIP-07 não perturbe
- **WHEN** DND é habilitado antes de uma nova entrada
- **THEN** a entrada é recusada com indicação coerente, sem tocar e sem interromper uma sessão já estabelecida

### Requirement: Transferência cega confirmada
O aplicativo SHALL transferir a chamada por REFER e acompanhar o resultado final por NOTIFY, sem informar sucesso apenas por receber 202. Em falha, SHALL manter ou recuperar a chamada original quando o diálogo ainda existir.

#### Scenario: SIP-08 transferência e falha
- **WHEN** o usuário transfere primeiro para um destino válido e depois para um destino inválido no laboratório
- **THEN** o sucesso só aparece após confirmação final, e a falha deixa a sessão original operável quando preservada pelo PBX

### Requirement: Segurança e recuperação de sinalização
O aplicativo SHALL validar certificado e hostname no TLS, aceitar CA customizada explícita e nunca fazer downgrade silencioso. Após perda de rede ou suspensão SHALL marcar indisponibilidade e tentar registrar com backoff limitado; não SHALL recriar automaticamente uma chamada perdida.

#### Scenario: SIP-09 certificado inválido
- **WHEN** o servidor apresenta certificado expirado, hostname diferente ou CA não confiável
- **THEN** a conexão segura falha de modo visível sem fallback para TCP ou UDP

#### Scenario: SIP-10 rede e suspensão
- **WHEN** a rede cai e retorna ou a máquina retoma de suspensão
- **THEN** o registro é recuperado sem tempestade de requests, e sessões encerradas não são rediscadas
