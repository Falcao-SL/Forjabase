# Changelog

## Não lançado

- **Marco 0.6 — OAuth2, OTP e Autenticação Multifator (MFA)**:
  - **OAuth2**: Suporte a autenticação com provedores externos (Google, GitHub, GitLab, Discord, Microsoft, genéricos OIDC, etc.) com geração automática de PKCE (S256) e State, troca de código de autorização e busca de perfil normalizado. Endpoints `POST /api/collections/{colecao}/auth-with-oauth2`, `GET /api/collections/{colecao}/records/{id}/external-auths`, `DELETE /api/collections/{colecao}/records/{id}/external-auths/{provider}` e página de retorno de popup compatível com SDKs do PocketBase em `GET /api/oauth2-redirect` (via `postMessage`).
  - **OTP (One-Time Password)**: Emissão e envio de senhas numéricas descartáveis por e-mail com tempo de expiração configurável, proteção contra enumeração de usuários, uso único e auto-verificação de e-mail (`verified = true`). Endpoints `POST /api/collections/{colecao}/request-otp` e `POST /api/collections/{colecao}/auth-with-otp`.
  - **MFA (Autenticação Multifator)**: Fluxo de autenticação em dois passos distintos (senha + OTP, OAuth2 + OTP, etc.) com emissão de desafio HTTP 401 Unauthorized contendo `{"mfaId": "..."}`, expiração temporal de sessão e validação de fatores distintos.
  - **Métodos de Autenticação (`auth-methods`)**: Endpoint público `GET /api/collections/{colecao}/auth-methods` expondo as opções de autenticação ativas (MFA, OTP, Senha e Provedores OAuth2 com URLs de autorização e desafios PKCE pré-gerados).
  - **Painel Administrativo**: Formulários e controles no painel para ativar e configurar durações e parâmetros de OTP, MFA e OAuth2 na edição de coleções auth.
- **Marco 0.5 — Operação e Configurações**:
  - **SMTP e Entrega de E-mails**: Implementado cliente SMTP em Rust assíncrono via `lettre` com suporte a TLS, STARTTLS, autenticação PLAIN e LOGIN, e endpoint `POST /api/settings/test/email`.
  - **Modelos de E-mail Editáveis**: Suporte aos modelos `verificationTemplate`, `resetPasswordTemplate` e `confirmEmailChangeTemplate` em `Configuracoes.meta`, com substituição dinâmica de placeholders (`{APP_NAME}`, `{APP_URL}`, `{TOKEN}`, `{ACTION_URL}`, `{RECORD:campo}`).
  - **Logs Estruturados de Requisições**: Tabela de sistema `_logs`, middleware Axum não-bloqueante gravando método, URL, código de status HTTP e tempo de execução (`execTime`), endpoints `GET /api/logs`, `GET /api/logs/{id}`, e agregações estatísticas horárias em `GET /api/logs/stats`, além de limpeza automática por retenção em dias.
  - **Sistema de Backups**: Geração de snapshots completos em arquivo `.zip` com checkpoint seguro do SQLite WAL e arquivos locais, endpoints `GET /api/backups`, `POST /api/backups`, download com `Content-Disposition`, restauração atômica, exclusão e upload multipart em `POST /api/backups/upload`.
  - **Agendador Cron In-Process**: Endpoints `GET /api/crons` e `POST /api/crons/{id}` com execução manual ou agendada para limpeza de logs (`__pbLogsCleanup__`) e backups recorrentes (`__pbBackupRun__`).
  - **Encerramento Gracioso (*Graceful Shutdown*)**: Tratamento de sinais `SIGINT` (Ctrl+C) e `SIGTERM` no servidor HTTP permitindo finalizar requisições ativas antes do encerramento.
  - **Telas do Painel Administrativo**: Adicionadas páginas completas no servidor (Maud + HTMX) para Visualização de Logs com gráfico de barras de volume por hora (`/_/logs`), Tarefas Agendadas com botão de execução manual (`/_/crons`), Gerenciamento de Backups com download e restauração (`/_/backups`), e Central de Configurações (`/_/settings`) com abas para Geral, Remetente, SMTP, S3, Backups e Logs.
- Editor e gerenciamento de coleções no painel administrativo (`/_/new-collection`,
  `/_/collections/{nome}/settings` e `delete-collection`): criação e edição de
  coleções (base, auth, view), configuração de view queries, regras de acesso
  (superusuário, público ou expressões) e adição interativa de campos customizados
  via HTMX sem recarregar a página e sem build de front-end.
