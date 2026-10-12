## Purpose

Integrar o softphone ao desktop Linux com comportamento seguro e degradável, sem exigir extensões de shell ou alterar preferências do sistema sem consentimento.

## ADDED Requirements

### Requirement: Notificações e bandeja opcionais
O aplicativo SHALL notificar entradas e chamadas perdidas pelo desktop quando disponível, com ações de atender/recusar quando suportadas. SHALL funcionar sem bandeja ou serviço de notificações, mantendo janela de entrada acessível.

#### Scenario: LIN-01 desktop sem bandeja
- **WHEN** o desktop não oferece StatusNotifierItem ou notificações
- **THEN** chamadas recebidas continuam visíveis e atendíveis na aplicação, sem crash ou janela permanentemente inacessível

### Requirement: URI externa confirmada
O aplicativo SHALL aceitar `tel:` e `sip:` sem executar shell, limitar e validar o destino, rejeitar CR/LF e parâmetros não suportados e solicitar confirmação antes de discar. Instalação de associação e autostart SHALL ser opt-in em nível de usuário.

#### Scenario: LIN-02 link legítimo e malicioso
- **WHEN** o usuário abre um link válido ou um link contendo caracteres de controle, comando shell ou parâmetro inválido
- **THEN** o válido abre confirmação, o malicioso é rejeitado e nenhum comando ou chamada é executado automaticamente

### Requirement: Instância única local
O aplicativo SHALL manter uma instância por usuário/perfil e encaminhar URIs para ela por IPC local protegido, sem API remota ou porta de controle pública.

#### Scenario: LIN-03 segunda instância
- **WHEN** um segundo processo é iniciado para o mesmo perfil com uma URI válida
- **THEN** a janela original apresenta a confirmação e não ocorre segundo registro SIP ou concorrência destrutiva no banco

### Requirement: Fechar e sair com segurança
O aplicativo SHALL distinguir ocultar janela de sair. Sem bandeja, fechar SHALL oferecer saída segura em vez de ocultação irrecuperável. Sair durante chamada SHALL exigir confirmação; shutdown SHALL liberar áudio, persistir resultados e tentar encerrar/desregistrar com prazo máximo de cinco segundos.

#### Scenario: LIN-04 saída durante chamada
- **WHEN** o usuário solicita saída em chamada e confirma, ou o processo recebe SIGTERM
- **THEN** a chamada é finalizada no melhor esforço, o microfone é liberado e o processo termina dentro do prazo mesmo sem resposta do PBX
