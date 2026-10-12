## Purpose

Garantir áudio de voz realmente bidirecional, dispositivos selecionáveis e negociação de mídia segura no artefato Linux que será distribuído.

## ADDED Requirements

### Requirement: RTP e codecs essenciais
O aplicativo SHALL negociar G.711 PCMA/PCMU, enviar e receber RTP, lidar com sequência, timestamp, jitter e pacotes perdidos ou fora de ordem com memória limitada. Codec incompatível MUST produzir erro compreensível em vez de sucesso sem áudio.

#### Scenario: AUD-01 voz bidirecional
- **WHEN** o artefato de release estabelece uma chamada de cinco minutos com cada codec no PBX
- **THEN** os dois participantes ouvem voz nos dois sentidos, sem crash, e codec/dispositivos ficam registrados no recibo sanitizado

#### Scenario: AUD-02 rede degradada e codec incompatível
- **WHEN** o laboratório injeta perda de 1%, jitter de até 30 ms e reordenação, ou oferece somente codec incompatível
- **THEN** a sessão degradada permanece controlável com buffers limitados, e a sessão incompatível falha explicitamente sem indicador falso de áudio ativo

### Requirement: Dispositivos e hotplug
O aplicativo SHALL listar e selecionar entrada, saída e saída de toque, mostrar ausência/permissão negada e permitir recuperação após desconexão de headset. Troca de microfone durante chamada MUST exigir seleção explícita, não ativar outro microfone silenciosamente.

#### Scenario: AUD-03 seleção e headset removido
- **WHEN** o usuário escolhe o headset e depois o desconecta durante uma chamada
- **THEN** a UI informa perda do dispositivo, permite selecionar outro e só volta a capturar após confirmação, sem travar ou alterar configurações globais do sistema

### Requirement: Mute e teste local
O aplicativo SHALL permitir teste de captura/reprodução sem chamada e mute de microfone durante chamada. Enquanto mute estiver ativo, SHALL impedir transmissão de voz capturada sem desligar a recepção.

#### Scenario: AUD-04 teste e mute
- **WHEN** o usuário testa áudio e depois ativa mute numa chamada de laboratório
- **THEN** o teste local confirma o dispositivo, o outro ramal deixa de receber voz do microfone e o usuário continua ouvindo a chamada

### Requirement: Mídia segura sem downgrade
O aplicativo SHALL oferecer perfil TLS + SRTP com SDES e AES_CM_128_HMAC_SHA1_80; chaves SDES MUST trafegar somente sobre TLS validado. Perfil seguro MUST recusar peer sem SRTP. Modo RTP não criptografado SHALL exigir escolha explícita e aviso persistente de segurança.

#### Scenario: AUD-05 SRTP obrigatório
- **WHEN** o perfil seguro negocia com peer compatível e depois com peer que oferece apenas RTP
- **THEN** a primeira chamada usa SRTP verificado no laboratório, e a segunda é rejeitada sem downgrade ou registro de chaves

### Requirement: NAT com limites explícitos
O aplicativo SHALL suportar rport, keepalive, RTP simétrico e STUN configurável para redes compatíveis. SHALL documentar LAN/VPN/PBX com media relay como topologias do release, sem garantir NAT simétrico arbitrário ou TURN.

#### Scenario: AUD-06 topologia suportada
- **WHEN** o aplicativo chama através de NAT simples com PBX ancorando mídia e STUN configurado
- **THEN** o áudio passa nos dois sentidos e falhas de conectividade são distinguíveis de dispositivo ausente, sem alegar suporte universal