- Macros de data e hora nos filtros: `@now`, `@yesterday`, `@tomorrow`, `@todayStart`,
  `@todayEnd`, `@monthStart`, `@monthEnd`, `@yearStart`, `@yearEnd`, `@second`,
  `@minute`, `@hour`, `@day`, `@month`, `@year`, `@weekday`. Todos convertidos
  para strings de timestamp UTC no formato do PocketBase e amarrados por parâmetros SQLite.
- Modificadores de filtro nos campos: `:lower` (`LOWER("campo")`), `:length` (`LENGTH("campo")`)
  e `:isset` (`campo IS NOT NULL AND campo != ''` ou `IS NULL OR = ''`).
- Seletor de campos (`fields`, com suporte a `:excerpt(n)`) e suporte em tempo real
  (SSE) a `expand` e `fields` por assinatura no evento de broadcast.
- Miniaturas sob demanda no campo `file`: `?thumb=WxH` com suporte a corte
  central, topo (`t`), base (`b`), ajuste proporcional (`f`) e dimensões
  proporcionais `0xH` e `Wx0`. Miniaturas geradas e armazenadas em cache local em
  `storage/{colecao}/{registro}/thumbs_{arquivo}/{thumb}_{arquivo}`, com
  remoção automática na exclusão do arquivo.
- Operadores "algum item" nos filtros: `?=`, `?!=`, `?>`, `?>=`, `?<`, `?<=`, `?~`, `?!~`.
  Em SQLite, compilados dinamicamente para `json_valid` e `json_each`, funcionando
  tanto para arrays JSON e multi-select/relations quanto para valores escalares.
- `PATCH /api/collections/{colecao}`: suporte completo para alteração de esquema
  de coleções em tempo de execução, incluindo renomeação de tabelas e views no SQLite,
  adição de novas colunas via `ADD COLUMN` e reconfiguração de regras de acesso.
- `POST /api/collections/meta/dry-run-view` (e `/api/collections/dry-run-view`):
  inspeção prévia e teste de consultas SQL para coleções do tipo `view`, validando
  a coluna primária `id` e inferindo tipos de campos antes da criação.
- Campo `file` em disco local. O multipart do criar e do atualizar grava em
  `storage/{colecaoId}/{registroId}/{arquivo}`, com nome normalizado. Valem
  `maxSelect`, `maxSize` (0 = 5 MiB) e `mimeTypes`. `campo+` acrescenta e
  `campo-` tira nomes que já estão no registro. `GET /api/files/...` serve o
  arquivo, com `?download=1` e um `Range` simples. Protegido exige
  `POST /api/files/token` (tipo `file`, 3 minutos) e a `viewRule`; falha é 404.
  `?thumb=` devolve o original. Miniatura e S3 ficam para fechar a 0.3. O
  painel mostra o nome e não envia o arquivo.
- `POST /api/batch`: até 50 operações JSON de registro (`GET`, `POST`, `PATCH`,
  `DELETE`) no mesmo commit do SQLite. Uma falha desfaz o lote e a resposta é
  esse erro. O evento e a remoção de arquivo saem só depois do commit.
- SSE em `GET` e `POST /api/realtime`, no protocolo do PocketBase (`PB_CONNECT`,
  depois as assinaturas). Tópicos de coleção e de registro, com a `listRule` ou
  a `viewRule` de cada cliente e um filtro opcional. Sem `expand` nem `fields`.
  Conexão ociosa cai em 5 minutos e nenhuma passa de 30.
- Coleções `view`: `viewQuery` é um único `SELECT` (ou `WITH`) com coluna `id`.
  Os outros nomes viram campos, inferidos do tipo SQL. O registro é somente
  leitura (API e painel). Apagar uma coleção que uma view ainda cita é recusado.
- Macro `@collection.colecao.campo` em regras e no filtro do superusuário. Vira
  `EXISTS`: predicados no mesmo `&&` valem para uma linha da outra coleção;
  um `||` separa os grupos. No `filter` da query string, visitante não pode
  usar `@collection` nem `@request` (403).
- Barramento de escrita (ADR-001, M1): depois do commit, `criar`, `atualizar`
  e `apagar` publicam `Evento { colecao, acao, id }` num `tokio::broadcast`.
  Vale também para senha, verificação de e-mail, superusuário e cada registro
  atingido pela cascata. Sem assinante, ou com assinante lento, a escrita fica.
  O evento leva também o `registro` (no apagar, o último estado). A rota SSE
  está na fatia de arquivos e lote, acima.
