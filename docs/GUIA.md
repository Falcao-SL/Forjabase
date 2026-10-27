# Guia de uso — Forja

Como usar cada parte do Forja. Cada seção diz se já funciona na versão atual
(**0.0.2**) ou se descreve como vai funcionar, para quem quiser acompanhar ou
contribuir. A lista completa está em [`FUNCIONALIDADES.md`](FUNCIONALIDADES.md);
o roteiro, em [`PLANO.md`](PLANO.md).

> **O que funciona na 0.0.3:** `forja serve`, `forja superuser upsert`,
> `/api/health`, login de superusuário, coleções dos tipos base e auth (criar,
> listar, ver, apagar), registros (listar com filtro, ordenação e paginação;
> criar, ver, atualizar, apagar) com regras de acesso, autenticação de usuários
> com verificação de e-mail, redefinição de senha e troca de e-mail, e o painel
> administrativo em `/_/` (HTML no servidor + htmx). O cliente Python do
> PocketBase já funciona contra o Forja. O resto deste guia está marcado
> *(planejado)* e pode mudar até a 1.0.0.

---

## 1. Instalar

### A partir do código (funciona)

Requisitos: Rust estável recente (`rustup`). Não há etapa de build de
front-end: o painel vai junto com o binário.

```sh
git clone https://github.com/iramarfalcao/Forjabase.git
cd forjabase
cargo build --release -p forja
./target/release/forja --version     # forja 0.0.3
```

### Executável pronto *(planejado)*

Baixar do GitHub Releases o executável do seu sistema (macOS, Linux, Windows;
x86_64 e arm64), ou usar a imagem Docker:

```sh
docker run -p 8090:8090 -v ./forja_data:/data ghcr.io/iramarfalcao/forja serve --http 0.0.0.0:8090
```

## 2. Subir o servidor (funciona)

```sh
forja superuser upsert admin@exemplo.com 'uma-senha-forte'   # senha com 10+ caracteres
forja serve                          # http://127.0.0.1:8090, dados em ./forja_data
forja serve --dir /srv/forja         # outra pasta de dados
forja serve --http 0.0.0.0:8090      # aceitar conexões de fora
curl http://127.0.0.1:8090/api/health
# {"code":200,"data":{},"message":"API is healthy."}
```

### Motor do banco

SQLite é o motor suportado — um arquivo por projeto, nada para instalar.

```sh
forja serve                                  # forja_data/default/data.db
```

Turso e PostgreSQL ainda compilam com `--features turso` / `--features postgres`,
mas são **experimentais e sem suporte** ([ARQUITETURA](ARQUITETURA.md), decisão
13): não entram no build padrão, não rodam no CI e podem quebrar entre versões.

*(planejado)* `forja serve exemplo.com` com HTTPS automático, `--origins` para
CORS, `--dev` para logs de SQL.

## 3. Primeiro acesso e painel (funciona)

Hoje o superusuário é criado pela CLI (`forja superuser upsert`, acima); o
`serve` avisa se ainda não houver nenhum. *(planejado)* Na primeira execução,
o Forja abre no navegador um link de uso único para criar o superusuário.

Com o servidor no ar, abra `http://127.0.0.1:8090/_/` e entre com o e-mail e a
senha do superusuário. O painel é HTML gerado no servidor com htmx: a lista de
coleções, o filtro da tabela (com `hx-get` a cada tecla), a paginação e o
apagar com confirmação trocam só o fragmento `#tabela`, sem recarregar a
página. O formulário de criação e edição é um `POST` comum e volta para a
lista — funciona até com JavaScript desligado.

A sessão é um cookie `forja_painel`, `httpOnly` e `SameSite=Strict`, com um
JWT de superusuário válido por 24 h. Sair (`/_/logout`) apaga o cookie. A API
continua aceitando `Authorization: <token>` como antes; o cookie vale só para
as rotas `/_/`. Em HTTP local o cookie não leva `Secure`; quando o servidor
tiver TLS (0.9), ele passa a ser obrigatório.

O que ainda não existe no painel: editor de coleções e campos, configurações,
backups, logs e cron. Por enquanto, isso é CLI ou API.

## 4. Projetos *(planejado)*

Uma instância atende vários projetos isolados.

```sh
forja projects create loja
forja projects list
```

| Projeto | URL base para os SDKs e a API |
|---|---|
| `default` | `https://host` (compatível com clientes do PocketBase) |
| `loja` | `https://host/p/loja` |
| `loja` com domínio mapeado | `https://api.loja.com` |

## 5. Coleções e registros (funciona)

Entrar como superusuário e guardar o token:

