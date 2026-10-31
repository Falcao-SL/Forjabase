# Status — Forja

> Atualizado em **2026-10-31**. Este arquivo é a fonte única de "em que pé está".

| Campo | Valor |
|---|---|
| **Estágio** | **Em desenvolvimento** — Marcos 0.1, 0.2, 0.3, 0.4, 0.5 e 0.6 concluídos! OAuth2 com PKCE e normalização de perfis, OTP por e-mail com uso único e auto-verificação, MFA em dois fatores com desafio 401 e `mfaId`, endpoint `auth-methods`, SMTP/e-mails, agendador cron in-process, logs estruturados com agregações horárias, backups em zip com restauração e upload, encerramento gracioso, e telas completas no painel administrativo |
| **Plataformas** | Servidor: macOS, Linux, Windows (Rust); painel: navegador, HTML no servidor + htmx (sem build de front-end) |
| **Stack** | Rust (axum; SQLite), maud + htmx, wasmtime in-process na 0.8. Decisão: [ADR-001](arquitetura/ADR-001-um-binario.md) |
| **Site** | Landing em `site/` (inglês na raiz, português em `index.pt.html`). O host previsto é https://forja.falcaosl.com, no Coolify, com `deploy/site/Dockerfile`. O DNS ainda não existe; o endereço público segue o repositório. |
| **Monetização** | Nenhuma: código aberto, MIT |
| **Próximo marco** | Marco 0.7 (Segredo Apple OAuth2, Alertas de login e Rate limiting) |
| **Data-alvo** | A definir; 1.0.0 só quando estável (PLANO §6) |

## O que já está pronto

- Workspace com 4 crates e 82 testes passando.
- Autenticação avançada (Marco 0.6 completo):
  - **OAuth2**: suporte aos provedores do PocketBase (Google, GitHub, GitLab, Discord, Microsoft, generic OIDC, etc.), fluxo de autorização com geração automática de PKCE (S256) e State, troca de código e consulta de perfil com normalização de dados, vínculo com tabela `_externalAuths`, endpoints `POST /api/collections/{colecao}/auth-with-oauth2`, `GET /api/collections/{colecao}/records/{id}/external-auths`, `DELETE /api/collections/{colecao}/records/{id}/external-auths/{provider}` e página de redirecionamento `GET /api/oauth2-redirect` via postMessage compatível com popups dos SDKs.
  - **OTP (One-Time Password)**: geração criptográfica de códigos numéricos com expiração configurável, entrega por e-mail com templates personalizáveis, proteção contra enumeração de e-mails, validação com expiração e uso único, endpoints `POST /api/collections/{colecao}/request-otp` e `POST /api/collections/{colecao}/auth-with-otp`, validação automática de e-mail (`verified = true`).
  - **MFA (Multi-Factor Authentication)**: autenticação em dois fatores sequenciais distintos (senha + OTP, OAuth2 + OTP, etc.), emissão de desafio com código de status HTTP `401 Unauthorized` contendo `{"mfaId": "..."}`, expiração de sessão e prevenção de repetição do mesmo fator.
  - **Métodos de Autenticação (`auth-methods`)**: endpoint público `GET /api/collections/{colecao}/auth-methods` expondo status e parâmetros de MFA, OTP, Senha e Provedores OAuth2 com PKCE gerado.
  - **Painel Administrativo**: aba e campos de configuração de OTP, MFA e OAuth2 no editor de coleções auth.
- Operação e Configurações (Marco 0.5 completo):
  - Suporte completo a SMTP (`SmtpCorreio` via `lettre`) com TLS, STARTTLS, PLAIN e LOGIN, e endpoint de teste `POST /api/settings/test/email`.
  - Modelos de e-mail editáveis (`verificationTemplate`, `resetPasswordTemplate`, `confirmEmailChangeTemplate`) com placeholders (`{APP_NAME}`, `{APP_URL}`, `{TOKEN}`, `{ACTION_URL}`, `{RECORD:campo}`).
  - Logs estruturados de requisições: tabela `_logs`, middleware assíncrono não-bloqueante para rotas `/api/...`, endpoints `GET /api/logs`, `GET /api/logs/{id}`, `GET /api/logs/stats` com agregação horária e limpeza automática por retenção em dias.
  - Backups compactados em formato `.zip`: endpoints `GET /api/backups`, `POST /api/backups` (com checkpoint de WAL), `GET /api/backups/{key}` (download), `POST /api/backups/{key}/restore`, `DELETE /api/backups/{key}` e `POST /api/backups/upload`.
  - Tarefas agendadas (Cron): agendador in-process, endpoints `GET /api/crons` e disparo manual via `POST /api/crons/{id}`, com tarefas do sistema pré-configuradas (`__pbLogsCleanup__` e `__pbBackupRun__`).
  - Encerramento gracioso (*graceful shutdown*) no servidor HTTP com captura de `SIGINT` (Ctrl+C) e `SIGTERM`.
  - Telas administrativas no painel para Configurações (`/_/settings`), Logs com gráfico horário (`/_/logs`), Tarefas Agendadas (`/_/crons`) e Backups (`/_/backups`).
