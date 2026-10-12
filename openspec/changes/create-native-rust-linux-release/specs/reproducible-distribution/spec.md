## Purpose

Permitir compilar, testar e distribuir o primeiro release Linux sem toolchain no host, com artefatos verificáveis e sem publicar referências locais ou dados pessoais.

## ADDED Requirements

### Requirement: Executável com recursos incorporados
O release SHALL entregar um ELF x86_64 com UI, fontes licenciadas, ícones, tons, codecs essenciais, migrações e SQLite incorporados, sem downloads no primeiro uso ou subprocessos do aplicativo. SHALL executar em Ubuntu 24.04 GNOME/Wayland e KDE/X11 limpos sem instalar Rust, Node, Qt, banco externo ou bibliotecas específicas do aplicativo. Dependências padrão do Linux SHALL ser enumeradas em manifesto auditado.

#### Scenario: DIST-01 máquina limpa
- **WHEN** somente o ELF é copiado para uma VM limpa da matriz suportada com seu desktop e áudio padrão, conectada ao PBX auxiliar e segundo endpoint pela rede isolada de laboratório
- **THEN** o usuário configura uma conta e completa chamada bidirecional, sem baixar recursos da aplicação ou instalar pacotes extras

### Requirement: AppImage adicional não mascara dependências
O release SHALL oferecer AppImage adicional com o mesmo núcleo, checksums e caminho de execução sem FUSE documentado. O AppImage MUST NOT ser apresentado como prova de que o ELF atende ao requisito de binário único.

#### Scenario: DIST-02 ELF e AppImage separados
- **WHEN** os dois artefatos são testados em ambiente suportado, inclusive sem FUSE para o AppImage
- **THEN** cada um possui recibo próprio e um sucesso não substitui um teste falho do outro

### Requirement: Build Docker sem toolchain no host
O projeto SHALL oferecer build a partir de clone limpo apenas com Docker Engine e BuildKit, sem exigir Cargo no host ou a pasta `base/`. Toolchain, lockfile, imagens e dependências SHALL ser fixados; artefatos SHALL ser exportados para diretório local sem privilégios de root desnecessários.

#### Scenario: DIST-03 build de clone limpo
- **WHEN** um operador executa o comando documentado numa máquina com Docker e sem Rust instalado
- **THEN** recebe ELF, AppImage, SHA256SUMS, licenças, SBOM e manifesto de dependências em `dist/`, sem copiar `.env`, bancos ou referências locais ao contexto

### Requirement: Laboratório Asterisk descartável
O projeto SHALL oferecer laboratório Docker Asterisk com dois ramais fictícios, echo/URA, TLS/SRTP, testes SIP e teardown. O default SHALL isolar a rede sem publicar portas no host; o perfil manual no host SHALL bindar somente loopback, sem credenciais reais ou trunks externos. Um perfil adicional de aceite em VMs SHALL ser opt-in, usar exclusivamente rede host-only isolada, bind no IP explícito da VM auxiliar e ACL apenas para clientes de teste, nunca interface pública ou wildcard. Credenciais de teste SHALL ser geradas localmente e ignoradas pelo Git.

#### Scenario: DIST-04 laboratório e isolamento
- **WHEN** o operador sobe o laboratório default, executa testes e o remove
- **THEN** o PBX atende os cenários automatizados, nenhuma porta fica publicada, não existem rotas para PSTN e o teardown remove os recursos descartáveis

#### Scenario: DIST-05 perfil desktop manual
- **WHEN** o operador ativa explicitamente o perfil local para chamar com o aplicativo no host, ou o perfil isolado de aceite em VMs
- **THEN** apenas portas SIP e RTP documentadas ficam disponíveis em 127.0.0.1 no perfil host ou no IP/ACL da VM auxiliar no perfil VM, com mídia ancorada e sem exposição em 0.0.0.0 ou rede de produção

### Requirement: Gates reais e revisão independente
O release MUST exigir testes unitários, de integração e chamadas com áudio no artefato exato, matriz visual nos dois temas e revisão pelo CLI do Claude. Revisão de texto, mocks ou smoke de janela MUST NOT ser usados como prova de telefonia funcional.

#### Scenario: DIST-06 bloqueio de release incompleto
- **WHEN** há apenas uma janela visível, testes mockados ou uma aprovação de arquitetura
- **THEN** o release continua bloqueado até existir evidência de chamadas e áudio bidirecional, sem anunciar uma versão usável prematuramente

### Requirement: Publicação rastreável e privada por padrão
O projeto SHALL publicar no GitHub somente arquivos próprios auditados e referências documentais, nunca `base/`, credenciais, bancos, dumps, gravações ou logs pessoais. Um release SHALL identificar commit, licença, checksums, SBOM e recibos sanitizados. A visibilidade inicial SHALL ser privada salvo instrução explícita do proprietário.

#### Scenario: DIST-07 auditoria antes do push
- **WHEN** o conteúdo staged e os artefatos de release são auditados
- **THEN** nenhum caminho `base/`, segredo ou dado pessoal é encontrado, e a origem de cada artefato distribuído pode ser identificada
