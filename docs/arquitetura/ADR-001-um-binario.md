# ADR-001 — Um binário, no espírito do PocketBase

- **Estado:** aceito
- **Data:** 2026-10-27
- **Supersede:** as decisões 7, 8 e 9 do [PLANO](../PLANO.md) §4 (já marcadas lá). Em divergência com [ARQUITETURA](../ARQUITETURA.md), [PARIDADE](../PARIDADE.md) ou o PLANO, vale este ADR.

## Contexto

O Forja é um BaaS auto-hospedado: `forja serve` sobe API, painel e arquivos no mesmo processo. A inspiração de produto é o PocketBase (um executável, SQLite, admin no mesmo processo, REST). A implementação é Rust. O painel é HTML no servidor com htmx, não uma SPA.

Appwrite e Supabase entram como **cardápio de capacidades** (auth, banco, arquivos, tempo real, funções leves), não como arquitetura. Os dois são uma nuvem de serviços. Copiar isso quebra a tese do binário único.

Premissas que esta decisão não reabre:

- O cliente Python do PocketBase já fala com o Forja. O contrato público que funciona é o do PocketBase.
- `forja-admin` já não é casca Leptos. É HTML em `maud`, com htmx embutido, login por cookie e CRUD de registros. Ver [CHANGELOG](../../CHANGELOG.md), seção "Não lançado".
- Postgres e Turso já compilam atrás de feature flag. Tratá-los como motores pares obrigaria cada recurso novo a três dialetos.

## Camadas

Um processo. Quatro crates. Nada de serviço ao lado.

```
                    forja  — um processo, um binário
                    CLI: serve | superuser | …
                              │
        ┌─────────────────────┼──────────────────────┐
        │                     │                      │
   /api/*  REST          /_/  painel HTMX       estático
   formato PocketBase    cookie httpOnly        --publicDir
        │                     │                  (quando existir)
        └──────────┬──────────┘
                   │
            forja-server (axum)
            rotas, JWT, erros, SSE
                   │
     ┌─────────────┼──────────────────┐
     │             │                  │
 forja-admin    forja-core         wasmtime (0.8)
 só Markup      sem HTTP           no mesmo processo
                │
                ├─ Banco: regras compilam para SQL
                ├─ auth: Argon2id, JWT
                ├─ correio: trait (console hoje, SMTP na 0.5)
                ├─ Executor: SQLite suportado
                │            postgres | turso só com feature, sem suporte
                ├─ arquivos (0.3): disco local ou S3, mesmo processo
                └─ barramento (marco M1): broadcast in-process
                   │
              forja_data/<projeto>/
                data.db    storage/    extensões .wasm
```

| Crate | Faz | Não faz |
|---|---|---|
| `forja` | Binário, CLI, sobe o servidor | Regra de negócio, HTML |
| `forja-server` | axum: `/api`, `/_/`, estático | SQL, markup de tela |
| `forja-admin` | Funções `-> Markup` (página e fragmento htmx) | axum, disco, segredo, cookie |
| `forja-core` | Coleções, registros, filtros, auth, correio, motor | HTTP |

O layout atual já é o alvo. Não se cria `forja-auth`, `forja-storage` nem `forja-realtime`: isso simula microsserviço dentro do processo. O único crate novo permitido, e só na 0.8 se o `wasmtime` pesar o `forja-core`, é um host `forja-ext` chamado pelo servidor. Até lá, zero crate novo. O trait de gancho mora no core, com implementação vazia; o wasmtime mora no servidor.

## Decisão

### HTTP — axum

`axum` 0.8 sobre tokio, com `tower-http` para arquivo estático. Já é o servidor. Actix não compra nada aqui. Não há segundo listener: API, painel, htmx, SSE e estático dividem a mesma porta (8090, igual ao PocketBase).

### Persistência — SQLite no binário; Postgres e Turso não são produto

| Motor | Quando usar | Suporte |
|---|---|---|
| **SQLite** (rusqlite `bundled`, WAL, um `data.db` por projeto) | Sempre. É o produto: backup é copiar o arquivo, não há serviço para instalar. | Sim. Único motor do CI e da 1.0. |
| **Postgres** (`--features postgres`, `FORJA_PG_URL`) | Só se o operador **já tem** Postgres (inclusive a URL de um projeto Supabase) e aceita sair da história de um arquivo. Não é caminho para "falar a API do Supabase". | Não. Sem CI, sem promessa. A feature existe para a costura `Executor` não apodrecer no git. SQL novo pode usar `json_extract`, FTS5 e a API de backup do SQLite sem portar. |
| **Turso** (`--features turso`) | Só para abrir o **mesmo arquivo** com libSQL, ou experimentar escrita concorrente. Não vira padrão enquanto o SQLite embutido resolver o caso. | Não. Mesma regra do Postgres. |