- Armazenamento em S3 compatível (`ProvedorStorage`, `DiscoLocal` e `S3Storage` com autenticação AWS SigV4 pura em Rust) com suporte a AWS S3, MinIO, Cloudflare R2, Wasabi, etc.
- Endpoints de configurações (`GET` e `PATCH /api/settings`) e teste de conectividade S3 (`POST /api/settings/test/s3`), compatíveis com o formato do PocketBase.
- Painel administrativo: gerenciador de coleções (`/_/new-collection`, `/_/collections/{nome}/settings`, `delete-collection`) com adição dinâmica de campos via htmx (`/_/htmx/campo-linha`), formulário de regras com suporte a superusuário, público e expressões personalizadas, e exclusão com diálogo de confirmação nativo.
- Macros de data e hora nos filtros (`@now`, `@yesterday`, `@tomorrow`, `@todayStart`, `@todayEnd`, `@monthStart`, `@monthEnd`, `@yearStart`, `@yearEnd`, `@second`, `@minute`, `@hour`, `@day`, `@month`, `@year`, `@weekday`) resolvidos em UTC para comparação cronológica como no PocketBase.
- Modificadores de filtro nos campos: `:lower` (`LOWER("campo")`), `:length` (`LENGTH("campo")`) e `:isset` (`campo IS NOT NULL AND campo != ''`).
- Seletor de campos (`fields`, inclusive com modificador `:excerpt(n)`) e suporte em tempo real (SSE) a `expand` e `fields` por assinatura.
- Operadores "algum item" (`?=`, `?!=`, `?>`, `?>=`, `?<`, `?<=`, `?~`, `?!~`)
  nos filtros, suportando arrays JSON, relações múltiplas, select múltiplo e
  valores escalares via `json_valid` e `json_each`.
- `PATCH /api/collections/{colecao}`: atualização de esquema em tempo de execução
  (renomeia tabelas SQLite, views, adiciona colunas `ADD COLUMN`, valida e atualiza regras).
- `POST /api/collections/meta/dry-run-view`: testa a consulta SQL de uma view,
  garante a presença da coluna `id` e devolve os campos inferidos sem persistir.
- Miniaturas sob demanda no campo `file`: `?thumb=WxH` (com corte central, topo `t`,
  base `b`, ajuste proporcional `f`, ou proporções `0xH`, `Wx0`), com cache em disco em
  `storage/{colecao}/{registro}/thumbs_{arquivo}/{thumb}_{arquivo}` e exclusão automática.
- Barramento in-process (M1): `criar`, `atualizar` e `apagar` — inclusive senha,
  verificação, superusuário e cada registro da cascata — publicam
  `Evento { colecao, acao, id, registro }` em `tokio::broadcast` depois do commit.
  Assinante ausente ou lento não desfaz a escrita. No lote, o evento só sai no commit.
- Campo `file` no disco local (`storage/{colecaoId}/{registroId}/{arquivo}`):
  multipart no criar e no atualizar, `maxSelect`, `maxSize` (0 = 5 MiB),
  `mimeTypes`, `protected`. `GET /api/files/...` com `?download=1` e `Range`.
  Arquivo protegido pede `POST /api/files/token` (3 minutos) e a `viewRule`.
  O painel só mostra o nome; o envio é pela API.
- `POST /api/batch`: até 50 operações de registro em JSON, na mesma transação
  SQLite. Se uma falha, as outras desfazem e a resposta é o erro dessa operação.
- SSE no protocolo do PocketBase (`GET` + `POST /api/realtime`): `PB_CONNECT`,
  tópicos `coleção`, `coleção/*` e `coleção/{id}`, filtro opcional na assinatura.
  A regra é reavaliada por cliente. Sem `expand` nem `fields`. Ocioso 5 min,
  no máximo 30 min.
- Campo `relation` (um ou vários): o id tem de existir; visitante só liga o que
  a `viewRule` do alvo deixa ver. `cascadeDelete` apaga o registro que ficou
  sem referências; relação obrigatória sem cascata impede o apagamento.
  `?expand=` direto, aninhado (`autor.cidade`) e reverso (`comentarios_via_post`,
  sempre lista). O painel edita o campo como texto de ids.
- Coleção `view`: um `SELECT` (ou `WITH`) com coluna `id`. Os outros nomes
  viram campos. Listar e ver respeitam `listRule` e `viewRule`. Criar, atualizar
  e apagar registro respondem que a coleção não é gravável — no API e no painel.
  Apagar a coleção de origem é recusado enquanto alguma view a citar.
- Macro `@collection.colecao.campo` nas regras e, para o superusuário, no
  `filter`. Vira `EXISTS`: no mesmo `&&`, uma linha da outra coleção tem de
  cumprir todos os predicados. No `||`, cada lado é independente. Visitante que
  manda `@collection` ou `@request` no filtro recebe 403.
- SQLite é o único motor suportado (um arquivo por projeto, WAL). Turso e
  PostgreSQL continuam compilando atrás de feature flag, sem suporte e fora do CI.
- Superusuário (Argon2id, JWT), coleções base e auth, registros com filtro do
  PocketBase convertido em SQL com parâmetros, ordenação, paginação, regras e
  campos ocultos.
- Autenticação de usuários: login, refresh, verificação de e-mail, redefinição
  de senha, troca de e-mail, personificação e macros `@request.auth.*`.
- Painel em `/_/`: login por cookie `httpOnly`, lista de coleções, tabela com
  filtro e paginação via htmx, criar/editar/apagar registros com erros por campo.
- Cliente Python do PocketBase funciona contra o Forja (conferido em 2026-10-21).
- Documentação: [FUNCIONALIDADES](FUNCIONALIDADES.md), [PLANO](PLANO.md), [GUIA](GUIA.md),
  [ARQUITETURA](ARQUITETURA.md), [PARIDADE](PARIDADE.md),
  [ADR-001](arquitetura/ADR-001-um-binario.md) (um binário: o que entra no núcleo).

## O que falta

Tudo marcado 📋 em [FUNCIONALIDADES.md](FUNCIONALIDADES.md), na ordem do PLANO §5.