```sh
TOKEN=$(curl -s -X POST http://127.0.0.1:8090/api/collections/_superusers/auth-with-password \
  -H "Content-Type: application/json" \
  -d '{"identity":"admin@exemplo.com","password":"uma-senha-forte"}' | jq -r .token)
```

Criar uma coleção. Regra `""` = pública; `null` (ou ausente) = só superusuário;
um filtro = só os registros que passam nele:

```sh
curl -X POST http://127.0.0.1:8090/api/collections \
  -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"name":"produtos",
       "fields":[{"name":"nome","type":"text","required":true,"max":80},
                 {"name":"preco","type":"number","min":0},
                 {"name":"ativo","type":"bool"},
                 {"name":"tags","type":"select","values":["novo","promo"],"maxSelect":2}],
       "listRule":"ativo = true","viewRule":"ativo = true","createRule":null}'
```

Tipos de campo que já guardam valores: `text`, `editor`, `number`, `bool`,
`email`, `url`, `date`, `select`, `json`, `relation` e `password` (só em
coleção `auth`). Os outros (`file`, `autodate`, `geoPoint`) são recusados
com uma mensagem clara até chegarem. `relation` guarda o id (ou uma lista,
se `maxSelect` > 1) e aceita `?expand=campo` ou `?expand=colecao_via_campo`.

Registros:

```sh
curl -X POST http://127.0.0.1:8090/api/collections/produtos/records \
  -H "Authorization: $TOKEN" -H "Content-Type: application/json" \
  -d '{"nome":"Café","preco":32,"ativo":true,"tags":["novo"]}'

curl "http://127.0.0.1:8090/api/collections/produtos/records?filter=(preco<50%20%26%26%20nome~'café')&sort=-created&perPage=20"
curl -X PATCH  http://127.0.0.1:8090/api/collections/produtos/records/<id> -H "Authorization: $TOKEN" -H "Content-Type: application/json" -d '{"preco":30}'
curl -X DELETE http://127.0.0.1:8090/api/collections/produtos/records/<id> -H "Authorization: $TOKEN"
```

Filtros já suportados: `=`, `!=`, `>`, `>=`, `<`, `<=`, `~` (contém), `!~`,
`&&`, `||` e parênteses. Ordenação: `sort=-created,nome` e `@random`. Erros
de validação vêm no formato do PocketBase:

```json
{"code":400,"message":"Failed to create record.","data":{"nome":{"code":"validation_required","message":"Cannot be blank."}}}
```

Regras e filtros já aceitam `@request.auth.*` (o registro autenticado) e
`@collection.colecao.campo` (uma linha de outra coleção, via `EXISTS`).
No `filter` da URL, `@request` e `@collection` são só do superusuário.
`expand` direto, aninhado e reverso já responde em listar, ver, criar e
atualizar. Coleção `view` (`"type": "view"`, `"viewQuery": "SELECT id, …"`)
lista e mostra; criar, atualizar e apagar registro nela falham.
*(planejado)* `fields`, operadores `?=`, as demais macros `@request`,
`PATCH` de coleção, e a URL por projeto (`https://host/p/loja`).

## 6. Autenticação

Superusuário: `auth-with-password` e `auth-refresh` em `_superusers` (seção 5),
token no cabeçalho `Authorization` (puro ou com `Bearer `). Usuários de coleções
`auth` usam as mesmas rotas na própria coleção:

```sh
curl -X POST http://127.0.0.1:8090/api/collections/users/auth-with-password \
  -H "Content-Type: application/json" \
  -d '{"identity":"ana@exemplo.com","password":"..."}'
# { "token": "...", "record": { ... } }
```

Criar uma coleção `auth` injeta os campos de sistema (`email`, `emailVisibility`,
`verified`, `password`, `tokenKey`); o cadastro é o `POST` normal em `/records`
quando a `createRule` permite. Verificação de e-mail, redefinição de senha e
troca de e-mail enviam o link por e-mail — no desenvolvimento ele sai no stderr
do servidor (`[email] ... confirm-verification?token=...`), sem SMTP.

```text
POST /api/collections/users/request-verification      {"email": "..."}
POST /api/collections/users/confirm-verification      {"token": "..."}
POST /api/collections/users/request-password-reset    {"email": "..."}
POST /api/collections/users/confirm-password-reset    {"token": "...", "password": "...", "passwordConfirm": "..."}
POST /api/collections/users/request-email-change      {"newEmail": "..."}   (autenticado)
POST /api/collections/users/confirm-email-change      {"token": "...", "password": "..."}
POST /api/collections/users/impersonate/<id>          {"duration": 3600}    (superusuário)
```

Trocar a senha (ou confirmar a troca de e-mail) rotaciona a `tokenKey` e invalida
os tokens já emitidos. Nas regras, `@request.auth.*` vale o registro autenticado;
para visitantes, vazio.

