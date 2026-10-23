# Prompt mestre — PocketBase em Rust + htmx

Documento de especificação e prompt para reconstruir o
[PocketBase](https://github.com/pocketbase/pocketbase) como um **monorepo em
Rust**, com painel administrativo em **HTML gerado no servidor + htmx**.

O alvo é o comportamento HTTP e o modelo operacional do PocketBase — API REST,
regras de acesso, autenticação, realtime, arquivos, hooks, migrações, CLI — não
uma cópia do código Go nem da interface Svelte.

> **Como usar**
>
> 1. Copie o bloco da seção 1 e entregue a um agente de código como prompt
>    inicial.
> 2. A seção 2 em diante é a especificação de referência que o agente deve
>    consultar durante a execução (pode anexar o arquivo inteiro ao prompt).
> 3. Cada onda da seção 4 tem critério de aceite verificável. Não avance de onda
>    sem ele.
>
> O que este documento **não** é: uma cópia do PocketBase. É um contrato de
> comportamento escrito a partir do código-fonte público, da documentação e de
> observação black-box do binário de referência.

Fatos de configuração, rotas, formatos e defaults foram conferidos contra o
`pocketbase/pocketbase` **v0.40.6**, commit
`9d281ccaba65f2686eb7b0b614ccda72d1116a1b`, em **2026-10-23**. O PocketBase
evolui rápido: antes de implementar uma onda, reconfira em `apis/`, `core/`,
`tools/` e `migrations/` do commit fixado.

---

## 1. O prompt

```text
Você é engenheiro sênior de backend, banco de dados e front-end server-side.
Sua missão é construir, neste repositório, um monorepo em Rust que seja um
substituto compatível com o PocketBase e um painel administrativo em HTML
gerado no servidor com htmx, equivalente funcional ao dashboard oficial.
Trabalhe como um time de produto: fatias verticais pequenas, testes,
documentação e medição de compatibilidade em cada entrega.

CONTEXTO
O PocketBase é um backend open source (MIT) escrito em Go, distribuído como um
único binário, com:
- API REST em /api (SQLite, sem ORM externo);
- painel administrativo SvelteKit embutido em /_/;
- coleções dinâmicas (base, auth e view), campos tipados e regras de acesso
  por operação;
- autenticação por e-mail/senha, OTP, OAuth2, MFA e tokens JWT;
- realtime por Server-Sent Events (SSE);
- upload/download de arquivos com thumbnails e arquivos protegidos;
- logs de requisições, backups, crons, console SQL e settings;
- hooks em Go e um runtime JavaScript (JSVM, baseado em goja);
- migrações versionadas, CLI `serve`, `superuser` e `migrate`.
O alvo NÃO é copiar o código: é reproduzir o comportamento observável e o
modelo de operação (um binário, um diretório de dados, SQLite).

OBJETIVO MENSURÁVEL
Um cliente que hoje aponta para o PocketBase deve apontar para o seu binário
mudando apenas a URL, e continuar funcionando. A definição operacional de
"compatível" é, em ordem de prioridade:
1. o harness de contrato compara o mesmo request no binário de referência
   (v0.40.6) e no seu servidor e exige JSON normalizado equivalente;
2. os cenários do SDK JS oficial (pocketbase) rodam contra o seu servidor com
   uma lista explícita de exclusões justificadas;
3. os guias oficiais (REST, autenticação, realtime, arquivos, hooks e
   migrações) rodam sem alteração;
4. o painel cobre o fluxo do dashboard: instalação, login, schema, dados,
   logs, settings, backups, crons e SQL.

REGRAS INEGOCIÁVEIS
- Rust stable, edition 2024, workspace com crates pequenos e dependências
  explícitas. Sem framework JavaScript no build do painel: HTML no servidor
  (maud) + htmx embutido via include_bytes!.
- SQLite é o banco. Um app por processo/instância, como no PocketBase. Não
  reimplemente MongoDB nem Postgres.
- Compatibilidade de contrato: caminhos, headers, corpos, códigos de status,
  semântica de regras, formato de datas, IDs, tokens e arquivos seguem o
  PocketBase.
- Segurança primeiro: senha com bcrypt, JWT HS256, tokenKey por registro,
  rate limit, limite de corpo, validação de regra, CSRF no painel, sessão em
  cookie httpOnly + SameSite=Strict, nenhum segredo em HTML/log.
- Não copie código, CSS, textos ou assets do Painel Svelte nem do repositório
  do PocketBase. Consulte a documentação pública e o comportamento do binário
  de referência. Se usar qualquer trecho derivado, registre em NOTICE e
  mantenha a licença MIT.
- "PocketBase" é o nome do projeto original. O seu projeto tem nome próprio e
  se declara "compatível com a API do PocketBase", sem sugerir afiliação.
- Código e identificadores em inglês; documentação e mensagens de produto em
  português. Commits convencionais, um assunto por PR.

ARQUITETURA OBRIGATÓRIA (detalhe na seção 2)
crates/
  base-core         domínio puro: Collection, Field, Record, Rule, Token, erro
  base-storage      trait Storage + SQLite (data.db, auxiliary.db) + migrações
  base-http         axum: /api, middlewares, CORS, body limit, gzip, SSE
  base-auth         senha, JWT, OTP, OAuth2, MFA, auth origins, alertas
  base-files        filesystem local, S3 opcional, thumbs, arquivos protegidos
  base-realtime     broker de assinaturas, matching de regras, SSE
  base-jobs         crons, backups, logs, SQL, limpeza e manutenção
  base-hooks        barramento de hooks Rust + runtime JS opcional (rquickjs)
  base-migrations   runner, snapshots de coleções, automigrate
  base-server       composição: config, lifecycle, rotas, instalador
  base-cli          binário: serve, superuser, migrate, update, doctor
  base-admin        views maud + CSS + htmx + telas do painel
tests/
  conformance/      harness que compara seu servidor com o de referência
  sdk-js/           cenários do SDK JS oficial contra o seu servidor
  panel/            e2e do painel (Playwright)
  load/             k6/oha: CRUD, query, login, SSE
docker/             compose com o binário de referência fixado
docs/               ADRs, paridade, guia, runbook

FLUXO DE TRABALHO
1. Antes de codar uma onda, escreva o teste de contrato que ela precisa passar
   (request/response dourado ou cenário do SDK).
2. Implemente a menor fatia vertical que faz o teste passar, do SQLite à rota.
3. Rode: cargo fmt --all --check; cargo clippy --workspace --all-targets -- -D
   warnings; cargo test --workspace; e o harness de conformidade.
4. Atualize docs/PARIDADE.md (funcionalidade, estado, evidência, divergência) e
   o CHANGELOG.
5. Só então abra o PR. PR sem teste de contrato é rejeitado.
6. Ao terminar cada onda, rode os cenários do SDK JS e registre a taxa de
   aprovação e as exclusões. Regressão de compatibilidade bloqueia a onda
   seguinte.

ENTREGÁVEIS POR ONDA
- código + testes + migração de banco + documentação + evidência de paridade;
- um relatório curto em docs/relatorios/onda-N.md com: o que passou, o que não
  passou, por quê, e o próximo passo.

COMECE ASSIM (onda 0)
- crie o workspace, o binário base-cli com `serve`, o endpoint /api/health, o
  instalador de primeiro superuser, a configuração por flags/env e o CI;
- suba o binário de referência v0.40.6 em docker/compose.yaml e escreva o
  harness que envia o mesmo request aos dois e compara JSON normalizado;
- só considere a onda 0 pronta quando /api/health tiver o mesmo formato (a
  versão pode diferir), o instalador criar o primeiro superuser e o harness
  rodar no CI.

NÃO PARE NO CRUD. Um clone sem autenticação completa, realtime, arquivos,
hooks, migrações, backups e painel não é um substituto do PocketBase. A ordem e
os critérios estão na seção 4.
```

---

## 2. Especificação de referência

### 2.1 Escopo e repositórios

| Repositório | Licença | Papel | Onde entra no monorepo |
|---|---|---|---|
| `pocketbase/pocketbase` | MIT | Servidor Go, dashboard Svelte, CLI, JSVM, migrações | `base-*` (núcleo da obra) |
| `pocketbase/js-sdk` | MIT | Cliente JavaScript oficial | `tests/sdk-js/` (oráculo de cliente) |
| `pocketbase/dart-sdk` | MIT | Cliente Dart/Flutter | validação manual/best-effort |
| `pocketbase/site` | ver LICENSE | Documentação oficial | fonte de verdade da linguagem |
| `pocketbase/benchmarks` | ver LICENSE | Benchmarks | referência de carga |

O contrato é o servidor. Se o SDK JS oficial conversa com o seu binário e os
requests dourados batem, os demais SDKs são consequência. O dashboard Svelte
não é reimplementado linha a linha: o painel htmx cobre as mesmas operações.

O que **não** está no escopo:

- MongoDB, Postgres, Redis, filas externas ou múltiplos apps no mesmo processo;
- compatibilidade binária com plugins Go do PocketBase;
- Node.js/npm dentro do runtime JS (o JSVM do PocketBase não é Node);
- replicar textos, ícones, CSS ou telas do dashboard oficial.

### 2.2 Princípios de arquitetura

1. **Domínio sem I/O.** `base-core` não conhece axum, SQLite, SSE nem JS.
   Collection, Field, Record, Rule, Token e validação são testáveis sem rede.
2. **Um adaptador por preocupação.** Banco, arquivos, e-mail, realtime,
   relógio e runtime JS são traits. O servidor compõe; a configuração escolhe.
3. **A rota é fina.** O handler traduz HTTP → comando de domínio → resposta.
   Regra de negócio não mora no handler.
4. **Compatibilidade é teste.** Cada rota tem pelo menos um caso dourado e um
   caso de autorização (anônimo, dono, outro usuário, superuser).
5. **Tudo é observável.** Toda requisição gera log estruturado com método, URL,
   status, duração, IP e coleção de auth; nunca o token.
6. **Falha alto e cedo na configuração.** Diretório inválido, migração
   quebrada, hook inválido, chave de criptografia ausente: o processo não sobe
   e diz por quê.
7. **Um binário, um diretório.** `pb_data` (ou o diretório escolhido) contém
   bancos, storage, backups e settings. Copiar o diretório é copiar o app.

### 2.3 Modelo de dados

#### 2.3.1 Diretório de dados

O layout observável do PocketBase é:

```text
pb_data/
  data.db          # coleções, registros, settings (_params), _migrations
  auxiliary.db     # logs (_logs)
  storage/         # arquivos das coleções: <collectionId>/<recordId>/<filename>
  backups/         # arquivos .zip de backup
```

O diretório padrão é `pb_data`, relativo ao diretório de trabalho (ou ao
executável, conforme a distribuição). `--dir` troca a raiz. O clone deve
respeitar exatamente esses nomes e a possibilidade de copiar um `pb_data`
existente.

SQLite é configurado com:

```text
busy_timeout(10000)
journal_mode(WAL)
journal_size_limit(200000000)
synchronous(NORMAL)
foreign_keys(ON)
temp_store(MEMORY)
cache_size(-32000)
defensive(1)
```

O pool padrão do banco principal é 120 conexões abertas e 15 idle; o auxiliar
(logs) usa 20/3. O timeout padrão de `SELECT` é 30 s (`--queryTimeout`).

#### 2.3.2 Tabelas de sistema

| Tabela | Banco | Papel |
|---|---|---|
| `_collections` | `data.db` | Metadados de todas as coleções |
| `_params` | `data.db` | Settings do app (linha `id = "settings"`, valor JSON) |
| `_migrations` | `data.db` | Migrações aplicadas (`file` PK, `applied` inteiro) |
| `_logs` | `auxiliary.db` | Logs de requisição e de aplicação |
| `_superusers` | `data.db` | Coleção auth de superusuários |
| `_mfas` | `data.db` | Sessões pendentes de MFA |
| `_otps` | `data.db` | Códigos OTP |
| `_externalAuths` | `data.db` | Vínculos OAuth2 |
| `_authOrigins` | `data.db` | Fingerprints de dispositivos para alertas |
| `users` | `data.db` | Coleção auth de exemplo, criada no primeiro boot |

Schema de `_collections`:

```sql
CREATE TABLE _collections (
  id         TEXT PRIMARY KEY DEFAULT ('r'||lower(hex(randomblob(7)))) NOT NULL,
  system     BOOLEAN DEFAULT FALSE NOT NULL,
  type       TEXT DEFAULT "base" NOT NULL,
  name       TEXT UNIQUE NOT NULL,
  fields     JSON DEFAULT "[]" NOT NULL,
  indexes    JSON DEFAULT "[]" NOT NULL,
  listRule   TEXT DEFAULT NULL,
  viewRule   TEXT DEFAULT NULL,
  createRule TEXT DEFAULT NULL,
  updateRule TEXT DEFAULT NULL,
  deleteRule TEXT DEFAULT NULL,
  options    JSON DEFAULT "{}" NOT NULL,
  created    TEXT DEFAULT (strftime('%Y-%m-%d %H:%M:%fZ')) NOT NULL,
  updated    TEXT DEFAULT (strftime('%Y-%m-%d %H:%M:%fZ')) NOT NULL
);
```

`_params`:

```sql
CREATE TABLE _params (
  id      TEXT PRIMARY KEY DEFAULT ('r'||lower(hex(randomblob(7)))) NOT NULL,
  value   JSON DEFAULT NULL,
  created TEXT DEFAULT (strftime('%Y-%m-%d %H:%M:%fZ')) NOT NULL,
  updated TEXT DEFAULT (strftime('%Y-%m-%d %H:%M:%fZ')) NOT NULL
);
```

`_migrations`:

```sql
CREATE TABLE IF NOT EXISTS _migrations (
  file    VARCHAR(255) PRIMARY KEY NOT NULL,
  applied INTEGER NOT NULL
);
```

`_logs` contém `id`, `created`, `level` (inteiro), `message` e `data` (JSON).

#### 2.3.3 Coleções

Uma coleção tem:

```json
{
  "id": "pbc_1234567890",
  "name": "posts",
  "type": "base",
  "system": false,
  "fields": [],
  "indexes": [],
  "listRule": null,
  "viewRule": null,
  "createRule": null,
  "updateRule": null,
  "deleteRule": null,
  "created": "2026-10-23 12:00:00.000Z",
  "updated": "2026-10-23 12:00:00.000Z"
}
```

Tipos:

| Tipo | Semântica | Campos automáticos |
|---|---|---|
| `base` | CRUD comum | `id` |
| `auth` | CRUD + login/tokens | `id`, `password`, `tokenKey`, `email`, `emailVisibility`, `verified` |
| `view` | Somente leitura, definida por SQL | campos gerados a partir de `viewQuery` |

Observações que o clone deve reproduzir:

- `NewBaseCollection` cria **apenas** o campo `id`. `created` e `updated` são
  campos `autodate` de usuário; só existem se forem incluídos na criação. As
  coleções de sistema `users` e `_superusers` já os incluem. O painel deve
  oferecê-los por padrão, mas a API não os inventa.
- `auth` cria os campos de sistema com `password` oculto (`hidden`), `tokenKey`
  oculto e `email` obrigatório.
- `view` exige `viewQuery`; os campos são derivados da consulta e não podem ser
  alterados manualmente.
- Nome de coleção: `^\w+$`, 1–255 caracteres, único; não pode conter `_via_`.
  Coleções de sistema não podem ser renomeadas, ter o tipo trocado ou ser
  apagadas.
- `system: true` marca coleções internas; o superuser as vê, mas não as trata
  como coleções comuns no painel.
- `options` guarda as opções específicas do tipo: auth (`authRule`,
  `manageRule`, `passwordAuth`, `oauth2`, `otp`, `mfa`, `authAlert`, tokens e
  templates) ou view (`viewQuery`). Na serialização JSON da API, essas opções
  aparecem **no topo** do objeto da coleção, não dentro de `options`.
- Segredos de auth (`authToken.secret`, `fileToken.secret`, etc.) nunca são
  serializados; o JSON os omite.
- Índices são um array de strings SQL (`CREATE INDEX ...` ou
  `CREATE UNIQUE INDEX ...`), validados antes de aplicar. Os nomes devem ser
  únicos.

#### 2.3.4 Campos

Todo campo tem `id`, `name`, `type`, `system`, `hidden`, `presentable` e
`required`. Nome: `^\w+$`, 1–100 caracteres, único na coleção; não pode ser
`id`, `collectionId`, `collectionName`, `expand` nem conter `_via_`.

| Tipo | Opções JSON | Coluna SQLite | Observações |
|---|---|---|---|
| `text` | `min`, `max`, `pattern`, `autogeneratePattern`, `primaryKey` | `TEXT` | `primaryKey` só no `id` |
| `number` | `min`, `max`, `onlyInt` | `NUMERIC DEFAULT 0 NOT NULL` | |
| `bool` | — | `BOOLEAN DEFAULT FALSE NOT NULL` | |
| `email` | `exceptDomains`, `onlyDomains` | `TEXT DEFAULT '' NOT NULL` | |
| `url` | `exceptDomains`, `onlyDomains` | `TEXT DEFAULT '' NOT NULL` | |
| `editor` | `convertUrls`, `maxSize` | `TEXT DEFAULT '' NOT NULL` | HTML |
| `date` | `min`, `max` | `TEXT DEFAULT '' NOT NULL` | string de data |
| `autodate` | `onCreate`, `onUpdate` | `TEXT DEFAULT '' NOT NULL` | gerado no save |
| `select` | `values`, `maxSelect` | `JSON DEFAULT '[]' NOT NULL` | `maxSelect=1` → string; senão array |
| `file` | `maxSelect`, `maxSize`, `mimeTypes`, `thumbs`, `protected` | `JSON DEFAULT '[]' NOT NULL` | `maxSize=0` → 5 MB |
| `relation` | `collectionId`, `cascadeDelete`, `minSelect`, `maxSelect` | `JSON DEFAULT '[]' NOT NULL` | `maxSelect=1` → string; senão array |
| `json` | `maxSize` | `JSON DEFAULT NULL` | qualquer JSON |
| `geoPoint` | — | `JSON DEFAULT '{"lon":0,"lat":0}' NOT NULL` | `{lon, lat}` |
| `password` | `min`, `max`, `pattern`, `cost` | `TEXT DEFAULT '' NOT NULL` | hash bcrypt; nunca serializado |

Campos `file`, `relation` e `select` com `maxSelect = 1` retornam **string**;
com `maxSelect > 1` retornam **array**. `file` retorna os nomes normalizados dos
arquivos, não URLs; o cliente monta `/api/files/...`.

#### 2.3.5 IDs, datas e JSON

- IDs de registro: 15 caracteres `[a-z0-9]`, gerados pelo servidor se não
  enviados. IDs de coleção: `pbc_...` (checksum) ou string estável; `users`
  usa `_pb_users_auth_`.
- Datas são strings UTC no formato `"YYYY-MM-DD HH:MM:SS.sssZ"` — **espaço**,
  não `T`, e sempre UTC. Na entrada, o servidor aceita formatos comuns, mas a
  saída é sempre canônica.
- `null` e `""` são diferentes em regras: `null` significa "somente
  superuser"; `""` significa "público".
- Todo JSON é serializado de forma determinística (chaves ordenadas) quando
  aplicável; o cliente não deve depender da ordem, mas os testes podem.

#### 2.3.6 Regras de acesso

Cada coleção tem cinco regras: `listRule`, `viewRule`, `createRule`,
`updateRule`, `deleteRule`. Auth acrescenta `authRule` e `manageRule`; view não
tem create/update/delete.

| Valor | Significado |
|---|---|
| `null` | somente superuser |
| `""` | público (qualquer um, inclusive anônimo) |
| expressão | filtro avaliado contra o registro e o request |

Semântica:

- `listRule` é aplicada como `WHERE` na consulta; ela também filtra o que o
  usuário pode listar.
- `viewRule` é avaliada por registro.
- `createRule` é avaliada com `@request.body.*` e `@request.auth.*`.
- `updateRule` é avaliada contra o registro atual e o corpo; `deleteRule`
  contra o registro atual.
- `authRule` é aplicada **depois** da autenticação, antes de devolver o token.
  `null` desabilita login; `""` libera qualquer registro auth.
- `manageRule` concede poderes administrativos sobre registros auth (trocar
  senha sem a antiga, alterar e-mail/verified). É somada a create/update.
- Superuser ignora todas as regras.
- Em `filter` e `sort` de query string, usuários comuns **não** podem usar
  `@collection.` nem `@request.`; isso é 403. Campos `hidden` só são
  filtráveis/ordenáveis por superuser.
- `@request.context` identifica o contexto: `default`, `batch`, `oauth2`,
  `protectedFile`.
- `@request.body.<campo>` aceita modificadores `:each`, `:isset`, `:length`,
  `:lower`, `:changed`.

#### 2.3.7 Coleções de exemplo

No primeiro boot, além das tabelas de sistema, o PocketBase cria:

- `_superusers`: auth, sistema, com `email` obrigatório; token de 1 dia.
- `users`: auth, ID `_pb_users_auth_`, com `name` (texto 255), `avatar`
  (file, 1, imagens jpeg/png/svg/gif/webp) e `created`/`updated` (`autodate`).
  Regras: list/view/update/delete `id = @request.auth.id`, create `""`
  (signup público). OAuth2 mapeia `name` e `avatar`.

O clone deve criar essas coleções de forma idempotente e não deve sobrescrever
dados existentes.

### 2.4 API REST

#### 2.4.1 Convenções

- Base: `/api`. Não há versionamento de path; o contrato é a série v0.40.x.
- Autenticação: header `Authorization: <token>`. O prefixo `Bearer ` é aceito
  por compatibilidade, mas não é obrigatório.
- Conteúdo: JSON (`application/json`) ou `multipart/form-data` para uploads.
- Sucesso JSON: objeto ou array direto; listas usam envelope.
- `?fields=` na query aplica uma projeção ao JSON de respostas 2xx (inclusive
  aninhada); não altera a consulta SQL.
- Erros sempre no formato `{ "status": int, "message": string, "data": {...} }`.
- CORS: padrão `*`; `--origins` restringe. Preflight responde 204.
- Corpo: 32 MB por padrão; coleções com campos `file` calculam um limite
  dinâmico a partir de `maxSize`/`maxSelect`. Acima do limite: 413.
- Headers de segurança nas respostas da API: `X-XSS-Protection`,
  `X-Content-Type-Options: nosniff`, `X-Frame-Options: SAMEORIGIN`,
  `Cross-Origin-Opener-Policy: same-origin`. Downloads de arquivo removem
  `X-Frame-Options` para permitir embed.
- Gzip é usado nos assets do painel; a API responde JSON sem compressão por
  padrão.

#### 2.4.2 Mapa de rotas

| Método | Rota | Auth | Resposta |
|---|---|---|---|
| GET | `/api/health` | pública | `{code,message,data}` |
| GET | `/api/settings` | superuser | settings |
| PATCH | `/api/settings` | superuser | settings |
| POST | `/api/settings/test/s3` | superuser | 204 |
| POST | `/api/settings/test/email` | superuser | 204 |
| POST | `/api/settings/apple/generate-client-secret` | superuser | `{secret}` |
| GET | `/api/collections` | superuser | lista |
| POST | `/api/collections` | superuser | coleção (200) |
| GET | `/api/collections/{collection}` | superuser | coleção |
| PATCH | `/api/collections/{collection}` | superuser | coleção |
| DELETE | `/api/collections/{collection}` | superuser | 204 |
| DELETE | `/api/collections/{collection}/truncate` | superuser | 204 |
| PUT | `/api/collections/import` | superuser | 204 |
| GET | `/api/collections/meta/scaffolds` | superuser | objeto `{base,auth,view}` |
| GET | `/api/collections/meta/oauth2-providers` | superuser | provedores |
| POST | `/api/collections/meta/dry-run-view` | superuser | `{fields,sample}` |
| GET | `/api/collections/{collection}/records` | regra | lista |
| POST | `/api/collections/{collection}/records` | regra | registro (200) |
| GET | `/api/collections/{collection}/records/{id}` | regra | registro |
| PATCH | `/api/collections/{collection}/records/{id}` | regra | registro (200) |
| DELETE | `/api/collections/{collection}/records/{id}` | regra | 204 |
| GET | `/api/collections/{collection}/auth-methods` | pública | métodos |
| POST | `/api/collections/{collection}/auth-refresh` | auth da coleção | auth |
| POST | `/api/collections/{collection}/auth-with-password` | pública | auth |
| POST | `/api/collections/{collection}/auth-with-oauth2` | pública | auth |
| POST | `/api/collections/{collection}/request-otp` | pública | `{otpId}` |
| POST | `/api/collections/{collection}/auth-with-otp` | pública | auth |
| POST | `/api/collections/{collection}/request-password-reset` | pública | 204 |
| POST | `/api/collections/{collection}/confirm-password-reset` | pública | 204 |
| POST | `/api/collections/{collection}/request-verification` | pública | 204 |
| POST | `/api/collections/{collection}/confirm-verification` | pública | 204 |
| POST | `/api/collections/{collection}/request-email-change` | auth da coleção | 204 |
| POST | `/api/collections/{collection}/confirm-email-change` | pública | 204 |
| POST | `/api/collections/{collection}/impersonate/{id}` | superuser | auth |
| GET/POST | `/api/oauth2-redirect` | pública | HTML |
| GET | `/api/logs` | superuser | lista |
| DELETE | `/api/logs` | superuser | 204 |
| GET | `/api/logs/stats` | superuser | estatísticas |
| GET | `/api/logs/{id}` | superuser | log |
| GET | `/api/backups` | superuser | array |
| POST | `/api/backups` | superuser | 204 |
| POST | `/api/backups/upload` | superuser | 204 |
| GET | `/api/backups/{key}` | token de arquivo | zip |
| DELETE | `/api/backups/{key}` | superuser | 204 |
| POST | `/api/backups/{key}/restore` | superuser | 204 |
| GET | `/api/crons` | superuser | array |
| POST | `/api/crons/{id}` | superuser | 204 |
| GET | `/api/files/{collection}/{recordId}/{filename}` | regra/token | arquivo |
| POST | `/api/files/token` | auth | `{token}` |
| POST | `/api/batch` | opcional (cada subrequest segue as regras) | array de resultados |
| GET | `/api/realtime` | opcional | SSE |
| POST | `/api/realtime` | opcional | 204 |
| POST | `/api/sql` | superuser | resultado SQL |

`{collection}` aceita nome **ou** ID. `{id}` é o ID do registro.

Corpos auxiliares:

- `PUT /api/collections/import`: `{ "collections": [ ... ], "deleteMissing": false }`.
  Importa em transação; `deleteMissing` apaga as coleções que não estão na
  lista. Sucesso: 204.
- `GET /api/collections/meta/scaffolds`: retorna um **objeto** com as chaves
  `base`, `auth` e `view`, cada uma com uma coleção vazia (só os campos de
  sistema) e IDs limpos; o painel usa para montar o formulário.
- `POST /api/collections/meta/dry-run-view`: `{ "query": "SELECT ..." }`
  (máx. 5000 chars) e retorna `{ "fields": [...], "sample": [records] }`,
  com até 10 linhas. Serve para o editor de view inferir os campos.

#### 2.4.3 Health

`GET /api/health` responde 200:

```json
{ "code": 200, "message": "API is healthy.", "data": {} }
```

Com superuser autenticado, `data` inclui `canBackup`, `realIP` e, quando
aplicável, `possibleProxyHeader` (nome do header que sugere proxy reverso). O
formato básico deve ser idêntico; os campos extras de diagnóstico podem ser
omitidos se o teste não os exigir.

#### 2.4.4 Listas e busca

Toda lista (records, collections, logs) usa os mesmos parâmetros:

| Parâmetro | Padrão | Limite | Semântica |
|---|---|---|---|
| `page` | 1 | — | página 1-based |
| `perPage` | 30 | 1000 | itens por página |
| `skipTotal` | falso | — | se verdadeiro, `totalItems`/`totalPages` = -1 |
| `filter` | vazio | 200 expressões; 3500 chars cada | filtro |
| `sort` | vazio | 8 campos; 255 chars cada | `-campo`, `+campo`, `@random`, `@rowid` |

Resposta:

```json
{
  "items": [],
  "page": 1,
  "perPage": 30,
  "totalItems": 0,
  "totalPages": 0
}
```

Sem `sort`, a ordem não é garantida (o SDK costuma enviar `sort=-created`).
`@random` usa `RANDOM()`; `@rowid` ordena pelo `_rowid_` interno.

#### 2.4.5 Linguagem de filtro

Operadores:

| Operador | Significado |
|---|---|
| `=` | igual |
| `!=` | diferente |
| `>` `>=` `<` `<=` | comparação |
| `~` | contém / LIKE |
| `!~` | não contém |
| `?=` | qualquer valor igual (multi-valor/relação) |
| `?!=` | nenhum valor igual |
| `?>` `?>=` `?<` `?<=` | qualquer comparação |
| `?~` | qualquer valor contém |
| `?!~` | nenhum valor contém |

Lógicos: `&&`, `||`, `!`, parênteses.

Macros: `@now`, `@yesterday`, `@tomorrow`, `@second`, `@minute`, `@hour`,
`@day`, `@month`, `@weekday`, `@year`, `@todayStart`, `@todayEnd`,
`@monthStart`, `@monthEnd`, `@yearStart`, `@yearEnd`.

Funções: `geoDistance(lonA, latA, lonB, latB)` (Haversine, em metros) e
`strftime(format, valor, ...)`.

Acesso ao request: `@request.auth.*`, `@request.body.*`, `@request.query.*`,
`@request.headers.*`, `@request.method`, `@request.context`.

Join não relacional: `@collection.<coleção>.<campo>`.

Modificadores: `:each`, `:isset`, `:length`, `:lower`, `:changed`.

Exemplos:

```text
status = "published" && created >= @monthStart
author = @request.auth.id
tags ?= "rust"
geoDistance(location.lon, location.lat, 12.34, 56.78) < 5000
@collection.categories.slug ?= "backend"
```

#### 2.4.6 Registros

Listar:

```http
GET /api/collections/posts/records?page=2&perPage=50&filter=status="published"&sort=-created&expand=author
```

Ver:

```http
GET /api/collections/posts/records/RECORD_ID?expand=author.profile
```

Criar:

```http
POST /api/collections/posts/records
Content-Type: application/json

{ "title": "Olá", "status": "draft" }
```

Resposta 200 com o registro criado, incluindo `id`, `collectionId`,
`collectionName` e os campos. **Não** é 201.

Atualizar:

```http
PATCH /api/collections/posts/records/RECORD_ID
Content-Type: application/json

{ "title": "Novo título" }
```

Resposta 200 com o registro completo. `PATCH` é parcial.

Apagar: `DELETE .../records/{id}` → 204.

Upload multipart:

```http
POST /api/collections/posts/records
Content-Type: multipart/form-data

title=Olá
cover=@foto.jpg
```

Regras de escrita:

- Campos `file` aceitam arquivos multipart; em JSON, aceitam apenas nomes de
  arquivos já existentes (e o servidor valida a posse).
- `campo+` anexa, `+campo` prepende e `campo-` remove em campos multi-valor
  (`file`, `relation`, `select`).
- Campos `autodate` são calculados no save; o cliente não os define.
- `password` em auth é hash; `passwordConfirm` é validação de formulário, não
  coluna.
- `created`/`updated` só existem se a coleção os tiver como campos.
- Erros de validação retornam 400 com `data` por campo.
- `expand` aceita relação direta (`author`), encadeada (`author.profile`) e
  reversa (`comments_via_post`).
- `?fields=` projeta a resposta, por exemplo `?fields=id,title,author.name`.
- Um registro de coleção `view` é somente leitura; create/update/delete
  retornam erro.

#### 2.4.7 Autenticação

Todas as rotas de auth ficam sob `/api/collections/{collection}` e exigem uma
coleção do tipo `auth`.

Resposta de sucesso:

```json
{
  "token": "<jwt>",
  "record": { "id": "...", "collectionId": "...", "collectionName": "users", "...": "..." },
  "meta": {}
}
```

`meta` é opcional (OAuth2, por exemplo).

| Rota | Corpo | Resultado |
|---|---|---|
| `GET auth-methods` | — | métodos habilitados |
| `POST auth-with-password` | `{identity, password, identityField?}` | auth |
| `POST auth-with-oauth2` | `{provider, code, codeVerifier, redirectURL, createData?}` | auth |
| `POST request-otp` | `{email}` | `{otpId}` |
| `POST auth-with-otp` | `{otpId, password}` | auth |
| `POST request-password-reset` | `{email}` | 204 |
| `POST confirm-password-reset` | `{token, password, passwordConfirm}` | 204 |
| `POST request-verification` | `{email}` | 204 |
| `POST confirm-verification` | `{token}` | 204 |
| `POST request-email-change` | `{newEmail}` | 204 |
| `POST confirm-email-change` | `{token, password}` | 204 |
| `POST auth-refresh` | — (token no header) | auth renovado |
| `POST impersonate/{id}` | `{duration?}` | auth estático |

Detalhes:

- `auth-methods` retorna `password` (`enabled`, `identityFields`), `oauth2`
  (`enabled`, `providers[]` com `name`, `displayName`, `logo`, `state`,
  `authURL`, `codeVerifier`, `codeChallenge`, `codeChallengeMethod`), `mfa`
  (`enabled`, `duration`), `otp` (`enabled`, `duration`) e campos legados
  (`authProviders`, `usernamePassword`, `emailPassword`).
- Identidade padrão: `email`. Se a coleção tiver o campo `username` e ele for
  habilitado em `passwordAuth.identityFields`, o login pode usar username.
  Com múltiplos identity fields, o cliente pode enviar `identityField`.
- Falha de senha: 400 `"Failed to authenticate."` (sem revelar se o e-mail
  existe).
- `authRule` é checada após a senha; se falhar, 403.
- `request-password-reset` e `request-verification` retornam 204 mesmo quando o
  e-mail não existe (anti-enumeração). `request-email-change` exige auth e
  valida que o novo e-mail é diferente e único na coleção.
- Confirmar reset de senha, OTP e troca de e-mail marca `verified = true`
  quando o e-mail é comprovado.
- Trocar a senha rotaciona `tokenKey`, invalidando tokens antigos.
- `auth-refresh` devolve um token novo se o token atual for renovável; tokens
  de impersonate não são renováveis e voltam iguais.
- `impersonate/{id}` exige superuser, aceita `duration` em segundos (0 usa o
  padrão da coleção) e devolve um token estático, não renovável.
- MFA: com `mfa.enabled`, a primeira autenticação responde **401** com
  `{"mfaId":"..."}`; o cliente repete a autenticação com `?mfaId=` ou
  `{"mfaId":"..."}` usando um método **diferente** (password, oauth2 ou otp).
- OTP: 8 dígitos por padrão, validade 3 min; `request-otp` pode reutilizar o
  último OTP se houver abuso.
- OAuth2: o servidor guarda `state` e PKCE; o redirect global
  `/api/oauth2-redirect` devolve HTML que fecha a janela e entrega o código.
  Provedores conhecidos: Apple, Bitbucket, Box, Discord, Facebook, Gitea,
  Gitee, GitHub, GitLab, Google, Instagram, Kakao, Lark, Linear, LiveChat,
  Mailcow, Microsoft, Monday, Notion, OIDC (e OIDC2/OIDC3), Patreon,
  Planning Center, Spotify, Strava, Trakt, Twitch, Twitter, VK, WakaTime,
  Yandex. A configuração é genérica (`authURL`, `tokenURL`, `userInfoURL`,
  `clientId`, `clientSecret`, `pkce`, `extra`), então um motor OAuth2 genérico
  cobre todos; presets só preenchem URLs e detalhes.
- Apple: `POST /api/settings/apple/generate-client-secret` recebe
  `{clientId, teamId, keyId, privateKey, duration}` e devolve `{secret}`.
- Auth alert: por padrão habilitado; um novo fingerprint em `_authOrigins`
  dispara e-mail de alerta.
- `superuserIPs` (settings) restringe o login e o uso de tokens de superuser a
  IPs/CIDRs listados.

Tokens:

- JWT HS256. Claims comuns: `id`, `collectionId`, `type`, `exp`; tokens de
  auth acrescentam `refreshable`.
- `type`: `auth`, `file`, `verification`, `passwordReset`, `emailChange`.
- Chave de assinatura: `tokenKey` + o segredo do tipo (`authToken.secret`,
  `fileToken.secret`, etc.). Trocar `tokenKey` invalida tokens existentes.
- Durações padrão: auth 5 dias (432000 s); superuser 1 dia (86400 s); reset e
  troca de e-mail 30 min; verificação 1 dia; file token 3 min; MFA 10 min.
- `file` token é usado em `?token=` para arquivos protegidos e backups; não é
  aceito como `Authorization` de API.
- A resposta de auth não usa o header `Authorization`; o token vai no corpo. O
  cliente o envia no header nas chamadas seguintes.

#### 2.4.8 Batch

`POST /api/batch` só funciona com `settings.batch.enabled = true`; desabilitado
retorna 403.

```json
{
  "requests": [
    { "method": "POST", "url": "/api/collections/posts/records", "body": { "title": "A" } },
    { "method": "PATCH", "url": "/api/collections/posts/records/ID", "body": { "title": "B" } },
    { "method": "DELETE", "url": "/api/collections/posts/records/ID2" },
    { "method": "PUT", "url": "/api/collections/posts/records", "body": { "id": "ID3", "title": "C" } }
  ]
}
```

- Ações suportadas: `POST .../records`, `PATCH .../records/{id}`,
  `DELETE .../records/{id}` e `PUT .../records` (upsert por `body.id`).
- `url` pode incluir query string (ex.: `?expand=author`).
- Tudo roda em uma transação, com timeout padrão de 3 s (configurável).
- Máximo padrão: 50 requests; corpo máximo 128 MB.
- Resposta 200: array na mesma ordem, cada item `{status, body}`.
- Qualquer falha aborta a transação e retorna 400; nada é parcialmente
  aplicado.
- Multipart: campos vão em `@jsonPayload` e arquivos em
  `requests.N.campo`/`requests[N].campo`.

#### 2.4.9 Settings

`GET /api/settings` retorna o objeto completo, mas **omite** os segredos já
definidos (`smtp.password`, `s3.secret`, `backups.s3.secret`). `PATCH` faz merge
parcial sobre o clone: campos omitidos permanecem; para **não** apagar um
segredo, não envie a chave. Enviar string vazia limpa o segredo. No JSON abaixo
os campos de segredo aparecem vazios apenas para indicar o formato; a resposta
real os omite quando estão preenchidos.

Defaults (arrays vazios podem vir como `null`; normalize no cliente):

```json
{
  "meta": {
    "appName": "Acme",
    "appURL": "http://localhost:8090",
    "hideControls": false,
    "senderName": "Support",
    "senderAddress": "support@example.com",
    "accentColor": "#1055c9"
  },
  "logs": { "maxDataSize": 0, "maxDays": 5, "minLevel": 0, "logIP": true, "logAuthId": false },
  "smtp": {
    "enabled": false, "host": "smtp.example.com", "port": 587,
    "username": "", "password": "", "authMethod": "", "tls": false,
    "localName": ""
  },
  "s3": { "enabled": false, "bucket": "", "region": "", "endpoint": "",
          "accessKey": "", "secret": "", "forcePathStyle": false },
  "backups": { "cron": "", "cronMaxKeep": 3,
               "s3": { "enabled": false, "bucket": "", "region": "",
                       "endpoint": "", "accessKey": "", "secret": "",
                       "forcePathStyle": false } },
  "rateLimits": {
    "enabled": false,
    "rules": [
      { "label": "*:auth", "audience": "", "duration": 3, "maxRequests": 2 },
      { "label": "*:create", "audience": "", "duration": 5, "maxRequests": 20 },
      { "label": "/api/batch", "audience": "", "duration": 1, "maxRequests": 3 },
      { "label": "/api/", "audience": "", "duration": 10, "maxRequests": 300 }
    ],
    "excludedIPs": []
  },
  "trustedProxy": { "headers": [], "useLeftmostIP": false },
  "batch": { "enabled": false, "maxRequests": 50, "timeout": 3, "maxBodySize": 0 },
  "superuserIPs": []
}
```

- `rateLimits.rules[].label`: tag (`*:auth`, `*:create`), path exato,
  prefixo terminando em `/` (`/api/`) ou `MÉTODO /path/`.
- `audience`: `""` (todos), `"@guest"`, `"@auth"`.
- Rate limit é por IP, **janela fixa** (`duration` segundos, reseta o
  contador); superusers e IPs em `excludedIPs` não são limitados. Ao exceder,
  429 sem `Retry-After`.
- `trustedProxy.headers` define de quais headers ler o IP real;
  `useLeftmostIP` escolhe a ponta esquerda da cadeia.
- `settings/logs.maxDays` controla a retenção; a limpeza roda a cada 6 h.
  `logs.maxDataSize = 0` usa o fallback de ~16 KB por entrada.
- `settings.backups.cron` agenda o backup automático; `cronMaxKeep` mantém os
  N últimos backups automáticos (`0` = sem limite).

Testes:

- `POST /api/settings/test/s3` com `{filesystem:"storage"|"backups"}` → 204.
- `POST /api/settings/test/email` com `{email, template?, collection?}` → 204.

#### 2.4.10 Logs

- `GET /api/logs` usa o mesmo provider de busca (`page`, `perPage`, `filter`,
  `sort`, `skipTotal`). Campos filtráveis: `id`, `created`, `level`,
  `message`, `data` e `data.<caminho>`.
- `GET /api/logs/stats?filter=...` retorna `[{ "date": "...", "total": n }]`
  agrupado por hora.
- `GET /api/logs/{id}` retorna o log.
- `DELETE /api/logs` trunca a tabela e faz VACUUM; 204.
- `data` contém, quando aplicável: `url`, `method`, `status`, `execTime`,
  `referer`, `userAgent`, `auth` (nome da coleção), `authId` (se
  `logAuthId`), `userIP`, `remoteIP`, `error`, `details`, `meta`.
- Nível é inteiro no estilo slog: `-4` debug, `0` info, `4` warn, `8` error.

#### 2.4.11 Backups

- `GET /api/backups` retorna um array de `{key, size, modified}`.
- `POST /api/backups` com `{name?}` cria um zip; 204. Nome deve casar
  `^[a-z0-9_-]+\.zip$`, 1–150 chars, e não existir. Sem nome, o servidor gera
  `{appNameSnake}_{YYYYMMDDHHMMSS}.zip`; o automático gera
  `@auto_pb_backup_{appNameSnake}_{YYYYMMDDHHMMSS}.zip`.
- `POST /api/backups/upload` (multipart, campo `file`) aceita apenas
  `application/zip`; 204.
- `GET /api/backups/{key}?token=<fileToken>` baixa; o token precisa ser de
  superuser (gerado por `POST /api/files/token`).
- `DELETE /api/backups/{key}` → 204; recusa apagar o backup em uso.
- `POST /api/backups/{key}/restore` → 204 e restaura de forma assíncrona; o
  processo reinicia. A conexão pode cair.
- O zip contém o `pb_data` (via `VACUUM INTO` para os dois bancos + storage),
  **exceto** `backups/`, `.pb_temp_to_delete/`, `.notify/`, `.autocert_cache/`
  e `lost+found/`. Arquivos em S3 **não** entram no zip; backup de S3 é
  responsabilidade da configuração de storage.
- Só um backup/restore por vez; concorrente retorna 400.

#### 2.4.12 Crons

- `GET /api/crons` retorna `[{ "id": "...", "expression": "..." }]`, com jobs
  internos (`__pb...`) por último.
- `POST /api/crons/{id}` dispara o job em background; 204.
- Jobs internos: `__pbDBOptimize__` (diário, 00:00), `__pbLogsCleanup__`
  (a cada 6 h), `__pbMFACleanup__` e `__pbOTPCleanup__` (horários),
  `__pbAutoBackup__` (se configurado).

#### 2.4.13 SQL

`POST /api/sql` com `{ "query": "SELECT ..." }`, somente superuser.

Resposta:

```json
{
  "execTime": 3,
  "affectedRows": 0,
  "columns": [{ "name": "id", "type": "TEXT", "nullable": false }],
  "rows": [["..."]]
}
```

- Máximo de 1000 linhas e 3 min de execução.
- `SELECT`/`WITH` são leitura; comandos de escrita conhecidos (`INSERT`,
  `CREATE`, `UPDATE`, `DELETE`, `DROP`, `DETACH`, `ALTER`, `REPLACE`) rodam em
  transação.
- Erros retornam 400 com a mensagem crua do SQLite no `message`.
- O console é poderoso: exige superuser, CSRF no painel e confirmação para
  escrita.

### 2.5 Realtime

`GET /api/realtime` abre uma conexão SSE. O servidor envia imediatamente:

```text
id:<clientId>
event:PB_CONNECT
data:{"clientId":"<40 chars>"}

```

Formato de cada evento:

```text
id:<clientId>
event:<tópico>
data:{"action":"create|update|delete","record":{...}}

```

- `Content-Type: text/event-stream`, `Cache-Control: no-store`,
  `X-Accel-Buffering: no`.
- Sem replay/histórico. Ao reconectar, o cliente recebe um novo `clientId` e
  precisa se inscrever de novo.
- Idle timeout: 5 min; duração máxima: 30 min. Depois disso, a conexão fecha e
  o cliente reconecta.
- A conexão SSE em si pode ser anônima; a autenticação é associada ao cliente
  no `POST /api/realtime`, que leva o header `Authorization`. É assim que o SDK
  oficial funciona no browser (o `EventSource` nativo não envia headers). O
  cliente pode começar anônimo e fazer upgrade para auth; trocar de um auth
  para outro é proibido.

Inscrições: `POST /api/realtime` com:

```json
{ "clientId": "...", "subscriptions": ["posts", "posts/RECORD_ID", "posts/*"] }
```

- 204. A lista substitui as inscrições anteriores.
- O IP do POST deve ser o mesmo da conexão SSE.
- Tópicos:
  - `coleção` (equivalente a `coleção/*`, mantido por compatibilidade);
  - `coleção/*` — todos os registros, sujeito a `listRule`;
  - `coleção/{id}` — um registro, sujeito a `viewRule`.
- Opções por tópico:
  `?options={"query":{"filter":"...","expand":"...","fields":"..."},"headers":{"X-Token":"..."}}`
  (JSON URL-encoded). O `event` enviado é o tópico completo, incluindo as
  opções.
- O servidor reavalia as regras a cada evento; um registro que deixou de
  satisfazer a regra não é entregue.
- `action` é `create`, `update` ou `delete`; `record` é o estado atual (no
  delete, o último estado).
- `filter` nas opções restringe a entrega; `expand` e `fields` enriquecem o
  registro.
- Em transações, os eventos só saem depois do commit; rollback não emite.
- Batch usa `@request.context = "batch"` na avaliação das regras.

### 2.6 Arquivos

Rotas:

```text
GET  /api/files/{collection}/{recordId}/{filename}
POST /api/files/token
```

- `collection` aceita nome ou ID. `filename` é o nome normalizado armazenado no
  campo (não o nome original).
- Upload: multipart em create/update, campo com o nome do campo `file`.
- Nome normalizado: `snake_case` do nome original + `_` + 10 caracteres
  aleatórios + extensão (ex.: `minha_foto_a1b2c3d4e5.jpg`). Extensão é
  detectada/validada; nomes muito curtos ganham parte aleatória.
- Campo `file` com `maxSelect=1` retorna string; com `maxSelect>1` retorna
  array. `maxSize=0` → 5 MB por arquivo. `mimeTypes` restringe.
- `protected: true` exige `?token=<fileToken>` **e** que a `viewRule` do
  registro permita o acesso. O token é gerado por `POST /api/files/token`
  (auth obrigatória), dura 3 min e é do tipo `file`.
- `?thumb=100x100` gera/entrega thumbnail para imagens; tamanhos aceitos:
  `100x100` (padrão) e os listados em `thumbs` do campo. Formatos: `WxH`,
  `WxHt`, `WxHb`, `WxHf`, `0xH`, `Wx0`. Se não for imagem ou o tamanho não for
  suportado, entrega o original.
- `?download=1` força `Content-Disposition: attachment`.
- Suporta `Range`/`HEAD` via servidor de arquivos.
- Storage local: `pb_data/storage/<collectionId>/<recordId>/<filename>` e
  thumbnails em `.../thumbs_<filename>/<size>_<filename>`.
- Ao apagar registro/coleção, os arquivos são removidos de forma assíncrona.
- S3: `settings.s3` (bucket, region, endpoint, accessKey, secret,
  forcePathStyle). O clone pode implementar S3 atrás de feature flag; o
  contrato HTTP não muda.
- Coleções `view` podem ter campos `file`; o download resolve o registro
  original via `FindRecordByViewFile`.
- Limite de corpo: 32 MB global; para coleções com arquivos, o limite é
  calculado a partir de `maxSize` e `maxSelect`.

### 2.7 Hooks e runtime JavaScript

#### 2.7.1 Barramento de hooks

O app dispara hooks antes/depois das operações. Nomes Go (equivalentes em
Rust):

| Grupo | Hooks |
|---|---|
| Ciclo | `OnBootstrap`, `OnBootstrapClear`, `OnServe`, `OnTerminate` |
| Registros | `OnRecordCreateRequest`, `OnRecordCreate`, `OnRecordCreateExecute`, `OnRecordAfterCreateSuccess`, `OnRecordAfterCreateError`; equivalentes `Update`, `Delete`; `OnRecordsListRequest`, `OnRecordViewRequest`, `OnRecordEnrich` |
| Auth | `OnRecordAuthRequest`, `OnRecordAuthWithPasswordRequest`, `OnRecordAuthWithOAuth2Request`, `OnRecordAuthWithOTPRequest`, `OnRecordRequestOTPRequest`, `OnRecordRequestPasswordResetRequest`, `OnRecordConfirmPasswordResetRequest`, `OnRecordRequestVerificationRequest`, `OnRecordConfirmVerificationRequest`, `OnRecordRequestEmailChangeRequest`, `OnRecordConfirmEmailChangeRequest`, `OnRecordAuthRefreshRequest` |
| Coleções | `OnCollectionsListRequest`, `OnCollectionViewRequest`, `OnCollectionCreateRequest`, `OnCollectionUpdateRequest`, `OnCollectionDeleteRequest`, `OnCollectionsImportRequest`, `OnCollectionCreate/Update/Delete`, `OnCollectionAfterCreateSuccess/Error`, etc. |
| Settings | `OnSettingsListRequest`, `OnSettingsUpdateRequest`, `OnSettingsReload` |
| Arquivos | `OnFileDownloadRequest`, `OnFileTokenRequest` |
| Realtime | `OnRealtimeConnectRequest`, `OnRealtimeMessageSend`, `OnRealtimeSubscribeRequest` |
| E-mail | `OnMailerSend`, `OnMailerRecordPasswordResetSend`, `OnMailerRecordVerificationSend`, `OnMailerRecordEmailChangeSend`, `OnMailerRecordOTPSend`, `OnMailerRecordAuthAlertSend` |
| Backups | `OnBackupCreate`, `OnBackupRestore` |
| Batch | `OnBatchRequest` |
| Modelos | `OnModelCreate/Update/Delete`, `OnModelAfterCreateSuccess/Error`, etc. |

Semântica:

- `*Request`: na borda HTTP, antes do save; pode inspecionar/modificar o
  payload, encerrar a resposta ou chamar `next()`.
- `OnRecordCreate/Update/Delete`: durante `save/delete`, antes da persistência.
- `*Execute`: dentro da transação.
- `After*Success`: depois do commit (não dentro da transação).
- `After*Error`: em falha; usado para cleanup.
- Tags opcionais por nome/ID de coleção; `*` quando suportado.
- Retornar erro aborta a operação; em `*Request`, o erro vira resposta HTTP.
- Em `*Execute`, o `app` do evento é o app transacional.
- Hooks de resposta (`OnRecordAuthRequest`, `OnFileDownloadRequest`) podem
  alterar o token, headers ou o corpo antes de escrever.

#### 2.7.2 JSVM

O binário oficial registra o plugin JSVM, que carrega hooks de
`pb_hooks/*.pb.js` e `*.pb.ts` (os `.pb.ts` são lidos como JS; não há
transpilação TypeScript) e migrações de `pb_migrations/*.js|*.ts`.

Globais:

| Global | Conteúdo |
|---|---|
| `$app` | app: `findRecordById`, `findFirstRecordByFilter`, `findAllRecords`, `save`, `delete`, `runInTransaction`, `logger`, `settings`, `cron`, `on*` hooks, `newFilesystem`, `db`, `auxDb` |
| `$apis` | helpers de API: `requireRecordAuth`, `requireSuperuserAuth`, `requestInfo`, `recordAuthResponse`, `static`, middlewares |
| `$dbx` | expressões: `exp`, `hashExp`, `not`, `and`, `or`, `in`, `notIn`, `like`, `orLike`, `notLike`, `orNotLike`, `exists`, `notExists`, `between`, `notBetween` |
| `$security` | random, hash, encrypt/decrypt, JWT |
| `$filesystem` | file, system, helpers |
| `$filepath` | path helpers |
| `$os` | getenv, read/write, walkDir, readDir |
| `$http` | HTTP client |
| `$mails` | envio de e-mails |
| `$template` | templates |

Funções globais: `routerAdd(method, path, handler, ...middlewares)`,
`routerUse(...middlewares)`, `cronAdd(id, expr, handler)`, `cronRemove(id)`,
`migrate(up, down)`, `sleep`, `toString`, `toBytes`, `readerToString`,
`unmarshal`, `arrayOf`, `nullString`, `nullInt`, `nullFloat`, `nullBool`,
`nullArray`, `nullObject`.

Construtores: `Record`, `Collection`, `RequestInfo`, `Middleware`, `Timezone`,
`DateTime`, `ValidationError`, `Cookie`, `SubscriptionMessage`.

Exemplo:

```js
/// <reference path="../pb_data/types.d.ts" />
onRecordCreateRequest((e) => {
  e.record.set("slug", e.record.getString("title").toLowerCase().replace(/\s+/g, "-"));
  e.next();
}, "posts");

routerAdd("GET", "/hello/:name", (c) => {
  return c.json(200, { message: "Olá, " + c.pathParam("name") });
});

cronAdd("limpeza", "0 3 * * *", () => {
  $app.logger().info("limpeza diária");
});
```

Configuração:

- `--hooksDir` (padrão `pb_data/../pb_hooks`);
- `--hooksWatch` (padrão `true`; reinicia em mudanças, exceto Windows);
- `--hooksPool` (padrão `15` runtimes pré-aquecidos);
- `--migrationsDir` (padrão `pb_data/../pb_migrations`);
- `--automigrate` (padrão `true` no binário oficial).

O JSVM não é Node: sem npm, sem `require` de pacotes externos; o runtime é
síncrono e isolado por execução. O clone deve impor um timeout por hook
(configurável, padrão conservador) para não travar o processo; isso é uma
melhoria de segurança, não uma mudança de contrato HTTP.

### 2.8 Migrações e CLI

#### 2.8.1 Migrações

- Tabela `_migrations` (`file` PK, `applied` inteiro).
- Migrações de sistema e de app; rodam no boot e via CLI.
- Arquivos: `{timestamp}_{nome}.js` (JSVM) ou `.go` (plugins Go). O clone usa
  `.rs` para migrações nativas e `.js` para as do JSVM. O nome deve ser único e
  ordenável por timestamp.
- Formato JS:

```js
migrate((app) => {
  const collection = new Collection({
    name: "posts",
    type: "base",
    fields: [
      { name: "title", type: "text", required: true },
    ],
    listRule: "",
    viewRule: "",
  });
  app.save(collection);
}, (app) => {
  const collection = app.findCollectionByNameOrId("posts");
  app.delete(collection);
});
```

- `migrate up` aplica pendentes; `migrate down [n]` desfaz as últimas `n`
  (padrão 1); `migrate create <nome>` cria um arquivo vazio; `migrate
  collections` gera uma migração com o snapshot das coleções atuais;
  `migrate history-sync` sincroniza `_migrations` com os arquivos existentes.
- Automigrate: criar/atualizar/apagar coleção via API/painel gera uma migração
  no diretório configurado. Em produção, pode ser desligado.
- Migrações são transacionais; falha não deixa schema parcial.
- Ao importar um `pb_data` existente, as migrações de sistema devem ser
  compatíveis com o schema do v0.40.6; migrações de app existentes precisam
  rodar com o mesmo resultado.

#### 2.8.2 CLI

Comandos do binário oficial:

```text
serve [domain(s)]
superuser upsert <email> <password>
superuser create <email> <password>
superuser update <email> <password>
superuser delete <email>
superuser otp <email>
superuser ips [ip/cidr ...]
migrate up|down|create|collections|history-sync
update
```

Flags globais:

```text
--dir <path>             # pb_data (padrão ./pb_data)
--encryptionEnv <ENV>    # nome da env com chave de 32 chars para settings
--dev                    # logs/SQL no console
--queryTimeout <s>       # timeout de SELECT (padrão 30)
```

Flags de `serve`:

```text
--http <addr>            # padrão 127.0.0.1:8090 sem domínio; 0.0.0.0:80 com domínio
--https <addr>           # padrão 0.0.0.0:443 com domínio; vazio sem TLS
--origins <lista>        # CORS; padrão ["*"]
```

Flags do binário de exemplo (plugins):

```text
--hooksDir, --hooksWatch, --hooksPool
--migrationsDir, --automigrate
--publicDir
```

Comportamento:

- Sem domínio: HTTP em `127.0.0.1:8090`.
- Com um ou mais domínios: HTTP em `0.0.0.0:80` e HTTPS em `0.0.0.0:443`,
  com certificados automáticos (ACME) para os domínios; HTTP redireciona para
  HTTPS.
- `--dir` move o `pb_data`; hooks/migrations/public continuam relativos ao
  diretório de dados/executável conforme a distribuição.
- `--encryptionEnv NOME` lê a env `NOME` (32 chars) e criptografa o valor de
  `_params.settings`; sem a env, o app não consegue ler settings criptografados.
- `superuser upsert` é idempotente por e-mail; `superuser otp` só funciona se
  OTP estiver habilitado em `_superusers`.
- `migrate` e `update` são plugins no binário oficial; o clone deve oferecê-los
  no binário padrão.
- `update` consulta releases e substitui o executável; é opcional e deve ser
  feature-gated em produção.

### 2.9 Painel administrativo (htmx)

O painel substitui a UI SvelteKit em `/_/`, sem build de front-end. Ele usa a
mesma camada de serviço da API; o contrato HTTP é o mesmo.

#### 2.9.1 Rotas do painel

```text
GET  /_/                         dashboard
GET  /_/install                  primeiro superuser (token de instalação)
GET  /_/login                    formulário de login
POST /_/login                    autentica e cria sessão
POST /_/logout                   encerra sessão
GET  /_/collections              lista de coleções
GET  /_/collections/new          escolha de tipo
GET  /_/collections/{id}         editar schema
GET  /_/collections/{id}/records lista de registros
GET  /_/collections/{id}/records/new
GET  /_/collections/{id}/records/{recordId}
GET  /_/logs
GET  /_/logs/{id}
GET  /_/settings
GET  /_/backups
GET  /_/crons
GET  /_/sql
GET  /_/events                   SSE do painel
GET  /_/assets/*                 CSS/htmx/JS mínimo
```

#### 2.9.2 Sessão e CSRF

- Login: e-mail/senha de superuser (`_superusers`), com dummy hash para
  e-mail inexistente e rate limit próprio.
- Sessão: ID aleatório de 32 bytes, armazenado **hasheado** em
  `_panel_sessions` (SQLite), com `created`, `last_seen`, `expires`,
  `csrf_token`, `superuser_id`, `ip`, `user_agent`.
- Cookie: `__Host-pb_panel` quando HTTPS; `HttpOnly`, `Secure` quando HTTPS,
  `SameSite=Strict`, `Path=/`, expiração absoluta de 12 h e idle de 30 min.
- Nenhum token de superuser no cookie/HTML: a cada request o painel carrega o
  registro `_superusers` e injeta o contexto de auth internamente.
- CSRF: token por sessão em `<meta name="csrf-token">` e
  `hx-headers='{"X-CSRF-Token": "..."}'`; todo método não-GET exige
  `X-CSRF-Token` igual ao da sessão. `SameSite=Strict` é defesa adicional.
- Logout rotaciona/invalida a sessão. Login rotaciona o ID (fixation).
- `/_/install` só funciona enquanto não existe superuser; o token é de uso
  único, curto, e o formulário cria o primeiro superuser.

#### 2.9.3 Telas

| Tela | Conteúdo | htmx |
|---|---|---|
| Dashboard | contadores, saúde, últimas coleções/logs | `hx-get` com polling leve |
| Instalação | e-mail/senha do primeiro superuser | form POST |
| Login | e-mail/senha, erro inline | form POST |
| Coleções | tabela, tipo, nº de registros, regras | `hx-get`/`hx-delete` |
| Schema | nome, tipo, campos (add/remove/reorder), opções, índices, regras | partials por campo |
| Registros | tabela paginada, filtro, sort, expand, criar/editar/apagar | `hx-get` na tabela, `hx-push-url` |
| Editor de registro | formulário por tipo, upload, relação, JSON, geo, select | validação inline |
| Logs | tabela, nível, detalhe, stats | filtro e drawer |
| Settings | abas app/SMTP/S3/backups/rate limit/batch/proxy/superuser IPs | PATCH parcial |
| Backups | lista, criar, upload, baixar, restaurar | confirmação + polling |
| Crons | lista, expressão, executar agora | `hx-post` |
| SQL | textarea, resultado, aviso de escrita | submit + erro |
| Realtime | badges "ao vivo" em listas/logs | SSE |

Regras de UI:

- Sem framework JS; htmx 2 embutido (`include_bytes!`), CSS próprio, JS mínimo
  para upload/JSON/geo.
- Navegação com `hx-boost`, `hx-target="#main"`, `hx-push-url="true"`,
  `hx-swap="innerHTML show:window:top"`.
- Formulários usam `hx-post`/`hx-patch` e devolvem o partial com erros 400
  mapeados por campo; foco no primeiro erro; `aria-live="polite"`.
- Destrutivos usam `hx-confirm` e, para ações críticas (restore, SQL de
  escrita), confirmação digitada.
- Tabelas: paginação server-side (30/50/100), ordenação por header, filtro
  persistido na query string; `skipTotal` para listas grandes.
- Schema: adicionar/remover/reordenar campos com botões (sem drag-and-drop
  obrigatório); aviso ao trocar tipo de campo com dados; índice em textarea
  validado no servidor.
- Registros: campos `file` mostram nome e permitem remover; `relation` usa
  select com busca (`hx-get`); `json` usa textarea com validação; `geoPoint`
  usa latitude/longitude; `date` usa `datetime-local` convertido para o
  formato canônico.
- Settings: segredos mascarados; enviar apenas o alterado; botões "testar
  SMTP"/"testar S3"; gerador de client secret Apple.
- Backups: download via rota do painel que gera file token internamente e
  redireciona; nunca expõe token no HTML.
- Logs: detalhe em drawer; copiar request ID; filtro por nível/URL/status.
- Acessibilidade: HTML semântico, labels, foco visível, contraste, navegação
  por teclado, sem dependência de mouse.

#### 2.9.4 SSE do painel

`GET /_/events` autentica pela sessão e reemite fragmentos HTML a partir do
broker interno:

- `/_/events?topic=logs` → `<tr>` novos de log;
- `/_/events?topic=collections/{id}` → linhas de registro;
- `/_/events?topic=backups` → progresso de backup/restore.

O painel pode usar a extensão SSE do htmx (`hx-ext="sse"`, `sse-connect`,
`sse-swap`) ou um `EventSource` mínimo. O servidor do painel não reaproveita o
`/api/realtime` diretamente porque o browser não envia header
`Authorization` no `EventSource`; a sessão por cookie resolve isso.

#### 2.9.5 Segurança do painel

- CSP estrita: `default-src 'self'; script-src 'self'; style-src 'self'
  'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self';
  frame-ancestors 'none'`. htmx configurado com `allowEval:false` e sem
  `hx-on`/JS inline; nenhum CDN.
- Nenhum segredo em HTML, log ou mensagem de erro. Mascarar `***` e tokens
  como `<jwt>…`.
- Sessão e CSRF conforme 2.9.2; `superuserIPs` respeitado.
- SQL console: apenas superuser, CSRF, confirmação para escrita, timeout e
  limite de linhas.
- Backup restore: confirmação, bloqueio de concorrência, validação de zip
  (sem path traversal, sem symlink), restart controlado.
- Uploads: validar MIME/extensão/tamanho; limites de pixel para thumbnails;
  nunca executar conteúdo enviado.
- Logs: redigir `Authorization`, `token`, `password`, `secret`, cookies.

### 2.10 Adaptadores (traits)

| Trait | Padrão | Alternativas |
|---|---|---|
| `Database` | SQLite (`data.db` + `auxiliary.db`) | nenhuma no v1 |
| `Filesystem` | local (`pb_data/storage`) | S3 (feature) |
| `BackupsFilesystem` | local (`pb_data/backups`) | S3 (feature) |
| `Mailer` | SMTP (`lettre`) | fake em teste |
| `Realtime` | broker interno + SSE | nenhum externo no v1 |
| `JsRuntime` | `rquickjs` (feature `js`) | desabilitado no binário mínimo |
| `PasswordHasher` | bcrypt cost 10 | verificar hashes importados |
| `Clock` | sistema | fake em teste |
| `IdGenerator` | 15 chars `[a-z0-9]` | determinístico em teste |
| `TokenSigner` | JWT HS256 | — |

Contratos:

- `Filesystem`: `upload`, `delete`, `exists`, `attributes`, `serve`,
  `list`, `size`, `createThumb`; paths sempre relativos e normalizados.
- `Mailer`: `send(message)`; templates resolvidos no domínio; falha de SMTP
  não derruba a request de auth (loga e segue, quando o fluxo permitir).
- `Realtime`: `subscribe`, `unsubscribe`, `broadcast`, `clients`; a política
  de regra fica no domínio, não no transporte.
- `JsRuntime`: `eval_hook(name, event)`, `eval_migration(up/down)`,
  `register_router`, `register_cron`; timeout e isolamento por execução.
- `Database`: transações explícitas, queries parametrizadas, migrações
  idempotentes, `VACUUM INTO` para backup.

### 2.11 Configuração e defaults

Precedência: flags de CLI > variáveis de ambiente (`PB_*`, opcionais) >
arquivo `pb_config.toml` (opcional) > defaults. O contrato HTTP não muda com a
origem da configuração.

| Item | Default |
|---|---|
| HTTP | `127.0.0.1:8090` |
| HTTPS | vazio (sem TLS) |
| Domínios | nenhum |
| CORS | `*` |
| `pb_data` | `./pb_data` |
| `data.db` / `auxiliary.db` | dentro de `pb_data` |
| storage | `pb_data/storage` |
| backups | `pb_data/backups` |
| hooks | `pb_data/../pb_hooks` |
| migrations | `pb_data/../pb_migrations` |
| public | `pb_public` (relativo ao executável) |
| body limit | 32 MB |
| batch | desabilitado; 50 requests; 3 s; 128 MB |
| paginação | page 1; perPage 30; máx 1000 |
| filtro | 200 expressões; 3500 chars cada |
| sort | 8 campos; 255 chars cada |
| query timeout | 30 s |
| SQL | 1000 linhas; 3 min |
| realtime | idle 5 min; máx 30 min |
| arquivo | 5 MB por arquivo (se `maxSize=0`) |
| thumbnail padrão | `100x100` |
| auth token | 5 dias |
| superuser token | 1 dia |
| reset/e-mail change | 30 min |
| verificação | 1 dia |
| file token | 3 min |
| OTP | 3 min; 8 dígitos |
| MFA | 10 min |
| rate limit | desabilitado (regras padrão acima) |
| logs | 5 dias; IP ligado |
| backup automático | sem cron; manter 3 |
| SMTP | desabilitado; porta 587 |
| S3 | desabilitado |

### 2.12 Erros

Formato:

```json
{
  "status": 400,
  "message": "Failed to create record.",
  "data": {
    "title": {
      "code": "validation_required",
      "message": "Cannot be blank."
    }
  }
}
```

- `status` é o mesmo do HTTP.
- `message` é humano e estável o suficiente para o cliente exibir.
- `data` é sempre objeto; em validação, chaves por campo.
- Códigos comuns: 400 (validação/regra de negócio), 401 (sem token/MFA),
  403 (regra/superuser), 404 (registro/coleção), 413 (corpo), 429 (rate limit),
  500 (interno).
- `401` de MFA tem `{"mfaId":"..."}` no corpo.
- `404` é usado para não revelar existência quando a regra de view falha em
  arquivo protegido.
- Erros internos nunca vazam stack trace; o log tem o detalhe.

### 2.13 Importação de um PocketBase existente

O clone deve aceitar um `pb_data` copiado de um PocketBase v0.40.x:

1. Parar o servidor antigo.
2. Copiar `pb_data` inteiro (bancos, storage, backups).
3. Se as settings estavam criptografadas, usar a mesma `--encryptionEnv`.
4. Rodar `migrate history-sync` e `migrate up`.
5. Verificar contagens, IDs, datas, nomes de arquivos e regras.
6. Rodar o harness de leitura (list/view/expand) e comparar com o original.

Critérios:

- Nenhum ID muda.
- Datas mantêm o formato e o instante.
- Arquivos continuam acessíveis nos mesmos paths e com os mesmos nomes.
- Settings, coleções, regras e índices são preservados.
- Tokens existentes continuam válidos **se** o `tokenKey` e o segredo da
  coleção forem preservados; caso contrário, documentar a invalidação.

### 2.14 Desvios conscientes

| Área | PocketBase | Este projeto | Por quê |
|---|---|---|---|
| Linguagem | Go | Rust | requisito do projeto |
| Painel | SvelteKit embutido | HTML server-side + htmx | sem build de front-end |
| Runtime JS | goja | QuickJS (`rquickjs`) atrás de feature | binário menor/isolamento; API JS equivalente |
| Plugins Go | API Go | API Rust + hooks | não há compatibilidade binária |
| Banco | SQLite | SQLite | mesmo modelo operacional |
| S3 | embutido | feature opcional | binário mínimo |
| OAuth2 | provedores + config genérica | motor genérico + presets | cobre todos os provedores |
| Atualização | `update` oficial | `update` próprio, feature-gated | evita dependência de release alheia |
| UI | textos/assets oficiais | textos próprios | marca e licença |

Cada desvio precisa de teste que prove que o comportamento observável via HTTP
continua igual. O que muda é a implementação, não o contrato.

---

## 3. Estratégia de testes e compatibilidade

### 3.1 Servidor de referência

`docker/compose.yaml` sobe o binário oficial **v0.40.6** por URL fixada; no CI,
confira o SHA-256 publicado antes de executar (não use `latest`):

```yaml
services:
  pocketbase:
    image: alpine:3.22
    command: >
      sh -c "apk add --no-cache wget unzip &&
             wget -qO /tmp/pb.zip https://github.com/pocketbase/pocketbase/releases/download/v0.40.6/pocketbase_0.40.6_linux_amd64.zip &&
             unzip -o /tmp/pb.zip -d /pb &&
             /pb/pocketbase serve --http 0.0.0.0:8090 --dir /pb/pb_data"
    ports: ["8090:8090"]
    volumes: ["./fixtures/pb_data:/pb/pb_data"]
```

O harness (`tests/conformance`) recebe cenários declarativos e executa nos dois
servidores:

```yaml
- name: cria e lista
  steps:
    - method: POST
      path: /api/collections/posts/records
      auth: superuser
      body: { title: "Café", status: "draft" }
    - method: GET
      path: /api/collections/posts/records?filter=status="draft"&sort=-created
      auth: superuser
  normalize: [id, collectionId, created, updated, token]
```

Normalização:

- IDs → `"<id>"` (preservando igualdade entre referências).
- Datas → `"<date>"`.
- Tokens JWT → `"<token>"`.
- Ordem de arrays quando irrelevante → ordenada.
- Chaves de objeto → ordenadas.
- Headers voláteis (`Date`, `Content-Length`, `ETag`) → removidos.

Divergência = falha com request/response dos dois lados no relatório.

### 3.2 SDK JS oficial

O SDK JS do PocketBase não é uma suíte de integração pronta; ele é o **cliente
de referência**. O harness `tests/sdk-js` cobre:

- `pb.health.check()`;
- `pb.collection(name).getList/getFirstListItem/getOne/create/update/delete`;
- `authWithPassword`, `authRefresh`, `authWithOAuth2` (provider fake),
  `requestOTP`/`authWithOTP`, `requestPasswordReset`, `requestVerification`,
  `requestEmailChange`;
- `pb.files.getURL`, upload multipart, `thumb`, `token`;
- `pb.realtime.subscribe`/`unsubscribe` com `PB_CONNECT`, create/update/delete;
- `pb.batch`;
- `pb.settings`, `pb.backups`, `pb.logs`, `pb.crons`, `pb.sql`;
- `pb.collections`.

Cada cenário tem uma versão "referência" e uma "clone"; a comparação é
normalizada. Exclusões vão em `tests/sdk-js/exclusoes.txt` com motivo e onda.

### 3.3 Pirâmide

| Nível | O que cobre |
|---|---|
| unidade | domínio: campos, regras, tokens, validação, parsing de filtro |
| integração | rota → SQLite em memória; transações; migrações; hooks |
| contrato | dourado contra o binário de referência |
| SDK | cenários do SDK JS |
| segurança | matriz de autorização por rota e por regra |
| realtime | SSE com cliente oficial e reconexão |
| carga | k6/oha: CRUD, query, login, SSE com 1k conexões |
| caos | queda do SQLite, do storage, do SMTP; restore de backup |

### 3.4 Relatório de paridade

`docs/PARIDADE.md` com uma linha por funcionalidade e quatro colunas: estado
(✅/🔨/📋), evidência (teste/arquivo), divergência conhecida, onda. O relatório
é gerado a partir dos testes; números escritos à mão envelhecem.

---

## 4. Ondas de entrega

Cada onda é um conjunto de PRs pequenos, com o critério de aceite em negrito.
Tempo é indicativo; o que importa é o critério.

| # | Onda | Entrega | Critério de aceite |
|---|---|---|---|
| 0 | Fundação | workspace, CLI `serve`, config, `/api/health`, instalador, SQLite + migrações de sistema, settings, logs, CI, compose de referência, harness | **health no mesmo formato; instalador cria o primeiro superuser; harness roda no CI; `pb_data` com os dois bancos** |
| 1 | Coleções e schema | `_collections`, CRUD de coleções, scaffolds, import, truncate, dry-run-view, todos os tipos de campo, índices, regras, auth/view options | **criar/editar/apagar coleções base/auth/view via API com dourados; schema físico e JSON equivalentes; import/export ida e volta** |
| 2 | Registros e consultas | CRUD de records, list/view/create/update/delete, filtro, sort, expand, fields, validação, transações, batch | **matriz de query e regras passa; batch atômico; dourados de todos os tipos; `@collection` e modificadores** |
| 3 | Autenticação | password, refresh, OTP, MFA, OAuth2, reset, verificação, troca de e-mail, impersonate, auth alerts, tokens | **cenários de auth do SDK JS verdes; MFA com dois métodos; tokenKey invalida tokens; anti-enumeração** |
| 4 | Arquivos | upload/download, nomes, thumbs, protegidos, file token, S3 (fake/minio), limites, cleanup | **upload/download/thumb/protegido verdes; nomes normalizados; S3 opcional; body limit correto** |
| 5 | Realtime | broker, SSE, `PB_CONNECT`, tópicos, options, ACL, subscribe, reconexão | **cliente oficial recebe create/update/delete com regras; idle/max timeout; sem replay** |
| 6 | Operação | settings, logs, backups, crons, SQL, rate limit, trusted proxy, audit log | **APIs douradas; backup/restore de um `pb_data` real; rate limit 429; logs filtráveis** |
| 7 | Extensibilidade | hooks Rust, JSVM, migrações, CLI `migrate`, automigrate, `superuser` | **exemplos de hooks rodam; migração up/down idempotente; automigrate gera arquivo; CLI completa** |
| 8 | Painel htmx | todas as telas da §2.9, sessão/CSRF, SSE do painel, e2e | **checklist do painel completo; nenhuma chave no HTML; e2e verde; acessibilidade básica** |
| 9 | Produção | TLS/ACME, proxy, hardening, benchmarks, importação, docs, relatório | **paridade ≥95%; importação de um app real; carga sem regressão; runbook** |

Ordem obrigatória: 0 → 1 → 2 → 3. As ondas 4–6 podem ser paralelizadas por
pessoas diferentes depois da 2. A 7 começa depois da 3 e da 6. O painel (8)
começa na 1 e cresce junto, mas só fecha depois da 6. A 9 não começa antes de a
8 estar verde.

---

## 5. Regras de engenharia

### 5.1 Repositório e código

- Um crate, uma responsabilidade; dependências apontam para dentro
  (`base-http` → `base-core`, nunca o contrário).
- Erros com `thiserror` no domínio e `ApiError`/`Problem` na borda; `unwrap`
  proibido fora de teste.
- Nada de SQL concatenado; tudo por parâmetro. Nome de coleção/campo validado
  antes de virar identificador; identifiers sempre entre `[[...]]`/aspas.
- Toda escrita é transacional. Batch e migrações são all-or-nothing.
- `unsafe` proibido, salvo em FFI isolada e documentada.
- Feature flags para S3, JSVM, SMTP, TLS/ACME e `update`, para o binário mínimo
  continuar pequeno.
- Nenhum `unwrap` em handler; nenhum panic em request; panic recover devolve
  500 JSON.

### 5.2 Qualidade

- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D
  warnings`, `cargo test --workspace` no CI, nos três SOs.
- Cobertura mínima de 85% no domínio e 75% no servidor (medida, não aspirada).
- Nenhum PR sem teste de contrato para a funcionalidade nova.
- Benchmarks de regressão para query, batch e SSE; orçamento: não piorar 10%
  sem justificativa.
- Migrações testadas em banco vazio e em banco com dados.

### 5.3 Segurança

- Modelo de ameaças em `docs/SEGURANCA.md`: JWT, tokenKey, bcrypt, OAuth2
  state/PKCE, SSRF em `userInfoURL`, path traversal em arquivos/backups, zip
  slip, XSS em registros/logs, CSRF, session fixation, ReDoS em filtros,
  injeção no SQL console, enumeração de e-mail, timing de login, vazamento em
  logs.
- Segredos nunca em log, resposta, HTML ou mensagem de erro. Mascarar chaves
  como `***` e tokens como `<jwt>…`.
- Rate limit por IP, limite de corpo, limite de filtro/sort, timeout de query.
- `superuserIPs` e `trustedProxy` implementados no middleware, não no handler.
- Dependabot/auditoria (`cargo audit`) no CI; política de resposta a
  vulnerabilidade.
- Backups: validar entradas do zip, recusar `..`/symlink, restaurar em
  diretório temporário e trocar atomicamente.

### 5.4 Licença e marca

- Licença do projeto: **MIT**, alinhada ao PocketBase, com NOTICE.
- O dashboard oficial está no mesmo repositório MIT, mas é uma obra visual
  distinta: **não copie** CSS, textos, ícones, fontes ou assets. O painel é
  reimplementação limpa.
- O SDK JS oficial é MIT e pode ser usado como dependência de teste; preserve
  o copyright.
- Sem código copiado; se houver trecho derivado, marque o arquivo e o NOTICE.
- Nome próprio; README diz "compatível com a API do PocketBase" e não usa os
  logos do PocketBase sem permissão.

---

## 6. Decisões abertas (resolver na onda 0, registrar em ADR)

| # | Decisão | Opções | Recomendação |
|---|---|---|---|
| 1 | Driver SQLite | `sqlx` / `rusqlite` | `sqlx` (async, pool, migrações) com SQL explícito; `rusqlite` se precisar de controle fino |
| 2 | Runtime JS | `rquickjs` / `boa_engine` | `rquickjs` atrás de feature; isolar por execução e timeout |
| 3 | Templates | `maud` / `askama` | `maud`: componentes tipados, partials e escape por construção |
| 4 | Sessão do painel | SQLite / memória | SQLite (`_panel_sessions`) para sobreviver a restart |
| 5 | SSE do painel | broker interno / polling | broker interno + `/_/events`; polling só como fallback |
| 6 | OAuth2 | lib por provedor / motor genérico | motor genérico + presets; Apple com secret JWT |
| 7 | Hash de senha | bcrypt / Argon2id | bcrypt cost 10 para importar hashes; Argon2id opcional quebraria compatibilidade |
| 8 | S3 | `aws-sdk-s3` / `object_store` / `rust-s3` | `aws-sdk-s3` feature-gated; fake em teste |
| 9 | E-mail | `lettre` / SMTP manual | `lettre` com TLS/STARTTLS e timeout |
| 10 | JWT | `jsonwebtoken` / `josekit` | `jsonwebtoken` HS256, validação de `exp` e `type` |
| 11 | Nome do projeto | — | nome próprio, curto, sem "PocketBase" no produto; ADR com critérios de marca |

---

## 7. Anexos

### A. Checklist final de paridade

- [ ] `/api/health` no mesmo formato (versão pode diferir).
- [ ] Settings, SMTP, S3, backups, rate limits, batch e superuser IPs.
- [ ] CRUD de coleções base/auth/view, scaffolds, import, truncate,
      dry-run-view, índices.
- [ ] Todos os tipos de campo e validações; `maxSelect` string/array.
- [ ] CRUD de records, filtro, sort, expand, fields, paginação, skipTotal.
- [ ] Regras `null`/`""`/expressão; `@request`, `@collection`, macros,
      funções, modificadores.
- [ ] Auth: password, OTP, OAuth2, MFA, refresh, reset, verificação, troca de
      e-mail, impersonate, alerts.
- [ ] JWT HS256, tokenKey, durações, file token.
- [ ] Upload/download/thumb/protegido, S3 opcional, cleanup.
- [ ] SSE, `PB_CONNECT`, tópicos, options, ACL, subscribe.
- [ ] Batch atômico com timeout e limite.
- [ ] Logs, stats, truncate, audit log.
- [ ] Backups create/upload/download/delete/restore, automático, S3.
- [ ] Crons e jobs internos.
- [ ] SQL console com limite e transação.
- [ ] Hooks Rust e JSVM; migrações e automigrate; CLI.
- [ ] Painel: instalação, login, schema, dados, logs, settings, backups,
      crons, SQL, SSE.
- [ ] Nenhum segredo em HTML/log; CSRF, sessão, rate limit, body limit.
- [ ] Importação de um `pb_data` real.
- [ ] Relatório de paridade e cenários do SDK JS verdes.

### B. Referência rápida de regras e filtros

```text
null                      -> somente superuser
""                        -> público
expressão                 -> filtro por registro/request

=  !=  >  >=  <  <=
~  !~  ?=  ?!=  ?>  ?>=  ?<  ?<=  ?~  ?!~
&&  ||  !  ( )

@request.auth.*  @request.body.*  @request.query.*  @request.headers.*
@request.method  @request.context
@collection.<coleção>.<campo>
@now @yesterday @tomorrow @todayStart @todayEnd
@monthStart @monthEnd @yearStart @yearEnd
@second @minute @hour @day @month @weekday @year
geoDistance(lonA, latA, lonB, latB)
strftime(format, valor, ...)
:each :isset :length :lower :changed
```

### C. Vocabulário

| Termo | Significado |
|---|---|
| Coleção | Tabela + metadados + regras; tipos `base`, `auth`, `view` |
| Campo | Coluna tipada de uma coleção |
| Regra | Expressão que autoriza/filtra uma operação |
| Superuser | Registro da coleção `_superusers`; ignora regras |
| `tokenKey` | Segredo por registro auth; compõe a chave do JWT |
| `authRule` | Regra extra após autenticar |
| `manageRule` | Permissão administrativa sobre registros auth |
| `expand` | Expansão de relações na resposta |
| `fields` | Projeção do JSON de resposta |
| SSE | Server-Sent Events, usado pelo realtime |
| `PB_CONNECT` | Evento inicial do SSE com o `clientId` |
| Batch | Múltiplas operações de records em uma transação |
| JSVM | Runtime JavaScript de hooks/migrações |
| Backup | Zip do `pb_data` para restauração |
| `pb_data` | Diretório de dados do app |

### D. Primeiros arquivos a escrever

```text
Cargo.toml
crates/base-core/src/lib.rs           Collection, Field, Record, Rule, Token
crates/base-storage/src/lib.rs        trait Database
crates/base-storage/src/sqlite.rs     data.db, auxiliary.db, pragmas, migrações
crates/base-http/src/lib.rs           Router, middlewares, /api/health
crates/base-auth/src/lib.rs           bcrypt, JWT, auth flows
crates/base-files/src/lib.rs          storage local, thumbs, file token
crates/base-realtime/src/lib.rs       broker, SSE, tópicos
crates/base-jobs/src/lib.rs           crons, backups, logs, SQL
crates/base-hooks/src/lib.rs          hook bus, JSVM
crates/base-migrations/src/lib.rs     runner, snapshots
crates/base-server/src/main.rs        composição e lifecycle
crates/base-cli/src/main.rs           serve, superuser, migrate, doctor
crates/base-admin/src/lib.rs          layout, login, tabela, formulário
tests/conformance/src/lib.rs          harness de comparação
tests/sdk-js/                          cenários do SDK
tests/panel/                           e2e Playwright
docker/compose.yaml                    binário de referência v0.40.6
docs/ARQUITETURA.md                    ADRs e diagrama
docs/PARIDADE.md                       tabela gerada pelos testes
```

### E. Definição de pronto (por PR)

1. teste de contrato escrito antes;
2. código com erros tipados, sem `unwrap`, sem SQL interpolado;
3. `fmt`, `clippy`, `test` e conformance verdes;
4. documentação de usuário e de paridade atualizadas;
5. migração de banco (se houver schema novo) idempotente;
6. log estruturado e auditoria quando a operação é sensível;
7. nenhum segredo em log/HTML;
8. PR descreve o comportamento observável, não só o código.

### F. O que não fazer

- Não copie o dashboard Svelte, seus textos, CSS, ícones ou fontes.
- Não use `localStorage` para token de superuser no painel.
- Não coloque token, senha ou segredo em HTML, log ou mensagem de erro.
- Não use `SELECT *` sem limite em listas; não concatene SQL.
- Não use `unsafe` fora de FFI isolada e documentada.
- Não invente campos `created`/`updated` em coleções criadas pela API.
- Não trate `null` e `""` como a mesma regra.
- Não ignore datas com espaço (`"YYYY-MM-DD HH:MM:SS.sssZ"`).
- Não use 201 para create/update; o PocketBase responde 200.
- Não use `Bearer` obrigatório; o header aceita o token puro.
- Não presuma replay no SSE; não há histórico.
- Não implemente batch como operações independentes; é tudo ou nada.
- Não exponha `/api/sql` ou `/api/backups` sem superuser.
- Não deixe o painel sem CSRF/sessão/`superuserIPs`.
