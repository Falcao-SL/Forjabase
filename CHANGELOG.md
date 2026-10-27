# Changelog

## Não lançado

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
