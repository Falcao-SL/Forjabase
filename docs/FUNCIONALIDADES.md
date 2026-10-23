# Funcionalidades — Forja

Lista de tudo o que o Forja vai ter. A base é o PocketBase (o fork
`iramarfalcao/pocketbase`, igual ao `pocketbase/pocketbase` em 2026-10-21): cada
item dele está aqui, com o equivalente no Forja. Depois vêm o que o Forja
acrescenta (vários projetos, extensões em WebAssembly) e os SDKs.

**Legenda:** ✅ feito · 🔨 em construção · 📋 planejado · ➕ só no Forja · ⚠️ experimental, sem suporte

Versão atual: **0.0.2**. O Forja só vira **1.0.0** quando estiver estável
(critérios em [`PLANO.md`](PLANO.md) §6).

---

## 1. Executável único e linha de comando

| Funcionalidade | Estado |
|---|---|
| Um único executável, sem dependências externas (SQLite embutido) | 🔨 |
| `forja serve` — sobe o servidor (porta padrão 8090, como o PocketBase) | ✅ |
| `forja --version` | ✅ |
| `serve` com HTTPS automático (Let's Encrypt) ao receber um domínio | 📋 |
| `forja superuser upsert` (criar ou trocar a senha) | ✅ |
| `forja superuser create | update | delete | otp` | 📋 |
| `forja migrate up | down | create | collections | history-sync` | 📋 |
| Migrações automáticas ao mudar coleções pelo painel (automigrate) | 📋 |
| `forja update` — atualização do próprio executável pelo GitHub Releases | 📋 |
| Instalador do primeiro superusuário (link de uso único aberto no navegador) | 📋 |
| Pasta de dados configurável (`--dir`, padrão `forja_data`) | ✅ |
| Pasta pública (`--publicDir`) e origens CORS (`--origins`) | 📋 |
| Modo de desenvolvimento (`--dev`): log de SQL e erros detalhados | 📋 |
| Encerramento gracioso | 📋 |

## 2. Banco de dados

> **Decisão de arquitetura:** SQLite é o único motor suportado. Turso e
> PostgreSQL continuam compilando atrás de feature flag, mas não têm suporte,
> CI nem promessa de compatibilidade — ver [ARQUITETURA](ARQUITETURA.md).

| Funcionalidade | Estado |
|---|---|
| SQLite em modo WAL, um arquivo por projeto (`forja_data/<projeto>/data.db`) | ✅ |
| ➕ Três motores com a mesma API, escolhidos por projeto (`--engine sqlite\|turso\|postgres`) | ✅ (só SQLite suportado) |
| ➕ Bateria de conformidade: os mesmos testes passam nos três motores (o CI roda o Postgres) | ✅ (histórico; o CI hoje roda só SQLite) |
| Banco separado para logs (`auxiliary.db`) | 📋 |
| Consultas preparadas e transações | ✅ |
| Pool de conexões | 📋 |
| Índices (simples e únicos) definidos por coleção | 📋 |
| Endpoint de SQL para superusuários (`/api/sql`) | 📋 |
| Compatibilidade do arquivo com o PocketBase (abrir um `pb_data` existente) | 📋 (meta, ver PLANO §4) |
| ➕ PostgreSQL como banco opcional por projeto (mesma API) | ⚠️ (sem suporte) |

### 2.1 Motores

| Motor | Para quê | Estado |
|---|---|---|
| **SQLite** (rusqlite) | O motor. Um arquivo, nada para manter | ✅ |
| **Turso** ([tursodatabase/turso](https://github.com/tursodatabase/turso), Rust) | Experimental, sem suporte: o mesmo arquivo do SQLite, útil para testar escrita concorrente | ⚠️ |
| **PostgreSQL / Supabase** ([supabase/supabase](https://github.com/supabase/supabase)) | Experimental, sem suporte: existia para escrita concorrente alta e Postgres gerenciado | ⚠️ |

### 2.2 Recursos próprios de cada motor (sobre a mesma API)

| Funcionalidade | Motor | Estado |
|---|---|---|
| ➕ Regras de acesso também como Row Level Security do Postgres (como o Supabase), para quem acessa o banco direto | Postgres | 📋 |
| ➕ Tempo real a partir do log do banco (replicação lógica, como o Supabase Realtime) | Postgres | 📋 |
| ➕ Tempo real a partir da captura de mudanças (CDC) do Turso | Turso | 📋 |
| ➕ Campo `vector` e busca por similaridade (pgvector no Postgres, vetores nativos no Turso) | Postgres, Turso | 📋 |
| ➕ Criptografia do arquivo em repouso | Turso | 📋 |
| ➕ Escrita concorrente (MVCC, `BEGIN CONCURRENT`) | Turso | 📋 |
| ➕ Usar o Auth/Storage de um projeto Supabase existente | Postgres | 📋 (avaliar) |

## 3. Coleções

| Funcionalidade | Estado |
|---|---|
| Tipo **base** | ✅ |
| Tipo **auth** (campos de sistema, e-mail único sem diferenciar maiúsculas e senha em Argon2id) | ✅ |
| Tipo **view** (consulta SQL somente leitura) | ✅ (modelo) · 📋 (comportamento) |
| Listar, criar, ver e apagar coleções (`/api/collections`) | ✅ |
| Atualizar coleção (`PATCH`, com mudança de campos) | 📋 |
| Importar coleções em lote (`PUT /api/collections/import`) | 📋 |
| Esvaziar coleção (`DELETE /api/collections/{c}/truncate`) | 📋 |
| Modelos prontos de coleção (`/api/collections/meta/scaffolds`) | 📋 |
| Testar a consulta de uma view antes de salvar (`dry-run-view`) | 📋 |
| Coleções de sistema: `_superusers`, `_externalAuths`, `_mfas`, `_otps`, `_authOrigins` | 📋 |
| Renomear campos e coleções sem perder dados | 📋 |

### 3.1 Tipos de campo

| Tipo | O que guarda | Estado |
|---|---|---|
| `text` | texto, com mínimo, máximo, padrão (regex) e valor gerado automaticamente | ✅ |
| `editor` | HTML rico (com sanitização) | ✅ |
| `number` | número, com mínimo, máximo e "só inteiros" | ✅ |
| `bool` | verdadeiro/falso | ✅ |
| `email` | e-mail, com domínios permitidos ou bloqueados | ✅ |
| `url` | URL, com domínios permitidos ou bloqueados | ✅ |
| `date` | data e hora UTC, com mínimo e máximo | ✅ |
| `autodate` | data de criação ou atualização, preenchida sozinha | ✅ (tipo) |
| `select` | um ou vários valores de uma lista | ✅ |
| `file` | um ou vários arquivos, com tipos MIME, tamanho e miniaturas | ✅ (tipo) |
| `relation` | um ou vários registros de outra coleção, com exclusão em cascata | ✅ (tipo) |
| `json` | qualquer JSON, com tamanho máximo | ✅ |
| `password` | senha com hash (Argon2id), nunca devolvida nem filtrável | ✅ |
| `geoPoint` | latitude e longitude | ✅ (tipo) |

"✅ (tipo)" quer dizer que o tipo existe no modelo e serializa com o mesmo nome
do PocketBase, mas ainda não guarda valores: a coleção que o usa é recusada
com uma mensagem clara. "✅" quer dizer validado e guardado.

### 3.2 Regras de acesso

| Funcionalidade | Estado |
|---|---|
| Regras por operação: listar, ver, criar, atualizar, apagar, com filtros sobre os campos do registro | ✅ |
| Regras de auth: login (`authRule`) e gerenciar outros usuários (`manageRule`) | 📋 |
| Regra nula = só superusuário; `""` = público | ✅ |
| Regras com `@request.auth.*` | ✅ |
| Regras com `@request.body.*`, `@request.query.*`, `@request.headers.*`, `@request.context` e `@collection.*` | 📋 |

## 4. Registros (CRUD) e consulta

| Funcionalidade | Estado |
|---|---|
| Listar com paginação (`page`, `perPage`, `skipTotal`) | ✅ |
| Ver, criar, atualizar e apagar registros (JSON) | ✅ |
| **Filtros** com `=`, `!=`, `>`, `>=`, `<`, `<=`, `~` (contém), `!~`, `&&`, `||`, parênteses, textos, números, `true`, `false` e `null`, sempre como parâmetros de SQL | ✅ |
| Operadores "algum item" (`?=`, `?!=`, `?>`, `?~`…) | 📋 |
| Macros de data (`@now`, `@yesterday`, `@todayStart`, `@monthEnd`…) | 📋 |
| Modificadores `:isset`, `:changed`, `:length`, `:each`, `:lower` | 📋 |
| Funções `geoDistance()` e `strftime()` | 📋 |
| Ordenação (`sort`, com `-` e `@random`) | ✅ |
| Ordenação por `@rowid` | 📋 |
| Expandir relações (`expand`, inclusive aninhadas e reversas `coleção_via_campo`) | 📋 |
| Escolher campos (`fields`, com `:excerpt`) | 📋 |
| Modificadores de valor no envio (`campo+`, `+campo`, `campo-`) | 📋 |
| Upload de arquivos em `multipart/form-data` | 📋 |
| **Lote** (`/api/batch`): várias operações numa transação | 📋 |
| Validação com mensagens de erro por campo, no formato do PocketBase | ✅ |
| Campos `hidden` só para superusuários | ✅ |

## 5. Autenticação

| Funcionalidade | Estado |
|---|---|
| Superusuário com e-mail e senha (`_superusers/auth-with-password`), senha em Argon2id | ✅ |
| Usuários de coleções auth com e-mail (ou outro campo de identidade) e senha | ✅ |
| **OTP** por e-mail (código de uso único) | 📋 |
| **MFA** (dois métodos diferentes em sequência) | 📋 |
| **OAuth2** com os provedores do PocketBase: Apple, Bitbucket, Box, Discord, Facebook, Gitea, Gitee, GitHub, GitLab, Google, Instagram, Kakao, Lark, Linear, LiveChat, mailcow, Microsoft, monday.com, Notion, OpenID Connect (até 3 genéricos), Patreon, Planning Center, Spotify, Strava, Trakt, Twitch, Twitter/X, VK, WakaTime, Yandex | 📋 |
| Fluxo OAuth2 de redirecionamento (`/api/oauth2-redirect`) e PKCE | 📋 |
| Renovar token (`auth-refresh`) | ✅ (superusuário e usuário de coleção auth) |
| Verificação de e-mail (pedir e confirmar) | ✅ |
| Redefinir senha (pedir e confirmar) | ✅ |
| Trocar e-mail (pedir e confirmar) | ✅ |
| Personificação: superusuário gera token de outro usuário (`impersonate`) | ✅ |
| Alerta de login de novo dispositivo (`authAlert`) | 📋 |
| Métodos de login disponíveis por coleção (`auth-methods`) | 📋 |
| Token JWT de autenticação, invalidado ao trocar a senha | ✅ |
| Tokens de verificação, senha, e-mail e arquivo | ✅ (verificação, senha e e-mail) |
| Modelos de e-mail editáveis (verificação, senha, troca de e-mail, OTP, alerta) | 📋 (texto fixo; envio no console) |
| Gerador do segredo do Sign in with Apple | 📋 |

## 6. Arquivos

| Funcionalidade | Estado |
|---|---|
| Armazenamento local ou em S3 compatível (AWS, MinIO, R2…) | 📋 |
| Servir arquivos (`/api/files/{coleção}/{registro}/{arquivo}`) | 📋 |
| Miniaturas sob demanda (`?thumb=100x100`, com corte e ajuste) | 📋 |
| Arquivos protegidos com token de arquivo de curta duração | 📋 |
| Download forçado (`?download=1`) | 📋 |
| Testar a conexão S3 pelo painel | 📋 |

## 7. Tempo real

| Funcionalidade | Estado |
|---|---|
| Server-Sent Events (`/api/realtime`) | 📋 |
| Assinar uma coleção inteira ou um registro | 📋 |
| Eventos de criação, atualização e exclusão, respeitando as regras de acesso | 📋 |
| Opções por assinatura (filtro, expand, fields) | 📋 |

## 8. Configurações e operação

| Funcionalidade | Estado |
|---|---|
| Nome, URL e cor do app; remetente dos e-mails | 📋 |
| SMTP (com teste de envio) ou sendmail | 📋 |
| Limite de requisições por rota e por tipo de usuário | 📋 |
| Proxy confiável (cabeçalhos de IP real) | 📋 |
| IPs permitidos para superusuários | 📋 |
| Limite de tamanho de corpo, CORS e compressão gzip | 📋 |
| **Backups**: criar, baixar, enviar, restaurar e apagar; agendados (cron) com retenção; em S3 | 📋 |
| **Logs** de requisições: listar, estatísticas, retenção em dias, nível mínimo, IP e usuário opcionais | 📋 |
| **Cron**: listar e executar tarefas agendadas pelo painel | 📋 |
| `/api/health` | ✅ |

## 9. Painel administrativo (HTML no servidor + htmx)

O painel é HTML gerado no servidor com maud e htmx embutido no binário. Não há
build de front-end nem SPA: o navegador pede páginas e fragmentos ao mesmo
processo que serve a API, e a sessão é um cookie `httpOnly` com um JWT de
superusuário. WASM fica reservado para as extensões (§11).

| Funcionalidade | Estado |
|---|---|
| Painel embutido no executável, sem etapa de build própria | ✅ |
| Login de superusuário por cookie `httpOnly` + `SameSite=Strict` | ✅ |
| Lista de coleções | ✅ |
| Navegador de registros com filtro e paginação (htmx) | ✅ |
| Criar, editar e apagar registros pelo formulário, com erros por campo | ✅ |
| Editor de coleções, campos, regras e índices | 📋 |
| Telas de configurações, backups, logs e cron | 📋 |
| Visualização de logs com gráfico | 📋 |
| Extensões de interface do painel | 📋 |
| ➕ Seletor de projeto (§10) | 📋 |

## 10. ➕ Vários projetos numa instância

O PocketBase atende um projeto por processo. O Forja atende vários, isolados.

| Funcionalidade | Estado |
|---|---|
| Cada projeto com o próprio banco, coleções, usuários, arquivos, configurações e logs (`data/<projeto>/`) | 📋 |
| Endereço por caminho: `https://host/p/<projeto>/api/...` (a URL base dos SDKs) | 📋 |
| Endereço por domínio ou subdomínio (`app.exemplo.com` → projeto) | 📋 |
| Projeto padrão na raiz (`/api/...`), compatível com clientes do PocketBase sem mudança | 📋 |
| Criar, renomear, pausar, apagar e duplicar projetos (CLI, API e painel) | 📋 |
| Superusuários da instância (todos os projetos) e de projeto (só um) | 📋 |
| Limites por projeto: tamanho do banco, armazenamento, requisições | 📋 |
| Backup e restauração por projeto | 📋 |
| Importar um `pb_data` do PocketBase como um projeto | 📋 |

## 11. ➕ Extensões em WebAssembly (no lugar do JavaScript)

O PocketBase estende o servidor com JavaScript (goja). O Forja usa módulos
WebAssembly (wasmtime), escritos em qualquer linguagem que compile para WASM
(Rust, Go, C, AssemblyScript, Zig…).

| Funcionalidade | Estado |
|---|---|
| Hooks de eventos (antes e depois de criar, atualizar, apagar, autenticar, enviar e-mail…) | 📋 |
| Rotas próprias | 📋 |
| Tarefas agendadas (cron) | 📋 |
| Migrações escritas como módulo | 📋 |
| Isolamento: cada módulo com limite de memória e de tempo, sem acesso ao disco fora do permitido | 📋 |
| Recarregar módulos sem reiniciar o servidor | 📋 |
| Usar o Forja como biblioteca Rust (como o PocketBase em Go), com hooks em Rust | 📋 |

## 12. SDKs

Um núcleo único em Rust (`forja-sdk`) com a lógica comum (HTTP, armazenamento
do token, renovação, tempo real, montagem de filtros) e camadas geradas para
cada linguagem. Todos falam com qualquer projeto pela URL base (§10).

| SDK | Plataformas | Como é feito | Estado |
|---|---|---|---|
| **JavaScript / TypeScript** | navegador, Node, Deno, Bun; serve a **React, Vue, Angular, Svelte**, Next, Nuxt, SvelteKit, Solid, Astro e qualquer projeto JS/TS | núcleo em Rust → WebAssembly (wasm-bindgen), tipos TypeScript gerados | 📋 |
| React | hooks (`useRecord`, `useList`, `useAuth`, `useRealtime`) | camada fina sobre o SDK JS/TS | 📋 |
| Vue | composables | camada fina sobre o SDK JS/TS | 📋 |
| Angular | serviço injetável e Observables | camada fina sobre o SDK JS/TS | 📋 |
| Svelte | stores | camada fina sobre o SDK JS/TS | 📋 |
| **Kotlin** | Android (e JVM) | UniFFI | 📋 |
| **Swift** | iOS, iPadOS, macOS, watchOS, visionOS | UniFFI, pacote SwiftPM | 📋 |
| **Dart / Flutter** | Android, iOS, web, desktop | flutter_rust_bridge | 📋 |
| **Rust** | qualquer alvo, inclusive WASM | o próprio núcleo, publicado no crates.io | 📋 |
| **Java** | JVM (Spring, Android sem Kotlin) | sobre a camada Kotlin/JVM | 📋 |
| **Python** | CPython 3.9+ | UniFFI, wheels no PyPI | 📋 |
| **C#** | .NET 8+, Unity, MAUI | gerador UniFFI para C# | 📋 |
| **Go** | Go 1.22+ | gerador UniFFI para Go | 📋 |

Cada SDK cobre: autenticação (todos os métodos de §5) com armazenamento do
token, CRUD e consulta (§4) com filtros montados com segurança, arquivos (§6),
tempo real (§7), lote, chamadas a rotas próprias, e troca de projeto (§10).
Como a API segue a do PocketBase, os SDKs oficiais dele (JS e Dart) também
funcionam com o projeto padrão.

## 13. ➕ MCP (Model Context Protocol), com segurança

O Forja expõe um servidor MCP para agentes de IA (Claude, IDEs, outros
clientes MCP) lerem e, se permitido, alterarem dados de um projeto.

| Funcionalidade | Estado |
|---|---|
| Servidor MCP local pela CLI: `forja mcp --project <p>` (transporte stdio) | 📋 |
| Servidor MCP remoto em `/mcp` (HTTP streamable), só com HTTPS | 📋 |
| Ferramentas: listar coleções e esquemas, listar/ver/buscar registros, criar/atualizar/apagar registros, ver logs | 📋 |
| Recursos: esquema das coleções e documentação do filtro, para o agente montar consultas certas | 📋 |
| **Tokens de MCP** por projeto, com validade, criados e revogados pelo painel ou pela CLI | 📋 |
| **Somente leitura por padrão**; escrita e exclusão só com escopo explícito no token | 📋 |
| Lista de coleções e operações permitidas por token | 📋 |
| As regras de acesso das coleções (§3.2) valem também pelo MCP, como um usuário comum | 📋 |
| SQL bruto, configurações, backups e superusuários **fora** do MCP (sem opção de ligar remotamente) | 📋 |
| Exclusão em massa exige confirmação no próprio fluxo (dry-run mostra o que vai sair) | 📋 |
| Registro de auditoria de cada chamada (token, ferramenta, argumentos, resultado) | 📋 |
| Limite de requisições e de linhas por resposta por token | 📋 |
| Campos `password` e tokens nunca aparecem nas respostas | 📋 |

## 14. ➕ CLI de administração

Tudo o que o painel faz, também pelo terminal, local ou contra um servidor
remoto (com token).

| Funcionalidade | Estado |
|---|---|
| `forja projects list | create | rename | pause | delete | import-pb` | 📋 |
| `forja collections list | show | create | import | export | truncate` | 📋 |
| `forja records list | get | create | update | delete` com `--filter`, `--sort`, `--expand`, saída em tabela ou JSON | 📋 |
| `forja users` / `forja superuser` (criar, redefinir senha, gerar OTP) | 📋 |
| `forja tokens create | list | revoke` (inclusive tokens de MCP) | 📋 |
| `forja backups create | list | restore | download` | 📋 |
| `forja logs tail` | 📋 |
| `forja login --url https://host/p/projeto` e perfis de servidores (`--profile`) | 📋 |
| Saída em JSON estável para scripts (`--json`) | 📋 |

## 15. Qualidade e distribuição

| Funcionalidade | Estado |
|---|---|
| Testes de unidade e de API em todo crate | ✅ (32 testes) |
| Cliente Python do PocketBase (`pocketbase` 0.16) funciona contra o Forja: login, CRUD, filtro e ordenação | ✅ (conferido à mão em 2026-10-21) |
| Suíte de compatibilidade: as mesmas chamadas contra PocketBase e Forja, respostas iguais | 📋 |
| CI no GitHub Actions (formatação, lint e testes nos três SOs) | ✅ |
| Executáveis para macOS, Linux e Windows (x86_64 e arm64) no GitHub Releases | 📋 |
| Imagem Docker | 📋 |
| Site de documentação | 📋 |