Réplica é recuperação (backup, no futuro Litestream), não cluster. Um escritor por vez no SQLite é limite de produto, escrito por extenso, não defeito escondido.

### Admin — maud + htmx, sessão por cookie

O painel continua em **maud**. Cada tela é uma função que devolve `Markup`; fragmento htmx é a mesma coisa sem o documento inteiro (a tabela em `/_/collections/{nome}/tabela` já é isso). htmx vai em `include_bytes!` e sai em `/_/htmx.min.js`. Não há `trunk`, `wasm32` nem bundler no painel.

| Opção | Por que não |
|---|---|
| **askama** | Também é HTML em tempo de compilação, mas em arquivo separado. O painel é código do produto, não tema de terceiros. Trocar reescreve o que já funciona. |
| **minijinja** | Interpreta em runtime. Serve se um dia o usuário editar modelo de e-mail (0.5). Não serve para o admin embutido. |

Sessão: cookie `forja_painel`, `HttpOnly`, `SameSite=Strict`, valor = JWT de superusuário, `Path=/`. A API pública segue em `Authorization` (token puro ou `Bearer`). O cookie não é o login dos SDKs. Quando houver TLS (0.9), o cookie ganha `Secure` em HTTPS.

Regra para tela nova: markup em `forja-admin`, rota em `forja-server/src/painel.rs`. Sem SPA.

### API — contrato do PocketBase, não uma v1 própria

A API pública é a do PocketBase em `/api/...`: rotas, JSON, códigos de erro, filtros, regras e auth. Se o SDK oficial dele passa, a compatibilidade é real.

| Opção | Efeito |
|---|---|
| **Compatível com o PocketBase** (escolha) | SDKs, tutoriais e o cliente que já testamos continuam válidos. O custo é herdar as peculiaridades dele e não versionar por conta própria. |
| **`/api/v1` própria** | Liberdade de desenho. Joga fora o único contrato provado e obriga SDK antes de haver produto. |
| **As duas** | Dois contratos para sempre. Cada campo novo nasce duas vezes. |

Não há `/rest/v1` (PostgREST) nem `/auth/v1` (GoTrue). Capacidade que o PocketBase não tem entra como rota **aditiva** que não colide com a dele, ou como coleção de sistema, e leva teste próprio. Não se muda o formato de uma rota existente para "ficar mais parecida com o Supabase".

OpenAPI, na 0.9, documenta essa superfície. Até lá o cliente de teste é o SDK do PocketBase, não um SDK nosso.

### Tempo real — SSE, não WebSocket

SSE no protocolo do PocketBase: o cliente abre `GET /api/realtime` e manda as assinaturas em `POST`. O evento só sai se a regra de acesso da coleção deixar aquele registro visível.

WebSocket seria o modelo do Appwrite e do Supabase (canal nos dois sentidos, em geral ligado à replicação do banco). Aqui a assinatura já cabe num POST, o dado só desce, e o SDK do PocketBase fala SSE. Um segundo transporte dobra proxy, teste e cliente. WebSocket fica de fora.

O fan-out é in-process (`tokio::broadcast`), não Redis e não o log do Postgres.

### Funções — WASM no processo, não subprocesso

"Functions" no Forja são ganchos, rotas próprias e cron, no espírito das JSVM hooks do PocketBase e das edge functions do Supabase, com outro mecanismo.

| Opção | Efeito |
|---|---|
| **wasmtime no mesmo processo** (escolha, 0.8) | Um binário. O convidado não vê disco nem rede fora da ABI. Limite de memória e de combustível por chamada. Qualquer linguagem que compile para WASM. |
| **Subprocesso** (node, python, deno, container) | Exige runtime instalado ao lado. É o Appwrite (vários executores) e a edge do Supabase (isolate Deno). Quebra "nada para instalar". |
| **JS embutido** (goja, boa, quickjs) | Um segundo contrato de extensão. Se JS fizer falta, entra como módulo WASM com um motor dentro, sobre a mesma ABI — não como caminho paralelo. |

Cron e jobs leves também ficam no processo: tabela no SQLite + tarefa tokio. Sem fila, sem worker, sem Redis.

A ABI começa em JSON, versionada, com poucos ganchos (antes/depois de criar, atualizar, apagar, autenticar). Não se congela um component model completo no primeiro dia.

## Núcleo e extensão

O núcleo é o que `forja serve` (features padrão) promete até a 1.0. Extensão é feature flag, rota aditiva ou onda tardia — não um segundo produto.

