# Prompt mestre — Parse Server em Rust + htmx

Documento de especificação e prompt para reconstruir o
[Parse Platform](https://github.com/orgs/parse-community/repositories) — com foco
no **Parse Server** — como um **monorepo em Rust**, com painel administrativo em
**HTML no servidor + htmx**.

Este documento é independente do Forja: o alvo aqui é a API e o comportamento do
Parse Server, não o do PocketBase.

> **Como usar**
>
> 1. Copie o bloco da seção 1 e entregue a um agente de código como prompt
>    inicial.
> 2. A seção 2 em diante é a especificação de referência que o agente deve
>    consultar durante a execução (pode anexar o arquivo inteiro ao prompt).
> 3. Cada onda da seção 4 tem critério de aceite verificável. Não avance de onda
>    sem ele.
>
> O que este documento **não** é: uma cópia de código do Parse Server. É um
> contrato de comportamento escrito a partir da documentação pública, do
> protocolo publicado e de observação black-box do servidor de referência.

Fatos de configuração, rotas e defaults foram conferidos contra o
`parse-server` **9.10.7-alpha.1** (branch `master`) em **2026-10-23**. O Parse
muda rápido: antes de implementar uma onda, reconfira em
`src/Options/Definitions.js` e `src/Routers/`.

---

## 1. O prompt

```text
Você é engenheiro sênior de backend, banco de dados e front-end server-side.
Sua missão é construir, neste repositório, um monorepo em Rust que seja um
substituto compatível com o Parse Platform — principalmente o Parse Server —
e um painel administrativo em HTML gerado no servidor com htmx, equivalente ao
Parse Dashboard. Trabalhe como se fosse um time de produto: fatias verticais
pequenas, testes, documentação e medição de compatibilidade em cada entrega.

CONTEXTO
O Parse Platform é um backend open source (Apache-2.0) com:
- Parse Server (Node.js/Express + MongoDB/PostgreSQL), API REST em /parse;
- Parse Dashboard (React), painel que fala com um ou mais servidores;
- SDKs oficiais (JS, Swift, Android/Kotlin, .NET, Flutter, PHP, Arduino...);
- servidor de LiveQuery (WebSocket), GraphQL gerado a partir do schema;
- Cloud Code em JavaScript (funções, triggers, jobs, validadores);
- adaptadores de arquivos (FS, S3, GCS), push (APNs, FCM), e-mail, cache,
  Pub/Sub, WebSocket e banco.
O alvo NÃO é copiar o código: é reproduzir o comportamento observável.

OBJETIVO MENSURÁVEL
Um cliente que hoje aponta para o Parse Server deve apontar para o seu binário
mudando apenas a URL, e continuar funcionando. A definição operacional de
"compatível" é, em ordem de prioridade:
1. a suíte de integração do Parse-SDK-JS passa contra o seu servidor (com uma
   lista explícita de exclusões justificadas);
2. o mesmo request, enviado ao Parse Server de referência e ao seu servidor,
   devolve o mesmo JSON normalizado (ignorando objectId, datas, tokens e
   ordem de arrays quando irrelevante);
3. os exemplos dos guias oficiais (REST, Cloud Code, GraphQL, LiveQuery) rodam
   sem alteração;
4. o painel cobre o fluxo que o Parse Dashboard cobre: login, navegar/editar
   dados, editar schema, push, jobs, logs e GraphQL.

REGRAS INEGOCIÁVEIS
- Rust stable, edition 2024, workspace com crates pequenos e dependências
  explícitas. Sem JavaScript no build do painel: HTML no servidor (maud) +
  htmx embutido via include_bytes!.
- SQLite é o banco padrão (um arquivo por projeto, WAL). PostgreSQL é o alvo de
  produção. MongoDB não é reimplementado; a importação de um MongoDB existente é
  um comando de migração.
- Compatibilidade de contrato: caminhos, headers, corpos, códigos de erro,
  semântica de ACL/CLP e formatos de data/ponteiro/arquivo seguem o Parse.
- Segurança primeiro: master key nunca vai ao navegador; painel usa sessão
  própria em cookie httpOnly + SameSite=Strict + CSRF; toda rota tem teste de
  autorização anônimo/usuário/role/master.
- Não copie código do parse-server nem do parse-dashboard. Consulte a
  documentação pública, o protocolo publicado e o comportamento do servidor de
  referência rodando em Docker. Se usar qualquer trecho derivado, registre em
  NOTICE e mantenha a licença Apache-2.0.
- "Parse" é marca da Parse Platform. O projeto tem nome próprio e se declara
  "compatível com a API do Parse Platform", sem sugerir afiliação.
- Código e identificadores em inglês; documentação e mensagens de produto em
  português. Commits convencionais, um assunto por PR.

ARQUITETURA OBRIGATÓRIA (detalhe na seção 2)
crates/
  parse-core        domínio puro: Object, Value, ACL, CLP, Schema, Query, erro
  parse-storage     trait StorageAdapter + implementações SQLite e Postgres
  parse-http        axum: REST /parse, headers, middlewares, batch
  parse-auth        senha, sessões, OAuth, MFA, políticas de conta
  parse-cloud       runtime JS (rquickjs) + triggers, funções, jobs, hooks
  parse-graphql     schema gerado + Relay + playground
  parse-livequery   WebSocket, matching, Pub/Sub, ACL por evento
  parse-files       adaptadores FS, S3, GCS + upload/download
  parse-push        APNs (HTTP/2 + JWT) e FCM (HTTP v1) + _PushStatus
  parse-email       SMTP e adaptador HTTP genérico + templates
  parse-server      composição: config, lifecycle, rotas, migrações
  parse-cli         binário: serve, migrate, user, schema, doctor
  parse-admin       views maud + CSS + htmx + telas do painel
tests/
  conformance/      harness que compara seu servidor com o de referência
  sdk-js/           execução da suíte oficial do SDK JS
  livequery/        cliente oficial JS contra o seu servidor
docker/             compose com Mongo + Parse Server de referência
docs/               ADRs, paridade, guia, runbook

FLUXO DE TRABALHO
1. Antes de codar uma onda, escreva o teste de contrato que ela precisa passar
   (request/response dourado ou teste do SDK).
2. Implemente a menor fatia vertical que faz o teste passar, do banco à rota.
3. Rode: cargo fmt --all --check; cargo clippy --workspace --all-targets -- -D
   warnings; cargo test --workspace; e o harness de conformidade.
4. Atualize docs/PARIDADE.md (funcionalidade, estado, evidência) e o CHANGELOG.
5. Só então abra o PR. PR sem teste de contrato é rejeitado.
6. Ao terminar cada onda, rode a suíte do SDK JS e registre a taxa de aprovação
   e as exclusões. Regressão de compatibilidade bloqueia a onda seguinte.

ENTREGÁVEIS POR ONDA
- código + testes + migração de banco + documentação + evidência de paridade;
- um relatório curto em docs/relatorios/onda-N.md com: o que passou, o que não
  passou, por quê, e o próximo passo.

COMECE ASSIM (onda 0)
- crie o workspace, o binário parse-cli com `serve`, o endpoint /parse/health,
  /parse/serverInfo, a configuração por CLI/env/arquivo e o CI;
- suba o docker-compose com o Parse Server de referência e o Mongo;
- escreva o harness que envia o mesmo request aos dois e compara JSON;
- só considere a onda 0 pronta quando /parse/health e /parse/serverInfo
  estiverem no mesmo formato do de referência (a versão pode diferir) e o
  harness rodar no CI.

NÃO PARE NO CRUD. Um clone sem Cloud Code, LiveQuery, arquivos, push e painel
não é um substituto do Parse Server. A ordem e os critérios estão na seção 4.
```

---

## 2. Especificação de referência

### 2.1 Mapa dos repositórios da organização

| Repositório | Licença | Papel | Onde entra no monorepo |
|---|---|---|---|
| `parse-server` | Apache-2.0 | Servidor REST/GraphQL/LiveQuery, Cloud Code, adaptadores | `parse-*` (núcleo da obra) |
| `parse-dashboard` | licença própria (Parse, LLC), não Apache | Painel de administração (React) | `parse-admin` (htmx), reimplementação limpa |
| `Parse-SDK-JS` | Apache-2.0 | Cliente JS e suíte de integração | `tests/sdk-js/` (oráculo) |
| `Parse-Swift`, `Parse-SDK-Android`, `Parse-SDK-dotNET`, `Parse-SDK-Flutter`, `parse-php-sdk`, `Parse-SDK-Arduino` | variadas | Clientes oficiais | validação manual/best-effort |
| `parse-server-push-adapter` | MIT | APNs + FCM | `parse-push` |
| `parse-server-s3-adapter`, `parse-server-gcs-adapter`, `parse-server-fs-adapter` | ver LICENSE | Armazenamento de arquivos | `parse-files` |
| `parse-server-api-mail-adapter`, `parse-server-simple-mailgun-adapter` | MIT | E-mail transacional | `parse-email` |
| `parse-server-sqs-mq-adapter` | Apache-2.0 | Fila entre instâncias | `parse-server` (feature opcional) |
| `parse-server-conformance-tests` | ver LICENSE | Contrato de adaptadores | `tests/conformance/adapters` |
| `benchmark` | Apache-2.0 | Carga contínua | `benches/` |
| `docs` | ver LICENSE | Guias oficiais | fonte de verdade da linguagem |

Os repositórios de SDKs nativos, Bolts, ParseUI e utilitários de Facebook/Twitter
não são reimplementados. O contrato é o servidor: se os SDKs oficiais conversam
com ele, o resto é consequência.

### 2.2 Princípios de arquitetura

1. **Domínio sem I/O.** `parse-core` não conhece axum, SQL, WebSocket nem JS.
   Tudo que é regra (ACL, CLP, validação, query, schema) é testável sem rede.
2. **Um adaptador por preocupação.** Banco, arquivos, push, e-mail, cache,
   Pub/Sub e WebSocket são traits. O servidor compõe; a configuração escolhe.
3. **A rota é fina.** Handler traduz HTTP → comando de domínio → resposta.
   Nenhuma regra de negócio mora no handler.
4. **Compatibilidade é teste.** Cada rota tem pelo menos um caso dourado
   (fixture JSON) e um caso de autorização.
5. **Tudo é observável.** Toda operação gera log estruturado com requestId,
   appId, classe, operação, usuário (id, nunca token) e duração.
6. **Falha alto e cedo na configuração.** Chave ausente, banco inacessível,
   cloud code inválido: o processo não sobe e diz por quê.

### 2.3 Modelo de dados

#### Classes de sistema

| Classe | Campos próprios (além de objectId, createdAt, updatedAt, ACL) |
|---|---|
| `_User` | `username`, `password` (interno: `_hashed_password`), `email`, `emailVerified`, `authData`, tokens internos (`_email_verify_token`, `_perishable_token`, `_password_history`, `_failed_login_count`, `_account_lockout_expires_at`) |
| `_Role` | `name`, `users` (Relation `_User`), `roles` (Relation `_Role`) |
| `_Session` | `user` (Pointer `_User`), `installationId`, `sessionToken`, `expiresAt`, `createdWith` |
| `_Installation` | `installationId`, `deviceToken`, `channels`, `deviceType`, `pushType`, `GCMSenderId`, `timeZone`, `localeIdentifier`, `badge`, `appVersion`, `appName`, `appIdentifier`, `parseVersion` |
| `_Product` | `productIdentifier`, `download`, `downloadName`, `icon`, `order`, `title`, `subtitle` |
| `_PushStatus` | `pushTime`, `source`, `query`, `payload`, `title`, `expiry`, `expiration_interval`, `status`, `numSent`, `numFailed`, `pushHash`, `errorMessage`, `sentPerType`, `failedPerType`, `sentPerUTCOffset`, `failedPerUTCOffset`, `count` |
| `_JobStatus` | `jobName`, `source`, `status`, `message`, `params`, `finishedAt` |
| `_JobSchedule` | `jobName`, `description`, `params`, `startAfter`, `daysOfWeek`, `timeOfDay`, `lastRun`, `repeatMinutes` |
| `_Hooks` | funções, triggers e validadores registrados por REST |
| `_Audience` | segmentos de push salvos |
| `_GlobalConfig` | parâmetros de configuração em runtime |
| `_GraphQLConfig` | configuração do schema GraphQL |
| `_Idempotency` | `reqId` (chave) e `expire` |
| `_SCHEMA` | `className`, `schema` (JSON), `isParseClass` |

Campos internos de `_User` nunca aparecem em resposta, filtro, ordenação ou
GraphQL. `password` nunca é lido de volta; `_hashed_password` nunca é exposto.

#### Tipos de campo

`String`, `Number`, `Boolean`, `Date`, `Object`, `Array`, `Bytes`, `Pointer`,
`Relation`, `File`, `GeoPoint`, `Polygon`, `ACL`.

Codificação JSON:

| Tipo | Representação |
|---|---|
| Date | `{"__type":"Date","iso":"2026-10-23T12:00:00.000Z"}` |
| Pointer | `{"__type":"Pointer","className":"Jogo","objectId":"abc123"}` |
| Relation | `{"__type":"Relation","className":"Comentario"}` |
| File | `{"__type":"File","name":"...","url":"https://..."}` |
| GeoPoint | `{"__type":"GeoPoint","latitude":-23.5,"longitude":-46.6}` |
| Polygon | `{"__type":"Polygon","coordinates":[[lat,lng],...]}` |
| Bytes | `{"__type":"Bytes","base64":"..."}` |
| ACL | objeto `{"*":{"read":true},"role:admin":{"read":true,"write":true},"<userId>":{"read":true,"write":true}}` |

#### Schema e permissões

O schema de cada classe fica em `_SCHEMA.schema`:

```json
{
  "className": "Jogo",
  "fields": {
    "titulo": { "type": "String", "required": true, "defaultValue": "Sem título" },
    "autor":  { "type": "Pointer", "targetClass": "_User" },
    "tags":   { "type": "Array" },
    "capa":   { "type": "File" }
  },
  "classLevelPermissions": {
    "find":   { "*": true },
    "get":    { "*": true },
    "count":  { "*": true },
    "create": { "requiresAuthentication": true },
    "update": { "role:admin": true },
    "delete": { "role:admin": true },
    "addField": { "role:admin": true },
    "protectedFields": { "*": ["email", "authData"] }
  },
  "indexes": { "titulo_idx": { "titulo": 1 } }
}
```

`indexes` é um objeto `{ nomeDoÍndice: { campo: 1 | -1 } }` (formato de chave do
Mongo). Unicidade de `username`, e-mail, nome de role e chave de idempotência é
garantida por índices internos, não por uma flag no schema.

Chaves de CLP: `find`, `count`, `get`, `create`, `update`, `delete`, `addField`,
`protectedFields`, `readUserFields`, `writeUserFields`. O valor de cada operação
é um mapa de `"*"`, `"role:<nome>"` ou `"<userId>"` para booleano (a chave
especial `requiresAuthentication` também é booleana); `protectedFields`,
`readUserFields` e `writeUserFields` mapeiam para listas de campos.

Regras de avaliação, em ordem: master key (ignora ACL/CLP, com auditoria) →
CLP da operação → ACL do objeto → `protectedFields` (remove campos na leitura) →
campos internos. Sem ACL explícita, o objeto é público (o SDK é quem costuma
gravar a ACL do criador).

#### Armazenamento

| Conceito | SQLite | PostgreSQL |
|---|---|---|
| Objeto | tabela por classe, coluna `data` JSON + colunas geradas/indexáveis | tabela por classe, `data jsonb` + colunas geradas |
| `objectId` | `TEXT PRIMARY KEY` (10 chars `[A-Za-z0-9]`) | idem |
| Datas | ISO-8601 UTC em TEXT + índice | `timestamptz` derivado do jsonb ou coluna própria |
| Ponteiro | objeto JSON; coluna `_p_<campo>` com o id para índice | idem |
| Relação | tabela `<classe>_<campo>_join` (`owningId`, `relatedId`) | idem |
| ACL | JSON na coluna `acl` + índice GIN (PG) / json_extract (SQLite) | idem |
| Busca textual | FTS5 | `tsvector` |
| Geo | R*Tree (extensão) ou cálculo Haversine em Rust sobre candidatos | PostGIS opcional; Haversine como fallback |
| Transação | `BEGIN IMMEDIATE` (escritor único, WAL) | transação normal |
| Batch | transação quando `transaction: true` | transação quando `transaction: true` |

Regras: `objectId` gerado no servidor (tamanho configurável, 10 por padrão);
`createdAt`/`updatedAt` em UTC com precisão de milissegundos; criar **classe**
pelo cliente exige `allowClientClassCreation` (padrão `false`); criar **campo**
novo em classe existente exige permissão `addField` no CLP; migração de schema é
idempotente e registrada em `_SCHEMA`.

#### Consultas

Parâmetros REST: `where`, `order`, `limit` (padrão 100; teto definido por
`maxLimit`, que é ilimitado por padrão), `skip`,
`keys`, `include` (cadeia de ponteiros como `autor.time.nome`; a profundidade
máxima vem de `requestComplexity.includeDepth`, ilimitada por padrão),
`count=1`, `readPreference` (aceito e ignorado).

Operadores de `where`:

| Operador | Semântica |
|---|---|
| igualdade | igualdade profunda em JSON |
| `$ne`, `$lt`, `$lte`, `$gt`, `$gte` | comparação tipada (número, string, data) |
| `$in`, `$nin` | pertinência em lista |
| `$exists` | campo presente/ausente |
| `$regex`, `$options` | regex com timeout e limite de complexidade |
| `$all` | array contém todos |
| `$elemMatch` | ao menos um elemento casa com o subfiltro |
| `$size` | tamanho do array |
| `$nearSphere`, `$maxDistance` | distância esférica; `$maxDistance` em radianos |
| `$geoWithin`, `$box`, `$polygon` | polígono/retângulo |
| `$relatedTo` | relação `{object, key}` |
| `$inQuery`, `$notInQuery`, `$select`, `$dontSelect` | subconsultas |
| `$or`, `$and`, `$nor` | composição |
| `$text` | busca textual (FTS) |

`order`: `campo`, `-campo`, múltiplos separados por vírgula; `include`: cadeia
de ponteiros (`autor.time.nome`), com o limite de profundidade vindo de
`requestComplexity.includeDepth` (ilimitado por padrão); `keys`: projeção;
`count=1` devolve `count`.

Agregação (`GET /parse/aggregate/:classe`, master key) aceita o subconjunto:
`$match`, `$project`, `$group` (`$sum`, `$avg`, `$min`, `$max`, `$count`,
`$addToSet`, `$push`, `$first`, `$last`), `$sort`, `$limit`, `$skip`, `$unwind`,
`$lookup` (relação/ponteiro), `$addFields`, `$count`, `$facet` (opcional).
Qualquer outro estágio: erro `600` com a lista de estágios suportados.

### 2.4 API REST

Base: `{publicServerURL}{mountPath}` — padrão `http://host:1337/parse`.

#### Headers

| Header | Uso |
|---|---|
| `X-Parse-Application-Id` | obrigatório em toda requisição (exceto `/health`) |
| `X-Parse-Master-Key` | acesso administrativo (ignora ACL/CLP); o mesmo header transporta a read-only master key, que só lê |
| `X-Parse-Maintenance-Key` | campos internos e migrações (IP allowlist) |
| `X-Parse-REST-API-Key`, `X-Parse-Javascript-Key`, `X-Parse-Client-Key`, `X-Parse-Windows-Key` (também usada pelo SDK .NET) | opcionais, se configuradas |
| `X-Parse-Session-Token` | sessão do usuário |
| `X-Parse-Installation-Id` | instalação do dispositivo |
| `X-Parse-Cloud-Context` | JSON que chega em `request.context` nos triggers |
| `X-Parse-Request-Id` | idempotência de escrita |
| `X-Parse-Upload-Mode`, `X-Parse-File-Tags`, `X-Parse-File-Metadata`, `X-Parse-File-Directory` | upload de arquivos |
| `X-Parse-Job-Status-Id`, `X-Parse-Push-Status-Id` | resposta de jobs/push |

#### Rotas

| Método | Rota | Observações |
|---|---|---|
| GET | `/health` | `{"status":"ok"}`; estados `initialized`, `starting`, `ok`, `error` |
| GET | `/serverInfo` | master key; `{features, parseServerVersion}` |
| GET | `/security` | relatório de checagem de segurança (master key; requer `security.enableCheck`) |
| GET | `/schemas`, `/schemas/:className` | schema |
| POST | `/schemas`, `/schemas/:className` | criar classe/campo |
| PUT | `/schemas/:className` | atualizar campos/CLP/índices |
| DELETE | `/schemas/:className` | apagar classe |
| GET/POST | `/classes/:className` | listar (query string) / criar |
| GET/PUT/DELETE | `/classes/:className/:objectId` | ler / atualizar parcial / apagar |
| POST | `/batch` | sub-requests; `transaction: true` torna o lote atômico |
| GET | `/aggregate/:className` | master key; pipeline em `pipeline` (JSON) |
| DELETE | `/purge/:className` | apagar todos os objetos (master) |
| GET/POST | `/users` | listar/criar (signup) |
| GET | `/users/me` | usuário da sessão |
| GET/PUT/DELETE | `/users/:objectId` | ler/atualizar/apagar |
| GET/POST | `/login` | login por username/email |
| POST | `/loginAs` | master key, personificação |
| POST | `/logout` | invalida sessão |
| POST | `/requestPasswordReset` | envia e-mail |
| POST | `/verificationEmailRequest` | reenvia verificação |
| GET/POST | `/verifyPassword` | valida token de reset |
| POST | `/challenge` | MFA |
| POST | `/upgradeToRevocableSession` | converte sessão antiga |
| GET | `/sessions/me` | sessão atual |
| GET/POST | `/sessions` | listar/criar sessão |
| GET/PUT/DELETE | `/sessions/:objectId` | ler/atualizar/apagar |
| GET/POST | `/installations` | listar/criar |
| GET/PUT/DELETE | `/installations/:objectId` | ler/atualizar/apagar |
| GET/POST | `/roles` | listar/criar |
| GET/PUT/DELETE | `/roles/:objectId` | ler/atualizar/apagar |
| POST | `/files/:filename` | upload cru (appId no header); aceitar também `/files/:appId/:filename` legado |
| GET | `/files/:appId/:filename` | download com Range e ETag |
| GET | `/files/:appId/metadata/:filename` | metadados |
| DELETE | `/files/:appId/:filename` | apagar arquivo (master key) |
| POST | `/functions/:functionName` | Cloud Code; `{params}` → `{result}` |
| POST | `/jobs/:jobName` | inicia job; header `X-Parse-Job-Status-Id` |
| POST | `/jobs` | idem, com `jobName` no corpo (compatibilidade) |
| GET | `/cloud_code/jobs`, `/cloud_code/jobs/data` | agendamentos |
| POST/PUT/DELETE | `/cloud_code/jobs[/:objectId]` | CRUD de agendamento |
| POST | `/push` | push; header `X-Parse-Push-Status-Id` |
| GET/POST | `/push_audiences` | públicos salvos |
| GET/PUT/DELETE | `/push_audiences/:objectId` | CRUD de público |
| GET/PUT | `/config` | parâmetros globais (master) |
| GET/POST | `/hooks/functions`, `/hooks/triggers` | webhooks/funções |
| GET/PUT | `/hooks/functions/:name`, `/hooks/triggers/:className/:triggerName` | detalhe/edição |
| GET | `/scriptlog` | logs de Cloud Code |
| POST | `/events/AppOpened`, `/events/:eventName` | analytics |
| POST | `/validate_purchase` | validação de compra (legado) |
| POST | `/graphql` | GraphQL (fora do mountPath por padrão) |
| GET | `/playground` | GraphQL Playground (fora do mountPath, se habilitado) |

Regras de resposta:

- `POST /classes/:c` → `201 {"objectId","createdAt"}`.
- `PUT /classes/:c/:id` → `200 {"updatedAt"}`; corpo parcial; `__op` para
  operações atômicas.
- `DELETE` → `200 {}`.
- Erros → `{"code": <int>, "error": "<mensagem>"}` com o status HTTP
  correspondente. Nunca vazar stack trace, SQL ou caminho de arquivo.
- Datas sempre ISO-8601 UTC; números sem perda de precisão; `null` explícito
  quando o campo existe e é nulo.
- `include` expande ponteiros, mas não relations; relations só com `$relatedTo`.

#### Operadores de escrita (`__op`)

| `__op` | Efeito |
|---|---|
| `Increment` | soma `amount` (aceita negativo) |
| `Add` | acrescenta itens ao array (duplicados permitidos) |
| `AddUnique` | acrescenta só o que não existe |
| `Remove` | remove itens iguais |
| `Delete` | remove o campo |
| `AddRelation` / `RemoveRelation` | altera a tabela de junção |

#### Batch

`POST /batch` recebe `{"requests":[{"method":"POST","path":"/parse/classes/X","body":{...}}]}`.
O limite de sub-requests vem de `requestComplexity.batchRequestLimit` (−1 =
sem limite). Cada sub-request tem seu próprio status; o lote só é atômico
quando `"transaction": true` (com retry em conflito). Os paths incluem o
mountPath (ex.: `/parse/classes/X`) e passam pelas mesmas regras de ACL.

#### Idempotência

Se `X-Parse-Request-Id` vier em `POST`/`PUT`/`DELETE` e a rota estiver
habilitada em `idempotencyOptions.paths`, o servidor grava o `reqId` em
`_Idempotency` com TTL. Repetir a mesma chave gera `159 DUPLICATE_REQUEST` — o
Parse **não** devolve a resposta anterior, apenas impede a reexecução. No Parse
o recurso só vale para os adaptadores Mongo e Postgres; no nosso caso, habilitar
para SQLite e Postgres.

### 2.5 GraphQL

- Endpoint padrão `POST /graphql` (fora do `/parse`), configurável.
- Autenticação pelos mesmos headers do REST; o contexto GraphQL carrega
  `user`, `master`, `installationId`, `context`.
- Relay: conexões com `edges { node, cursor }`, `pageInfo`, `first/after/last/before`.
- Nomes gerados por classe `X`:
  - query de objeto: `x` (lowerCamel) com argumento `id`;
  - query de lista: plural de `x` (ex.: `jogos`), com `where`, `order`,
    `skip`, `first`, `after`, `last`, `before`, `include` e `options`;
  - mutations: `createX`, `updateX`, `deleteX`.
- Queries e mutations globais: `node`, `health` (Boolean), `class`, `classes`,
  `cloudConfig`, `callCloudCode`, `logIn`, `logInWith`, `signUp`, `logOut`,
  `challenge`, `resetPassword`, `createFile`,
  `createClass`, `updateClass`, `deleteClass`, `updateCloudConfig`.
- Tipos especiais: `ParseObject`, `Any`, `Date`, `Bytes`, `File`, `FileInfo`,
  `GeoPoint`, `Polygon`, `ACL`, `UserACL`, `RoleACL`, `PublicACL`.
- Entrada de objeto é um tipo `<Classe>Fields`; atualização usa
  `<Classe>Fields` com apenas os campos alterados.
- Introspecção desligada por padrão (`graphQLPublicIntrospection`); playground
  opcional e nunca com master key embutida no HTML.
- Erros GraphQL espelham os códigos do REST em `extensions.code`.

### 2.6 LiveQuery

WebSocket no mesmo processo (padrão), aceitando upgrade em qualquer path — o
cliente oficial conecta em `ws://host:port/`; `/live` fica como alias
documentado. Modo standalone opcional na porta configurada, para escala
horizontal.

Mensagens (JSON compacto, uma por frame):

| Direção | Mensagem |
|---|---|
| cliente → servidor | `{"op":"connect","applicationId","sessionToken?","installationId?","masterKey?","restAPIKey?","javascriptKey?","clientKey?","windowsKey?"}` |
| servidor → cliente | `{"op":"connected"}` |
| cliente → servidor | `{"op":"subscribe","requestId":1,"query":{"className","where","fields"?},"sessionToken"?}` |
| servidor → cliente | `{"op":"subscribed","requestId":1}` |
| servidor → cliente | `{"op":"create"}` / `"enter"` / `"update"` / `"leave"` / `"delete"`, com `requestId` e `object` |
| cliente → servidor | `{"op":"unsubscribe","requestId":1}` |
| servidor → cliente | `{"op":"unsubscribed","requestId":1}` |
| servidor → cliente | `{"op":"error","code":1,"error":"...","reconnect":true}` |

Regras:

- Só classes listadas em `liveQuery.classNames` publicam eventos.
- Cada evento é filtrado por ACL com o `sessionToken` da conexão/assinatura;
  trocar de usuário exige reconectar ou reenviar a assinatura.
- `enter` = antes não casava, agora casa; `leave` = o contrário.
- `fields` projeta o objeto enviado.
- Triggers `beforeConnect`, `beforeSubscribe`, `afterEvent` podem vetar ou
  enriquecer.
- Pub/Sub (em memória, Redis ou SQS) permite várias instâncias.
- Proteção contra ReDoS: timeout de regex (padrão 100 ms) e limite de
  complexidade do filtro.

### 2.7 Cloud Code

Runtime: **QuickJS embutido (`rquickjs`)**, carregando `cloud/main.js` e módulos
locais exigidos por `require` (whitelist de diretório, sem `node_modules` na
primeira versão). A ponte Rust ↔ JS é assíncrona sobre Tokio.

Superfície mínima a expor:

```js
Parse.Cloud.define(name, handler, options?)
Parse.Cloud.run(name, params, options?)
Parse.Cloud.job(name, handler)
Parse.Cloud.beforeSave(className, handler, validationHandler?)
Parse.Cloud.afterSave(className, handler)
Parse.Cloud.beforeDelete(className, handler)
Parse.Cloud.afterDelete(className, handler)
Parse.Cloud.beforeFind(className, handler)
Parse.Cloud.afterFind(className, handler)
Parse.Cloud.beforeLogin(handler)
Parse.Cloud.afterLogin(handler)
Parse.Cloud.afterLogout(handler)
Parse.Cloud.beforePasswordResetRequest(handler)
Parse.Cloud.httpRequest({ url, method, headers, body, params })
Parse.Cloud.useMasterKey()
Parse.Cloud.afterLiveQueryEvent(className, handler, validationHandler?)
Parse.Cloud.beforeConnect(handler, validationHandler?)
Parse.Cloud.beforeSubscribe(className, handler, validationHandler?)
```

Objetos expostos: `Parse.Object` (get/set/increment/add/addUnique/remove/
relation/destroy/save/fetch), `Parse.Query` (equalTo/notEqualTo/lessThan/.../
containedIn/containsAll/matches/ascending/descending/include/select/limit/skip/
first/find/count/get/aggregate), `Parse.User` (current/logIn/logOut/signUp),
`Parse.Role`, `Parse.File`, `Parse.GeoPoint`, `Parse.Polygon`, `Parse.ACL`,
`Parse.Config`, `Parse.Cloud`.

Contrato de trigger:

- `request.object` (não salvo no create), `request.original` (update, em
  beforeSave/afterSave), `request.user`, `request.master`, `request.isReadOnly`,
  `request.installationId`, `request.context` (de `X-Parse-Cloud-Context`),
  `request.query` (beforeFind), `request.objects` (afterFind; o handler devolve o
  array possivelmente alterado).
- `beforeSave` pode lançar `Parse.Error` e alterar o objeto; se retornar objeto,
  substitui. `afterSave`/`afterDelete` não bloqueiam a resposta: se a promise
  rejeitar, o erro é logado e a escrita continua valendo.
- `beforeFind` pode reescrever a query (`request.query`), inclusive negar.
- `job` recebe `{ message, params }` e reporta progresso; cada execução cria
  `_JobStatus`.
- `define` aceita opções de validação (`requireUser`, `requireMaster`,
  `validateMasterKey`, `requireAnyUserRoles`, `requireAllUserRoles`,
  `requireUserKeys`, `fields`), aplicadas também quando a função é chamada por
  `Parse.Cloud.run`.

Limites: memória e tempo por invocação configuráveis; exceções viram erro
`141 SCRIPT_FAILED`; `console.log` vai para `/scriptlog` e para o logger; sem
acesso a filesystem/rede fora de `httpRequest`; um pool de runtimes isolados por
app evita vazamento de estado entre requisições.

### 2.8 Autenticação e sessões

- Senha: **bcrypt** por padrão, custo 10, compatível com bancos existentes
  (`$2a$`/`$2b$` lidos sem alteração); Argon2id opcional por configuração. Nova
  senha sempre no formato configurado; migração transparente no login quando o
  formato antigo for detectado. A coluna interna é `_hashed_password`.
- Sessão: token opaco com prefixo `r:` (revogável), registro em `_Session`,
  `expiresAt`, `sessionLength`, `expireInactiveSessions`, `extendSessionOnUse`,
  `revokeSessionOnPasswordReset`.
- Login: username ou e-mail; `convertUsernameToLowercase`,
  `convertEmailToLowercase`; unicidade case-insensitive quando configurado;
  bloqueio por tentativas (`accountLockout`); política de senha e histórico;
  e-mail verificado opcionalmente.
- Verificação de e-mail e reset: tokens de uso único, TTL configurável,
  páginas HTML localizáveis servidas pelo servidor (sob o mountPath, em
  `/apps/:appId/...`), com placeholders (`{{appName}}`, `{{link}}`,
  `{{userEmail}}`) e templates sobrescrevíveis.
- OAuth: `authData` com os adaptadores embutidos (`apple`, `facebook`, `github`,
  `google`, `instagram`, `linkedin`, `meetup`, `spotify`, `twitter`) e os
  adicionais (`ldap`, `keycloak`, `microsoft`, `mfa`, ...). Validação no servidor
  (nunca confiar no cliente) com `auth` configurado; vínculo de conta existente
  respeita e-mail verificado; conflito de vínculo retorna `208` com o
  provedor/id já associado.
- MFA: TOTP e SMS; `challenge` com token de curta duração; recuperação por
  códigos de backup; segredo nunca retorna após a criação.
- Chaves: master, read-only master e maintenance; allowlist de IP
  (`masterKeyIps`, `readOnlyMasterKeyIps`, `maintenanceKeyIps`), TTL de master
  key e auditoria de uso.
- `protectedFields` e `userSensitiveFields` removem campos na leitura; no
  response de save, `protectedFields` só é aplicado quando
  `protectedFieldsSaveResponseExempt` é `false`. `enforcePrivateUsers` restringe
  `_User` a si mesmo.

### 2.9 Arquivos

- Upload: `POST /files/:fileName` com corpo cru e `Content-Type`
  obrigatório (o appId vem no header; aceite também o formato legado com
  appId no path); o nome é gerado (32 hex + `_` + nome original, preservando a
  extensão) quando `preserveFileName` é `false`; limites por `maxUploadSize`;
  `X-Parse-Upload-Mode` para upload em partes (opcional).
- Download: `GET /files/:appId/:fileName`, com `Range`, `ETag`,
  `Content-Disposition`, cache; `fileUpload`/`fileDownload` controlam quem pode
  enviar/baixar (`public`, `authenticated`, `anonymous`), e
  `allowedFileUrlDomains` restringe URLs de arquivo aceitas em objetos.
- Adaptadores: `FilesAdapter { create_file, delete_file, get_file_data,
  get_file_location }`; implementações FS (padrão), S3 (assinatura v4) e GCS.
- Metadados ficam no objeto (`name`, `url`, `__type: "File"`); tags e metadata
  enviadas por header são preservadas e consultáveis.
- A URL vem do adaptador: no FS ela aponta para o servidor e passa por validação
  de sessão; em S3/GCS pode ser pública ou assinada. `directAccess` é outra
  coisa: controla se chamadas do Cloud Code no mesmo processo vão direto ao
  banco/servidor em vez de fazer HTTP para si mesmas (padrão `true`).
- Apagar objeto **não** apaga o arquivo; apagar arquivo exige operação
  explícita (`Parse.File.destroy`) ou `DELETE /files/:appId/:filename` com
  master key.
- Importação: copiar arquivos do Mongo GridFS/S3 antigo para o adaptador novo,
  preservando `name` e URL pública.

### 2.10 Push

- Entrada: `POST /push` com `where` ou `channels` (ou `audience_id`), `data`,
  `push_time`, `expiration_time`, `expiration_interval`. Push localizado usa
  `data.alert` como objeto (`loc-key`, `loc-args`) ou o formato do provedor.
- Público: query em `_Installation` (`deviceType`, `channels`, `badge`,
  `appVersion`, `localeIdentifier`, `timeZone`); canais são arrays.
- Payload APNs: `aps` com `alert`, `badge`, `sound`, `content-available`,
  `mutable-content`, `category`, `thread-id`; FCM: `notification` + `data`.
- Adaptadores: APNs HTTP/2 com JWT (`.p8`, keyId, teamId) e FCM HTTP v1
  (service account). O trait `PushAdapter` recebe lotes e devolve
  `sent/failed` por dispositivo com o motivo.
- `_PushStatus` registra o ciclo (`pending`, `scheduled`, `running`, `succeeded`,
  `failed`), contadores por tipo e por fuso; o cliente recebe
  `X-Parse-Push-Status-Id`.
- Push agendado: `push_time` com `scheduledPush: true` grava `_PushStatus` como
  `scheduled`; o envio em si precisa de um scheduler (no Parse, job externo; no
  projeto, scheduler embutido com lock no banco).
- Auditoria: nunca logar device token completo; mascarar.

### 2.11 Hooks, jobs e configuração

- `/hooks/functions`: funções registradas via REST, executadas como webhook
  (`url`, `X-Parse-Webhook-Key`) ou como função nativa. Triggers por classe e
  nome (`beforeSave`, `afterSave`, ...).
- Jobs: `POST /jobs/:jobName` responde imediatamente com o id do `_JobStatus`;
  o job roda em background e atualiza `status`, `message`, `finishedAt`.
- `cloud_code/jobs`: CRUD de agendamentos (`daysOfWeek`, `timeOfDay`,
  `repeatMinutes`, `startAfter`, `lastRun`), com scheduler no processo e
  proteção contra execução dupla (lock no banco).
- `/config`: parâmetros globais tipados (string, number, boolean, date, array,
  object), cache em memória com invalidação, leitura pública e escrita master.
- `/serverInfo`: responder `{features, parseServerVersion}` no mesmo formato do
  Parse, com a versão da nossa implementação.

### 2.12 Painel administrativo (htmx)

Substitui o Parse Dashboard. Requisitos: HTML gerado no servidor (`maud`),
htmx embutido no binário, CSS embutido no binário e servido como arquivo (nada
de `unsafe-inline`), nenhuma etapa de build de front-end, acessível e utilizável
sem JavaScript para as operações essenciais.

Telas:

| Tela | Conteúdo |
|---|---|
| Login | usuário/senha do painel (criado por CLI), sessão em cookie httpOnly + SameSite=Strict + CSRF por formulário |
| Apps | lista de servidores registrados (URL + appId + chave mascarada), troca de app, teste de conexão |
| Navegador de dados | classes, tabela paginada, filtro por construtor (campo/operador/valor) e modo texto (`where`), colunas, ordenação, busca, criar/editar/apagar, ver JSON, navegar por ponteiro/relação |
| Registro | editor por tipo (data, geo, arquivo com preview, ACL, relação múltipla), validação no servidor, erros por campo |
| Schema | classes, campos (tipo, obrigatório, default, targetClass), CLP por operação, índices, protectedFields |
| Push | composição de query/canais, payload, agendamento, status e histórico (`_PushStatus`) |
| Jobs | agendamentos, executar agora com parâmetros, histórico e mensagens (`_JobStatus`) |
| Logs | tail ao vivo (SSE) de `scriptlog` e do logger, filtros por nível/classe |
| Config | parâmetros globais, edição com validação |
| Segurança | chaves mascaradas (nunca exibir master), IP allowlist, políticas de sessão/senha |
| GraphQL | playground embutido (assets locais), sem master key no HTML |
| LiveQuery | console de teste: assinar classe, ver eventos, inspecionar ACL aplicada |
| Exportar/importar | JSON/CSV de uma classe, com limites e confirmação |

Padrões htmx: tabela como fragmento (`#tabela`), filtro com debounce
(`hx-trigger="keyup changed delay:350ms"`), paginação com `hx-push-url`,
formulários como POST normal com redirect 303, exclusão com `hx-confirm`,
modal com `hx-get`/`hx-target`, logs com SSE (`hx-ext="sse"`), upload com
`hx-encoding="multipart/form-data"`.

Segurança do painel: sessão separada da API; autorização por papel
(`admin`, `readonly`); toda ação de escrita audita `quem/quando/o quê`;
CSRF token obrigatório; CSP sem `unsafe-inline` (CSS em arquivo servido pelo
binário); rate limit de login; bloqueio após N tentativas; nunca renderizar
valor de chave secreta no HTML, apenas "configurada" e um botão de rotação.

### 2.13 Adaptadores (traits)

| Trait | Métodos mínimos |
|---|---|
| `StorageAdapter` | `class_exists`, `create_class`, `add_field`, `update_field`, `delete_class`, `get_all_classes`, `get_class`, `create_object`, `find`, `count`, `distinct`, `update_objects`, `delete_objects`, `find_one_and_update`, `upsert`, `ensure_index`, `ensure_uniqueness`, `aggregate`, `transaction` |
| `FilesAdapter` | `create_file`, `delete_file`, `get_file_data`, `get_file_location` |
| `PushAdapter` | `send`, `valid_push_types` |
| `EmailAdapter` | `send_mail` |
| `CacheAdapter` | `get`, `put`, `del`, `clear` |
| `PubSubAdapter` | `publish`, `subscribe`, `unsubscribe` |
| `WssAdapter` | `start`, `close` |
| `LoggerAdapter` | `log` |

Regra: o servidor nunca importa uma implementação concreta; a composição
escolhe por configuração. Cada adaptador tem teste de conformidade próprio,
rodando a mesma bateria.

### 2.14 Configuração

Precedência: linha de comando > variável de ambiente > arquivo JSON > padrão.
Os padrões abaixo seguem o Parse Server quando ele define um; quando não define,
mostramos o padrão do projeto. As exceções estruturais estão na tabela de desvios
(§2.17) — em especial `databaseURI` (SQLite) e o adaptador de arquivos (FS).

| Opção | Env | Padrão |
|---|---|---|
| `appId` | `PARSE_SERVER_APPLICATION_ID` | obrigatório |
| `masterKey` | `PARSE_SERVER_MASTER_KEY` | obrigatório |
| `databaseURI` | `PARSE_SERVER_DATABASE_URI` | `sqlite://./data.db` |
| `serverURL` | `PARSE_SERVER_URL` | `http://localhost:1337/parse` |
| `publicServerURL` | `PARSE_PUBLIC_SERVER_URL` | igual a `serverURL` |
| `port` | `PORT` | `1337` |
| `mountPath` | `PARSE_SERVER_MOUNT_PATH` | `/parse` |
| `cloud` | `PARSE_SERVER_CLOUD` | — |
| `appName` | `PARSE_SERVER_APP_NAME` | `Parse Server` |
| `allowClientClassCreation` | `PARSE_SERVER_ALLOW_CLIENT_CLASS_CREATION` | `false` |
| `maxUploadSize` | `PARSE_SERVER_MAX_UPLOAD_SIZE` | `20mb` |
| `preserveFileName` | `PARSE_SERVER_PRESERVE_FILE_NAME` | `false` |
| `objectIdSize` | `PARSE_SERVER_OBJECT_ID_SIZE` | `10` |
| `defaultLimit` / `maxLimit` | `PARSE_SERVER_DEFAULT_LIMIT` / `PARSE_SERVER_MAX_LIMIT` | `100` / ilimitado |
| `sessionLength` | `PARSE_SERVER_SESSION_LENGTH` | `31536000` |
| `revokeSessionOnPasswordReset` | `PARSE_SERVER_REVOKE_SESSION_ON_PASSWORD_RESET` | `true` |
| `verifyUserEmails` | `PARSE_SERVER_VERIFY_USER_EMAILS` | `false` |
| `preventLoginWithUnverifiedEmail` | `PARSE_SERVER_PREVENT_LOGIN_WITH_UNVERIFIED_EMAIL` | `false` |
| `passwordPolicy` | `PARSE_SERVER_PASSWORD_POLICY` | — |
| `accountLockout` | `PARSE_SERVER_ACCOUNT_LOCKOUT` | — |
| `auth` | `PARSE_SERVER_AUTH_PROVIDERS` | — |
| `filesAdapter` | `PARSE_SERVER_FILES_ADAPTER` | FS |
| `emailAdapter` | `PARSE_SERVER_EMAIL_ADAPTER` | console |
| `push` | `PARSE_SERVER_PUSH` | — |
| `liveQuery.classNames` | `PARSE_SERVER_LIVEQUERY_CLASSNAMES` | `[]` |
| `startLiveQueryServer` | `PARSE_SERVER_START_LIVE_QUERY_SERVER` | `false` |
| `mountGraphQL` / `graphQLPath` | `PARSE_SERVER_MOUNT_GRAPHQL` / `PARSE_SERVER_GRAPHQL_PATH` | `false` / `/graphql` |
| `graphQLPublicIntrospection` | `PARSE_SERVER_GRAPHQL_PUBLIC_INTROSPECTION` | `false` |
| `mountPlayground` / `playgroundPath` | `PARSE_SERVER_MOUNT_PLAYGROUND` / `PARSE_SERVER_PLAYGROUND_PATH` | `false` / `/playground` |
| `masterKeyIps` | `PARSE_SERVER_MASTER_KEY_IPS` | `["127.0.0.1","::1"]` |
| `maintenanceKey` / `maintenanceKeyIps` | `PARSE_SERVER_MAINTENANCE_KEY` / `PARSE_SERVER_MAINTENANCE_KEY_IPS` | — |
| `idempotencyOptions` | `PARSE_SERVER_EXPERIMENTAL_IDEMPOTENCY_OPTIONS` | — |
| `rateLimit` | `PARSE_SERVER_RATE_LIMIT` | — |
| `requestComplexity` | `PARSE_SERVER_REQUEST_COMPLEXITY` | — |
| `routeAllowList` | `PARSE_SERVER_ROUTE_ALLOW_LIST` | — |
| `protectedFields` | `PARSE_SERVER_PROTECTED_FIELDS` | — |
| `logLevel` / `logsFolder` / `jsonLogs` | `PARSE_SERVER_LOG_LEVEL` / `PARSE_SERVER_LOGS_FOLDER` / `JSON_LOGS` | `info` / `./logs` / `false` |

Além dessas, o `src/Options/Definitions.js` do Parse Server registra **292
variáveis de ambiente** (a maioria `PARSE_SERVER_*`). O prompt exige:

- um registro único de opções no código, com tipo, padrão, sensível, env e
  help, gerando `--help`, documentação e validação;
- um teste que garanta que toda opção sensível é mascarada em log e painel;
- um comando `parse-cli doctor` que valida a configuração sem subir o servidor.

### 2.15 Erros

Formato: `{"code": <int>, "error": "<mensagem>"}`. Códigos principais
(extraídos do SDK oficial):

| Código | Nome | Quando |
|---|---|---|
| -1 | `OTHER_CAUSE` | erro não classificado |
| 1 | `INTERNAL_SERVER_ERROR` | falha inesperada |
| 100 | `CONNECTION_FAILED` | conexão com dependência |
| 101 | `OBJECT_NOT_FOUND` | objeto inexistente |
| 102 | `INVALID_QUERY` | query inválida |
| 103 | `INVALID_CLASS_NAME` | classe inválida |
| 104 | `MISSING_OBJECT_ID` | id ausente |
| 105 | `INVALID_KEY_NAME` | campo inválido |
| 106 | `INVALID_POINTER` | ponteiro inválido |
| 107 | `INVALID_JSON` | JSON malformado |
| 111 | `INCORRECT_TYPE` | tipo errado |
| 112 | `INVALID_CHANNEL_NAME` | canal de push inválido |
| 115 | `PUSH_MISCONFIGURED` | push mal configurado |
| 116 | `OBJECT_TOO_LARGE` | objeto grande demais |
| 119 | `OPERATION_FORBIDDEN` | operação proibida |
| 121 | `INVALID_NESTED_KEY` | chave aninhada inválida |
| 122 | `INVALID_FILE_NAME` | nome de arquivo inválido |
| 123 | `INVALID_ACL` | ACL inválida |
| 124 | `TIMEOUT` | timeout |
| 125 | `INVALID_EMAIL_ADDRESS` | e-mail inválido |
| 129 | `FILE_TOO_LARGE` | arquivo grande demais |
| 130 | `FILE_SAVE_ERROR` | falha ao salvar arquivo |
| 137 | `DUPLICATE_VALUE` | valor duplicado |
| 139 | `INVALID_ROLE_NAME` | nome de role inválido |
| 141 | `SCRIPT_FAILED` | Cloud Code falhou |
| 142 | `VALIDATION_ERROR` | validação (trigger) |
| 151 | `UNSAVED_FILE_ERROR` | arquivo não salvo |
| 153/161 | `FILE_DELETE_ERROR` / `FILE_DELETE_UNNAMED_ERROR` | apagar arquivo |
| 155 | `REQUEST_LIMIT_EXCEEDED` | rate limit |
| 159 | `DUPLICATE_REQUEST` | idempotência |
| 200 | `USERNAME_MISSING` | signup sem username |
| 201 | `PASSWORD_MISSING` | senha ausente |
| 202 | `USERNAME_TAKEN` | username em uso |
| 203 | `EMAIL_TAKEN` | e-mail em uso |
| 204 | `EMAIL_MISSING` | e-mail ausente |
| 205 | `EMAIL_NOT_FOUND` | e-mail não encontrado |
| 206 | `SESSION_MISSING` | sessão ausente |
| 207 | `MUST_CREATE_USER_THROUGH_SIGNUP` | usuário não criado por signup |
| 208 | `ACCOUNT_ALREADY_LINKED` | conta já vinculada |
| 209 | `INVALID_SESSION_TOKEN` | token inválido |
| 210/211 | `MFA_ERROR` / `MFA_TOKEN_REQUIRED` | MFA |
| 250 | `LINKED_ID_MISSING` | vínculo sem id |
| 251 | `INVALID_LINKED_SESSION` | sessão de vínculo inválida |
| 252 | `UNSUPPORTED_SERVICE` | provedor não suportado |
| 255 | `INVALID_SCHEMA_OPERATION` | operação de schema inválida |
| 600 | `AGGREGATE_ERROR` | agregação inválida |
| 601 | `FILE_READ_ERROR` | leitura de arquivo |

A tabela completa (61 códigos do SDK + os internos do servidor) deve virar um
`parse-error-codes.csv` no repositório, gerado/verificado por teste, para que
nenhum código seja inventado.

### 2.16 Importação de um Parse existente

Comando `parse-cli migrate --from <mongo-uri|postgres-uri> --to <sqlite|postgres>`:

1. lê `_SCHEMA` e recria classes, campos, CLP e índices;
2. copia documento por documento, preservando `objectId`, `createdAt`,
   `updatedAt` e `ACL`;
3. converte `_User` mantendo `_hashed_password` (bcrypt) e tokens internos;
4. copia relations (`_Join` collections) para as tabelas de junção;
5. copia arquivos do GridFS/S3 para o adaptador de destino;
6. valida contagem e checksum por classe e gera relatório;
7. é idempotente e retomável (checkpoint por classe).

Sem isso, o clone só serve para projetos novos — e a promessa de "substituto"
não se cumpre.

### 2.17 Desvios conscientes

| Área | Parse Server | Este projeto | Por quê |
|---|---|---|---|
| Banco | MongoDB (padrão) e PostgreSQL | SQLite (padrão) e PostgreSQL; importação de Mongo | um arquivo, operação simples; Mongo segue como origem de migração |
| Arquivos | GridFS (padrão) | FS (padrão), S3, GCS | não faz sentido depender de Mongo para arquivo |
| LiveQuery | processo separado (porta 1337) | embutido no mesmo processo (com modo standalone) | menos peças para operar; protocolo idêntico |
| Cloud Code | Node.js completo | QuickJS + shim de `Parse.*` | binário único; `require` restrito a módulos locais |
| Painel | Parse Dashboard (React, app à parte) | HTML no servidor + htmx embutido | sem build de front-end; mesma cobertura funcional |
| Middleware Express | `ParseServer` montável em Express | `parse-server` como biblioteca axum + binário | o alvo é o contrato HTTP, não a API do Express |
| Push | APNs, FCM e adaptadores comunitários | APNs e FCM | cobre os SDKs oficiais; o resto por adaptador |
| Agregação | todos os estágios do MongoDB | subconjunto documentado (§2.3); o resto retorna 600 | portável entre SQLite e Postgres |

Cada desvio precisa de teste de contrato que prove que o comportamento
observável via HTTP continua igual. O que muda é a implementação, não o
contrato.

---

## 3. Estratégia de testes e compatibilidade

### 3.1 Servidor de referência

`docker/compose.yaml` sobe:

- `mongo:7` com volume;
- `parseplatform/parse-server:latest` com `PARSE_SERVER_APPLICATION_ID`,
  `PARSE_SERVER_MASTER_KEY`, `PARSE_SERVER_DATABASE_URI`, `PARSE_SERVER_URL`,
  `PARSE_SERVER_MOUNT_PATH=/parse`, `PARSE_SERVER_START_LIVE_QUERY_SERVER=true`,
  `PARSE_SERVER_MOUNT_GRAPHQL=true` e `PARSE_SERVER_LIVEQUERY_CLASSNAMES` com as
  classes observadas;
- o seu servidor, em perfil separado, na mesma rede.

O harness (`tests/conformance`) recebe um cenário declarativo e executa nos dois
servidores:

```yaml
- name: cria e consulta
  steps:
    - method: POST
      path: /parse/classes/Produto
      body: { nome: "Café", preco: 32.5, tags: ["bebida"] }
    - method: GET
      path: /parse/classes/Produto?where={"preco":{"$gt":10}}
  normalize: [objectId, createdAt, updatedAt, sessionToken]
```

Comparação: JSON normalizado (ordem de chaves, ids e datas substituídos por
marcadores), depois diff semântico. Divergência = falha com o par
request/response no relatório.

### 3.2 Suítes oficiais

- **Parse-SDK-JS**: rodar a suíte de integração contra o servidor. O helper
  oficial sobe um `parse-server` embutido e aponta para
  `http://localhost:1337/parse` com `appId: integration`,
  `masterKey: notsosecret`, Cloud Code em `integration/test/cloud/main.js` e
  configuração específica (`liveQuery.classNames`, `idempotencyOptions.paths`,
  upload público). O harness substitui o boot do `parse-server` pelo nosso
  binário e replica essa configuração; testes que dependem de semântica de
  MongoDB entram em `tests/sdk-js/exclusoes.txt` com o motivo.
- **LiveQuery**: cliente oficial JS (`Parse.LiveQuery`) contra o servidor;
  cenários de create/enter/update/leave/delete, ACL e reconexão.
- **GraphQL**: snapshot de introspecção + operações geradas + mutations.
- **Adaptadores**: a bateria de `parse-server-conformance-tests` traduzida para
  Rust, rodando contra cada implementação.

### 3.3 Pirâmide

| Nível | O que cobre |
|---|---|
| unidade | domínio: ACL, CLP, query → SQL, validação, schema |
| integração | rota → banco em memória; transações; idempotência |
| contrato | dourado contra o servidor de referência |
| SDK | suíte oficial JS |
| segurança | matriz de autorização por rota e por chave |
| carga | k6/oha: CRUD, query, login, LiveQuery com 1k conexões |
| caos | queda do banco, do adaptador de arquivos, do Pub/Sub |

### 3.4 Relatório de paridade

`docs/PARIDADE.md` com uma linha por funcionalidade e quatro colunas: estado
(✅/🔨/📋), evidência (teste/arquivo), divergência conhecida, onda. O relatório
é gerado a partir dos testes; números escritos à mão envelhecem.

---

## 4. Ondas de entrega

Cada onda é um conjunto de PRs pequenos, com o critério de aceite em negrito.
Tempo é indicativo para um agente assistido; o que importa é o critério.

| # | Onda | Entrega | Critério de aceite |
|---|---|---|---|
| 0 | Fundação | workspace, CLI `serve`, config, health, serverInfo, CI, compose de referência, harness | **health e serverInfo no mesmo formato (versão pode diferir); harness roda no CI** |
| 1 | Núcleo e schema | classes, `_SCHEMA`, tipos, CRUD REST, índices, `allowClientClassCreation` | **CRUD de classe via REST com dourados; 90% dos tipos cobertos** |
| 2 | Consultas e permissões | operadores, pointers, relations, includes, batch, aggregate, ACL/CLP, idempotência | **matriz de ACL passa; operadores dourados; batch com `transaction: true` atômico** |
| 3 | Usuários e sessões | signup, login, logout, sessões, roles, installations, verificação, reset | **suíte de usuários do SDK JS verde** |
| 4 | Arquivos | FS, S3, GCS, upload/download, metadados, limites | **suíte de arquivos do SDK JS verde** |
| 5 | Cloud Code | QuickJS, funções, triggers, jobs, `/hooks`, `/scriptlog` | **exemplos do guia de Cloud Code rodam sem alteração** |
| 6 | Push e agendamento | APNs, FCM, `_PushStatus` (incluindo `scheduled`), scheduler com lock | **push para adaptador fake com status correto; push agendado executa uma única vez** |
| 7 | GraphQL | schema gerado, Relay, playground, auth | **introspecção (habilitada nos testes) e operações douradas; SDK JS GraphQL verde** |
| 8 | LiveQuery | WebSocket, matching, ACL, Pub/Sub, triggers | **cliente oficial JS passa nos 5 eventos com ACL** |
| 9 | Painel htmx | todas as telas da §2.12, e2e, auditoria | **checklist do painel completo; nenhuma chave secreta no HTML; e2e verde** |
| 10 | Produção | rate limit, complexidade, routeAllowList, logs, métricas, Docker, importação, docs | **relatório de paridade ≥95%; importação de um app real validada** |

Ordem obrigatória: 0 → 1 → 2 → 3. As ondas 4–8 podem ser paralelizadas por
pessoas diferentes depois da 3. O painel (9) começa na 1 e cresce junto, mas só
fecha depois da 8. A 10 não começa antes de a 8 estar verde.

---

## 5. Regras de engenharia

### 5.1 Repositório e código

- Um crate, uma responsabilidade; dependências apontam para dentro
  (`parse-http` → `parse-core`, nunca o contrário).
- Erros com `thiserror` no domínio e `Problem`/`ParseError` na borda; `unwrap`
  proibido fora de teste.
- Nada de SQL concatenado; tudo por parâmetro. Nome de classe/campo validado
  antes de virar identificador.
- Toda operação de escrita é transacional; a idempotência por
  `X-Parse-Request-Id` é aplicada quando a rota permite.
- `unsafe` proibido, salvo em FFI isolada e documentada.
- Feature flags para adaptadores pesados (S3, GCS, APNs, FCM, Redis) e para o
  runtime JS, para o binário mínimo continuar pequeno.

### 5.2 Qualidade

- `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace` no CI, nos três SOs.
- Cobertura mínima de 85% no domínio e 75% no servidor (medida, não aspirada).
- Nenhum PR sem teste de contrato para a funcionalidade nova.
- Benchmarks de regressão para query e batch; orçamento: não piorar 10% sem
  justificativa.

### 5.3 Segurança

- Modelo de ameaças em `docs/SEGURANCA.md`: chaves, sessões, ACL, SSRF no
  `httpRequest`, path traversal em arquivos, ReDoS em regex, injeção em filtros,
  CSRF no painel, vazamento em logs.
- Segredos nunca em log, resposta, HTML ou mensagem de erro. Mascarar chaves
  como `***` e tokens como `r:…` (4 primeiros caracteres).
- Rate limit por IP e por app; complexidade de query limitada
  (`requestComplexity`: profundidade e quantidade de `include`, profundidade de
  subconsultas, profundidade do GraphQL, regex).
- Dependabot/auditoria (`cargo audit`) no CI; política de resposta a
  vulnerabilidade.

### 5.4 Licença e marca

- Licença do projeto: **Apache-2.0** (alinhada ao Parse Server), com NOTICE.
- O `parse-dashboard` **não** é Apache-2.0: usa uma licença própria da Parse,
  LLC. O painel é reimplementação limpa — não copie código, CSS, textos ou
  assets dele.
- Sem código copiado; se houver trecho derivado, marcar arquivo e NOTICE.
- Nome próprio; README diz "compatível com a API do Parse Platform" e não usa
  os logos do Parse sem permissão.

---

## 6. Decisões abertas (resolver na onda 0, registrar em ADR)

| # | Decisão | Opções | Recomendação |
|---|---|---|---|
| 1 | Banco padrão | SQLite / Postgres | SQLite para desenvolvimento e apps pequenos; Postgres para produção |
| 2 | Runtime de Cloud Code | QuickJS (`rquickjs`) / Boa / WASM | QuickJS: menor e mais compatível com JS moderno |
| 3 | Hash de senha | bcrypt / Argon2id | bcrypt por compatibilidade, Argon2id opcional |
| 4 | LiveQuery | embutido / processo separado | embutido por padrão, standalone para escala |
| 5 | Multi-app | um app por processo / vários | um por processo (como o Parse); painel gerencia vários servidores |
| 6 | Agregação | subconjunto / completo | subconjunto com erro claro para o resto |
| 7 | SDK próprio | Rust / nenhum | `parse-sdk-rs` só depois da paridade; o oráculo é o SDK JS |
| 8 | Painel | embutido / separado | embutido, com registro de servidores remotos |

---

## 7. Anexos

### A. Checklist final de paridade

- [ ] `/parse/health` e `/parse/serverInfo` no mesmo formato (versão pode diferir).
- [ ] CRUD de classes, batch, aggregate e purge.
- [ ] Todos os operadores de query e `__op` de escrita.
- [ ] ACL, CLP, protectedFields, master/read-only/maintenance key.
- [ ] Signup, login, logout, sessão revogável, expiração e logout remoto.
- [ ] Verificação de e-mail, reset de senha, troca de e-mail.
- [ ] OAuth (Google, Apple, Facebook, GitHub e demais adaptadores).
- [ ] MFA (TOTP e SMS) com challenge.
- [ ] Upload/download/delete de arquivos nos três adaptadores.
- [ ] Cloud Code: define, run, triggers, jobs, httpRequest.
- [ ] Push APNs/FCM com `_PushStatus`.
- [ ] GraphQL com introspecção habilitável e mutations.
- [ ] LiveQuery com os cinco eventos e ACL.
- [ ] Importação de um app Parse existente.
- [ ] Painel: login, dados, schema, push, jobs, logs, config, GraphQL.
- [ ] Nenhum segredo em HTML/log; CSRF e rate limit ativos.
- [ ] Relatório de paridade e suíte do SDK JS verdes.

### B. Nomes de trigger

`beforeLogin`, `afterLogin`, `afterLogout`, `beforePasswordResetRequest`,
`beforeSave`, `afterSave`, `beforeDelete`, `afterDelete`, `beforeFind`,
`afterFind`, `beforeConnect`, `beforeSubscribe`, `afterEvent`.

### C. Vocabulário

| Termo | Significado |
|---|---|
| CLP | Class Level Permissions — permissões por operação na classe |
| ACL | Access Control List por objeto |
| Pointer | Referência a um objeto de outra classe |
| Relation | Conjunto de objetos, armazenado em tabela de junção |
| `__op` | Operação atômica de escrita em um campo |
| `_SCHEMA` | Metadados de classes, campos, CLP e índices |
| LiveQuery | Assinatura em tempo real via WebSocket |
| Cloud Code | Código do usuário executado no servidor (funções/triggers/jobs) |
| Master key | Chave administrativa que ignora ACL/CLP |

### D. Primeiros arquivos a escrever

```text
Cargo.toml
crates/parse-core/src/lib.rs          Value, Object, ACL, Schema, Error
crates/parse-storage/src/lib.rs       trait StorageAdapter
crates/parse-storage/src/sqlite.rs    implementação SQLite
crates/parse-http/src/lib.rs          Router, headers, /health, /classes
crates/parse-server/src/main.rs       composição e lifecycle
crates/parse-cli/src/main.rs          serve, migrate, doctor, user
crates/parse-admin/src/lib.rs         layout, login, tabela, formulário
tests/conformance/src/lib.rs          harness de comparação
docker/compose.yaml                   Mongo + Parse Server de referência
docs/ARQUITETURA.md                   ADRs e diagrama
docs/PARIDADE.md                      tabela gerada pelos testes
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
