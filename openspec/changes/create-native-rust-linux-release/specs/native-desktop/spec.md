## Purpose

Oferecer uma interface desktop Linux nativa, compacta e familiar para usuários do MicroSIP, com GOnnect como referência complementar sem disfarçar funcionalidades ainda indisponíveis.

## ADDED Requirements

### Requirement: Interface nativa e layout de referência
O aplicativo SHALL executar sem webview, Electron, Tauri ou Qt e oferecer janela compacta com abas Discador, Chamadas e Contatos, campo de número, teclado 3×4, ligar/encerrar, conta/status, menu de ajustes e DND. Mute, espera e transferência cega SHALL ser acessíveis em chamada; busca e favoritos SHALL estar nas listas locais. Não SHALL exigir dashboard/agenda do GOnnect ou runtime Wine. Recursos fora de escopo MUST ser omitidos ou indicar indisponibilidade sem simular dados.

#### Scenario: ND-01 layout comparável
- **WHEN** a janela abre com dados fictícios na mesma resolução, escala, idioma e estado da referência MicroSIP Lite 3.22.16 em modo de chamada única e do mock Rust aprovado
- **THEN** os elementos essenciais têm a ordem, agrupamento e função equivalentes no fluxo compacto, sem exigir identidade pixel a pixel de Win32, com diferenças registradas na matriz visual e sem controles sobrepostos ou cortados

#### Scenario: ND-02 função fora do release
- **WHEN** a matriz de UI é comparada com controles upstream de chat, vídeo, conferência ou gravação
- **THEN** controles fora do escopo estão ausentes ou claramente indisponíveis, sem ação enganosa nem conteúdo fabricado

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
