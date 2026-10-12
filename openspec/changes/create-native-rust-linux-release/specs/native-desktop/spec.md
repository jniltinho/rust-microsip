## Purpose

Oferecer uma interface desktop Linux nativa, familiar para usuários do GOnnect e fiel às telas essenciais sem disfarçar funcionalidades ainda indisponíveis.

## ADDED Requirements

### Requirement: Interface nativa e layout de referência
O aplicativo SHALL executar sem webview, Electron, Tauri ou Qt e reproduzir a navegação lateral, busca superior, histórico central e favoritos à direita, além das telas de contatos, discador, chamada e ajustes. Agenda e demais funções adiadas MUST indicar indisponibilidade sem simular dados.

#### Scenario: ND-01 layout comparável
- **WHEN** a janela abre com dados fictícios na mesma resolução, escala, idioma e estado da referência GOnnect 2.4.1
- **THEN** os elementos essenciais têm a mesma ordem, agrupamento e função, com diferenças registradas na matriz visual e sem controles sobrepostos ou cortados

#### Scenario: ND-02 função fora do release
- **WHEN** o usuário encontra uma área reservada a agenda, vídeo ou conferência
- **THEN** vê indicação de recurso indisponível neste release, sem ação enganosa nem conteúdo fabricado

### Requirement: Temas persistentes
O aplicativo SHALL oferecer claro, escuro e sistema, aplicar a escolha sem reiniciar e preservar a preferência ao reabrir.

#### Scenario: ND-03 alternância de tema
- **WHEN** o usuário alterna entre claro e escuro durante uma chamada
- **THEN** todas as telas e diálogos mudam de tema, a chamada continua e a escolha reaparece no próximo início

#### Scenario: ND-04 tema do sistema
- **WHEN** a opção sistema está ativa e o desktop comunica alteração de preferência de cor
- **THEN** a UI acompanha a mudança, usando claro como fallback se o serviço não existir

### Requirement: Operação por teclado e escala
O aplicativo SHALL manter foco visível, navegação por teclado, rótulos nos controles e contraste mínimo de texto normal de 4,5:1 nos dois temas. SHALL manter as ações essenciais acessíveis em 1024x768 e em escalas 100%, 150% e 200%, usando rolagem quando necessário.

#### Scenario: ND-05 teclado e HiDPI
- **WHEN** um operador usa apenas teclado nas telas essenciais em cada escala suportada
- **THEN** consegue buscar, discar, aceitar, recusar e encerrar, sem depender só de cor ou ícones sem rótulo

### Requirement: Estados e falhas visíveis
O aplicativo SHALL mostrar registro, toque, chamada ativa, espera, erro e reconexão sem congelar a UI ou afirmar sucesso antes da confirmação do backend.

#### Scenario: ND-06 falha de registro
- **WHEN** o servidor rejeita as credenciais
- **THEN** a UI informa falha e permite corrigir a configuração sem exibir senha ou fechar a aplicação