- Campo `relation` (um e vários), com `collectionId`, `minSelect`, `maxSelect`
  e `cascadeDelete`. O `expand` cobre relação direta, aninhada e reversa
  (`colecao_via_campo`) em listar, ver, criar e atualizar. O painel edita o
  id (ou ids separados por vírgula).
- Coleções **auth** de verdade: toda coleção do tipo `auth` ganha os cinco campos
  de sistema do PocketBase (`email`, `emailVisibility`, `verified`, `password`,
  `tokenKey`), e-mail único sem diferenciar maiúsculas e senha guardada em
  Argon2id — o hash e a `tokenKey` nunca voltam numa resposta nem entram em
  filtro ou ordenação.
- Login, renovação e ciclo de vida da conta em `/api/collections/{colecao}`:
  `auth-with-password`, `auth-refresh`, `request-verification` /
  `confirm-verification`, `request-password-reset` / `confirm-password-reset`,
  `request-email-change` / `confirm-email-change` e `impersonate`.
- Regras de acesso com `@request.auth.*`, resolvidas para parâmetros de SQL (nunca
  interpoladas). Sem usuário autenticado, a macro vale vazio, como no PocketBase.
- Envio de e-mail atrás do trait `correio::Correio`: `Console` (stderr, o padrão) e
  `Memoria` (testes). SMTP fica para a onda de operações.
- Campos de sistema protegidos: usuário comum não marca `verified`, não troca o
  e-mail por `PATCH` (só pelo fluxo com token) e o e-mail só aparece para o dono,
  para superusuários, ou quando `emailVisibility` está ligado.
- Trocar a senha rotaciona a `tokenKey` e invalida os tokens já emitidos.
- Painel administrativo em `/_/` reescrito: HTML gerado no servidor (`maud`)
  com htmx embutido, no lugar da SPA Leptos/WASM. Login de superusuário por
  cookie `httpOnly` + `SameSite=Strict`, lista de coleções, tabela com filtro e
  paginação via htmx, formulário de criação e edição com erros por campo, e
  exclusão com confirmação. Sem etapa de build de front-end; o WASM fica para
  as extensões.

## 0.0.3 — 2026-10-21

- Três motores de banco com a mesma API, escolhidos com `--engine` (ou `FORJA_ENGINE`):
  - **SQLite** (rusqlite), o padrão;
  - **Turso**, que abre o mesmo arquivo do SQLite;
  - **PostgreSQL / Supabase** (`FORJA_PG_URL`), com TLS por rustls e um esquema `forja_<projeto>` por projeto.
- `store::Banco` passa a ser um só, assíncrono, sobre o trait `motor::Executor`; a lógica de coleções, registros, regras e validação não depende mais do motor.
- Bateria de conformidade (`store::conformidade`) que todo motor precisa passar; o CI ganhou um job com Postgres 17.
- Ordenação estável por `"id"` em vez de `rowid`, igual em todos os motores.

## 0.0.2 — 2026-10-21

- SQLite por projeto (`forja_data/default/data.db`, WAL) atrás do trait `Banco`, pronto para um Postgres opcional por projeto.
- Superusuários: `forja superuser upsert`, login (`auth-with-password`) e `auth-refresh`; senha em Argon2id; token JWT invalidado ao trocar a senha.
- Coleções base: listar, criar, ver e apagar; campos `text`, `editor`, `number`, `bool`, `email`, `url`, `date`, `select` e `json` com validação e códigos de erro do PocketBase.
- Registros: listar (paginação, `filter`, `sort`, `skipTotal`), criar, ver, atualizar e apagar, com regras de acesso por operação e campos `hidden`.
- Filtros do PocketBase convertidos em SQL com parâmetros (SQLite e Postgres), recusando campos desconhecidos.
- `--dir` para a pasta de dados.

## 0.0.1 — 2026-10-21

- Workspace Rust com quatro crates: `forja`, `forja-core`, `forja-server`, `forja-admin`.
- `forja serve` (porta padrão 8090) e `GET /api/health` no formato do PocketBase.
- Tipos de coleção (base, auth, view) e os 14 tipos de campo, com os nomes do PocketBase.
- Casca do painel administrativo em Leptos, compilando para `wasm32-unknown-unknown`.
- Documentação: funcionalidades, plano, guia de uso e status.
