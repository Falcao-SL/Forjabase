# Plano — Forja

## 1. O que é

Um backend em um único executável — banco SQLite, autenticação, arquivos,
tempo real e painel administrativo — reescrito em **Rust e WebAssembly** a
partir do PocketBase (Gani Georgiev, MIT). Código aberto, licença MIT.

O que muda em relação ao PocketBase:

1. **Rust** no servidor (axum + SQLite embutido) e **WebAssembly** no painel
   (Leptos) e nas extensões (wasmtime), no lugar de Go, Svelte e JavaScript.
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
    forja-admin    painel em Leptos → wasm32, embutido no executável
    forja          o executável: CLI (serve, superuser, migrate, update)
  sdks/            núcleo forja-sdk em Rust e as camadas por linguagem (futuro)
```

- **Banco:** SQLite embutido (rusqlite, `bundled`), modo WAL. Um diretório por
  projeto.
- **Painel:** compilado com `trunk` para `wasm32-unknown-unknown` e embutido
  com `include_dir`/`rust-embed` em `/_/`.
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

## 5. Fases e versões

Versões **0.x** até estar estável; a 1.0.0 só depois dos critérios do §6.

| Versão | Entrega |
|---|---|
| **0.0.1** | Workspace, CLI `serve`, `/api/health`, tipos do modelo, casca do painel em WASM, CI, documentação |
| 0.1 | SQLite, coleções base com CRUD de registros, filtros e ordenação, superusuário |
| 0.2 | Autenticação (senha, verificação, redefinição, troca de e-mail), regras de acesso |
| 0.3 | Arquivos (local e S3, miniaturas), tempo real (SSE), lote |
| 0.4 | Painel administrativo utilizável |
| 0.5 | Vários projetos |
| 0.6 | OAuth2, OTP, MFA, personificação, alertas |
| 0.7 | Backups, logs, cron, configurações, limite de requisições; CLI de administração |
| 0.8 | Extensões em WASM (hooks, rotas, cron, migrações); MCP local e remoto com tokens de escopo |
| 0.9 | SDKs: Rust e JS/TS primeiro; depois Kotlin, Swift, Dart; depois Python, Java, C#, Go |
| 1.0.0 | Estável (§6) |

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
