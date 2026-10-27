# Plano — Forja

## 1. O que é

Um backend em um único executável — banco SQLite, autenticação, arquivos,
tempo real e painel administrativo — reescrito em **Rust, com WebAssembly onde
ele ajuda**, a partir do PocketBase (Gani Georgiev, MIT). Código aberto, licença
MIT.

O que muda em relação ao PocketBase:

1. **Rust** no servidor (axum + SQLite embutido), **HTML no servidor + htmx** no
   painel e **WebAssembly** nas extensões (wasmtime), no lugar de Go, Svelte e
   JavaScript.
2. **Vários projetos** numa só instância, isolados (FUNCIONALIDADES §10).
3. **SDKs para 9 linguagens** a partir de um núcleo único em Rust (§12).
4. **MCP** seguro para agentes de IA e **CLI de administração** completa (§13 e §14).

A lista completa está em [`FUNCIONALIDADES.md`](FUNCIONALIDADES.md).

## 2. Arquitetura

```
forjabase/
  crates/
    forja-core     modelo (coleções, campos, regras), filtros, validação — sem HTTP
    forja-server   axum: rotas no formato da API do PocketBase, tempo real (SSE)
    forja-admin    painel: HTML no servidor (maud) + htmx, embutido no executável
    forja          o executável: CLI (serve, superuser, migrate, update)
  sdks/            núcleo forja-sdk em Rust e as camadas por linguagem (futuro)
```

- **Banco:** `store::Banco` concentra coleções, registros, regras e validação
  sobre um `motor::Executor` que só executa SQL. O motor suportado é o
  **SQLite** (rusqlite, `bundled`, WAL, um arquivo por projeto). Postgres e
  Turso compilam com `--features postgres` / `--features turso` e não têm
  suporte; quando cada um existe está no
  [ADR-001](arquitetura/ADR-001-um-binario.md). O SQL sai com `?`; o executor
  do Postgres, se compilado, troca por `$n`.
- **Painel:** HTML gerado no servidor com `maud` e htmx embutido via
  `include_bytes!`, servido em `/_/` pelo mesmo processo e estado da API. Sem
  `trunk` e sem `wasm32` no painel; o WASM fica para as extensões.
- **Extensões:** módulos WASM (component model, wasmtime) com uma interface
  WIT para hooks, rotas e cron; limites de memória e tempo por módulo.