| Capacidade (Appwrite / Supabase) | No núcleo | Fora do núcleo |
|---|---|---|
| Auth e-mail e senha, verificação, reset, troca de e-mail | Sim (feito) | — |
| OAuth2, OTP, MFA, magic link, sessões revogáveis | Onda 0.6, mesmas rotas do PocketBase; magic link é rota aditiva | Provedor de SMS; Sign in with Apple só o fluxo OAuth, não uma infra de push |
| Coleções, registros, filtros, regras | Sim. Regra é filtro que vira `WHERE` | RLS do Postgres, ACL de equipe como segundo motor de permissão, GraphQL, PostgREST |
| Relação, `expand`, views | Onda 0.2 | — |
| Arquivos | Onda 0.3: campo `file`, disco local, miniaturas, URL assinada. S3 compatível é **driver** no mesmo processo, não serviço | Bucket como produto separado; CDN |
| Tempo real | Onda 0.3: SSE in-process | WebSocket, replicação lógica, CDC do Turso |
| Jobs / cron | Onda 0.5: agenda no SQLite, corre no processo | Fila, worker dyno, Redis |
| Functions | Onda 0.8: WASM in-process | Deno, Docker, dez runtimes, subprocesso |
| Postgres / Turso | Feature de compilação, sem suporte | Vetor, RLS, realtime pelo log do banco |
| Vários projetos | Onda 0.7: um diretório por projeto, prefixo `/p/<projeto>`, projeto `default` na raiz | Control plane de nuvem, billing por projeto |
| E-mail transacional | Trait `Correio`; SMTP na 0.5. Auth depende disso | SMS, push, tópicos, campanha |
| MCP para agentes | Onda 0.8, só leitura por padrão, sem SQL bruto | — |

## O que fica de fora de propósito

Isto não é backlog. É o que a arquitetura recusa para não virar nuvem.

- **Compose do Appwrite:** MariaDB, Redis, Traefik, workers e um container por runtime.
- **Fatia do Supabase:** GoTrue, PostgREST, Realtime em Elixir, Storage à parte, Kong, Studio em Next.js, edge em Deno, Logflare.
- **GraphQL** e qualquer segunda API de dados.
- **Cluster, multi-região, eleição de líder, escala horizontal.** A resposta oficial é subir a máquina.
- **Hospedagem de sites, domínio e firewall** (Appwrite Sites). Servir uma pasta pública, como o `--publicDir` do PocketBase, não é isso e pode entrar.
- **Mensageria de plataforma** (SMS, push, fila). E-mail transacional fica.
- **Função em linguagem arbitrária** via subprocesso.
- **Permissão em dois sistemas.** Ou a regra compila para SQL, ou não existe. Macro `@request.auth.*` (e, mais tarde, times como dados dentro dessa regra) não é uma ACL paralela.
- **Busca vetorial, pgvector, criptografia de disco do Turso** como promessa do núcleo.

Transformação de imagem (miniatura) fica: é arquivo, onda 0.3, no processo.

## Consequências

- Recurso novo nasce no SQLite e na API do PocketBase. Se não couber nos dois, não entra no núcleo.
- O binário padrão não liga `postgres` nem `turso`. Quem passar `--features postgres` está fora do suportado.
- O painel não volta a ser SPA. Extensão de UI do admin, se existir, é fragmento HTML, não pacote npm.
- Tempo real e ganchos WASM só enxergam escrita que passou pelo barramento (marco M1). Atalho que grava SQL direto não pode ir para produção.
- Fica mais difícil dizer "compatível com Supabase" em landing. O que se pode dizer: as mesmas famílias de recurso (auth, dados, arquivo, tempo real, função), num arquivo só, na API do PocketBase.

## Marco seguinte — M1, barramento de escrita

A 0.2 (relação, cascata, `expand`, view, `@collection.*`) fechou em 2026-10-27. O marco de **arquitetura** que entrou antes da cascata espalhar escrita foi:

1. `forja-core` ganha um `Barramento` (`tokio::broadcast`) no `Banco`.
2. `criar`, `atualizar*` e `apagar*` publicam `Evento { colecao, acao, id }` depois do commit.
3. Um teste prova que o assinante recebe o evento e que assinante lento ou ausente não desfaz a escrita.
4. A cascata da 0.2 publica pelo mesmo caminho, um evento por registro afetado.
5. Nenhum crate novo, nenhuma rota SSE ainda (isso é a 0.3).

Critério de pronto: não existe inserção, atualização ou exclusão de registro em `store.rs` que não passe por essa publicação.