OTP, MFA e OAuth2 (30 provedores) seguem as mesmas rotas do PocketBase
(FUNCIONALIDADES §5).

## 7. Arquivos e tempo real *(planejado)*

```text
GET /api/files/produtos/<id>/foto.jpg?thumb=200x200
GET /api/realtime            (Server-Sent Events; assinar "produtos" ou "produtos/<id>")
```

## 8. SDKs *(planejado)*

Enquanto os SDKs do Forja não saem, os do PocketBase funcionam com o projeto
padrão. Conferido com o cliente Python (`pip install pocketbase`):

```python
from pocketbase import PocketBase
pb = PocketBase("http://127.0.0.1:8090")
pb.collection("_superusers").auth_with_password("admin@exemplo.com", "uma-senha-forte")
pb.collection("produtos").get_list(1, 20, {"filter": "preco < 50", "sort": "-created"})
```

Os SDKs do Forja vão receber a URL base do projeto.

**JavaScript / TypeScript** (React, Vue, Angular, Svelte, Node, Deno, Bun):

```ts
import { Forja } from "@forjabase/sdk";

const forja = new Forja("https://host/p/loja");
await forja.collection("users").authWithPassword("ana@exemplo.com", "...");
const lista = await forja.collection("produtos").getList(1, 20, {
  filter: forja.filter("preco < {:max}", { max: 50 }),
});
forja.collection("produtos").subscribe("*", (e) => console.log(e.action, e.record));
```

React (`useList`, `useAuth`, `useRealtime`), Vue (composables), Angular
(serviço) e Svelte (stores) vêm em pacotes finos sobre esse SDK.

**Kotlin (Android)**

```kotlin
val forja = Forja("https://host/p/loja")
forja.collection("users").authWithPassword("ana@exemplo.com", "...")
val produtos = forja.collection("produtos").getList(page = 1, perPage = 20)
```

**Swift (iOS)**

```swift
let forja = Forja(url: "https://host/p/loja")
try await forja.collection("users").authWithPassword("ana@exemplo.com", "...")
let produtos = try await forja.collection("produtos").getList(page: 1, perPage: 20)
```

**Dart / Flutter**

```dart
final forja = Forja('https://host/p/loja');
await forja.collection('users').authWithPassword('ana@exemplo.com', '...');
```

**Rust, Python, Java, C# e Go** seguem o mesmo formato: `Forja(url)`,
`collection(nome)`, métodos de autenticação, CRUD, arquivos e assinaturas.

## 9. MCP para agentes de IA *(planejado)*

### Local, pela CLI

Só quem tem acesso à pasta de dados consegue rodar. Somente leitura por padrão.

```json
{
  "mcpServers": {
    "forja-loja": {
      "command": "forja",
      "args": ["mcp", "--project", "loja", "--dir", "/caminho/forja_data"]
    }
  }
}
```

Para permitir escrita numa coleção:
`forja mcp --project loja --allow-write produtos`.

### Remoto, por HTTP

Desligado até existir um token. Criar um token com escopo mínimo:

```sh
forja tokens create --project loja --mcp --read produtos,pedidos --expires 30d
```

E apontar o cliente MCP para `https://host/p/loja/mcp` com o cabeçalho
`Authorization: Bearer <token>`.

O que o MCP **não** faz, por desenho: SQL bruto, configurações, backups e
superusuários. As regras de acesso das coleções valem para o token, cada
chamada fica no registro de auditoria e exclusões em massa pedem confirmação
(FUNCIONALIDADES §13).

## 10. CLI de administração *(planejado)*

```sh
forja login --url https://host/p/loja          # guarda o perfil
forja collections list
forja records list produtos --filter "preco < 50" --sort -created --json
forja records create produtos --data '{"nome":"Café","preco":32}'
forja backups create
forja logs tail
```

## 11. Extensões em WebAssembly *(planejado)*

Um módulo WASM registra hooks, rotas e tarefas agendadas. Em Rust:

```rust
use forja_ext::*;

#[hook(on_record_create = "pedidos")]
fn ao_criar_pedido(e: &mut RecordEvent) -> Result<()> {
    if e.record.get_f64("total")? <= 0.0 {
        return Err(Error::bad_request("total precisa ser positivo"));
    }
    e.next()
}
```

```sh
forja ext add ./pedidos.wasm --project loja
```

Cada módulo roda isolado, com limite de memória e de tempo.

## 12. Contribuir (funciona)

```sh
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test
```

Estrutura do código em [`PLANO.md`](PLANO.md) §2. Mudanças de comportamento
entram em `CHANGELOG.md`, e cada funcionalidade entregue muda de 📋 para ✅
em `FUNCIONALIDADES.md`.