- **SDKs:** `forja-sdk` (Rust) → wasm-bindgen (JS/TS), UniFFI (Kotlin, Swift,
  Python), geradores UniFFI de terceiros (C#, Go), Kotlin/JVM (Java),
  flutter_rust_bridge (Dart).

## 3. Vários projetos

- Endereço por caminho `https://host/p/<projeto>` como padrão: os SDKs só
  recebem a URL base, então continuam compatíveis.
- Projeto `default` na raiz (`/api/...`), para clientes do PocketBase funcionarem
  sem mudança.
- Domínio ou subdomínio mapeado para projeto, opcional.
- Isolamento por diretório (`data/<projeto>/data.db`, `auxiliary.db`,
  `storage/`), pool de conexões por projeto aberto sob demanda.

## 4. Decisões em aberto

| # | Decisão | Recomendação |
|---|---|---|
| 1 | Compatibilidade de **API** com o PocketBase | Sim: rotas, corpos e erros no mesmo formato. É o que deixa SDKs e ferramentas existentes funcionarem |
| 2 | Compatibilidade do **arquivo** `pb_data` | Meta da 0.x tardia: importar `pb_data` como projeto; não prometer abrir o mesmo arquivo nos dois ao mesmo tempo |
| 3 | SDKs: núcleo Rust + bindings, ou SDKs escritos à mão em cada linguagem | Núcleo Rust: um comportamento só, um lugar para corrigir. Revisar se o tamanho do WASM do SDK JS passar de ~300 KB comprimido |
| 4 | Extensões: só WASM, ou também JavaScript | Só WASM no começo, coerente com "Rust e WebAssembly"; JS seria um módulo WASM com um motor JS dentro, se houver demanda |
| 5 | Nome dos pacotes nos registros (crates.io, npm, PyPI, NuGet, Maven, pub.dev) | `forja` / `forja-sdk` / `@forjabase/sdk`; reservar cedo |
| 6 | MCP remoto ligado por padrão? | Não: desligado até o superusuário criar o primeiro token de MCP; o local (`forja mcp`) só funciona com acesso ao diretório de dados |
| 7 | Só SQLite ou também PostgreSQL | Os dois: **SQLite padrão** (um arquivo, nada para manter) e **PostgreSQL opcional por projeto**, para escrita concorrente alta, vários servidores no mesmo banco ou Postgres gerenciado. Desde a 0.0.2 o armazenamento fica atrás do trait `Banco` e os filtros viram SQL por dialeto (`?`/`LIKE` no SQLite, `$1`/`ILIKE` no Postgres); desde a 0.0.3 o Postgres funciona e passa a mesma bateria de conformidade. Um projeto Supabase é um Postgres: basta a URL dele. ⚠️ **Superada** pela decisão 1 de [ARQUITETURA](ARQUITETURA.md): SQLite é o único motor suportado; Postgres fica atrás de feature flag, sem suporte |
| 8 | Turso | **Terceiro motor**, mesmo dialeto do SQLite: abre o arquivo que o rusqlite gravou, então trocar de motor não exige migração. Fica opcional enquanto o Turso for beta; vira candidato a padrão quando estabilizar. Recursos dele (CDC, vetores, criptografia, MVCC) entram como extras sobre a mesma API. ⚠️ **Superada** pela decisão 13 de [ARQUITETURA](ARQUITETURA.md): Turso permanece no código atrás de feature flag, sem suporte nem CI |
| 9 | Até onde ir no "Supabase" | A API continua a do PocketBase em todos os motores. Do Supabase vêm ideias para o motor Postgres: regras também como RLS, tempo real pela replicação lógica, pgvector. Não reimplementar PostgREST nem GoTrue: a API do Forja já cobre esses papéis |

## 5. Fases e versões

Versões **0.x** até estar estável; a 1.0.0 só depois dos critérios do §6.

| Versão | Entrega |
|---|---|
| **0.0.x** | Entregue: workspace, CLI `serve`, `/api/health`, coleções base com CRUD e filtros, regras de acesso, superusuário; SQLite (Postgres e Turso atrás de feature flag) |
| 0.1 | **Autenticação**: coleções `auth`, senha, verificação, redefinição, troca de e-mail, personificação, macros `@request.auth.*`, envio de e-mail — ✅ feito (SMTP fica na 0.5) |
| 0.2 | **Relações e views**: campo `relation`, cascata, `expand`, relações reversas, coleções `view`, macro `@collection.*` |
| 0.3 | **Arquivos, lote e tempo real**: campo `file`, local/S3, miniaturas, arquivos protegidos, `/api/batch`, SSE |
| 0.4 | **Painel administrativo utilizável** — 🔨 primeiro corte: login, lista de coleções, navegador e CRUD de registros |
| 0.5 | **Operação**: configurações, logs, cron, limite de requisições, backups com restauração, encerramento gracioso |
| 0.6 | **OAuth2, OTP e MFA** |
| 0.7 | **Vários projetos** e importação de um `pb_data` |
| 0.8 | **Extensões em WASM** (hooks, rotas, cron, migrações) e **MCP** com tokens de escopo |
| 0.9 | **SDKs gerados** a partir do OpenAPI, conformidade contra o PocketBase, TLS e builds multiplataforma |
| 1.0.0 | Estável (§6) |

Portão de saída e tamanho de cada onda: [ARQUITETURA](ARQUITETURA.md) §4.

## 6. Quando vira 1.0.0

- Todas as funcionalidades marcadas 📋 em FUNCIONALIDADES §1–§9 entregues.
- Suíte de compatibilidade passando contra o PocketBase para as rotas públicas.
- Formato de dados e API sem mudança que quebre por pelo menos dois lançamentos
  0.x seguidos.
- Executáveis para macOS, Linux e Windows e imagem Docker publicados.
- SDKs de JS/TS, Kotlin, Swift e Dart em versão estável.

## 7. Créditos

O Forja é uma reimplementação inspirada no PocketBase (© Gani Georgiev, MIT).
Onde houver tradução direta de código, a origem fica comentada no arquivo e a
licença MIT do PocketBase é mantida.
