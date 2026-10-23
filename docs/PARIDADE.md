# Paridade com PocketBase e Appwrite

Este documento responde a uma pergunta prática: **como chegar perto de todos os
recursos dos dois sem perder o rumo**. Ele não substitui a
[arquitetura](ARQUITETURA.md) nem a [lista de funcionalidades](FUNCIONALIDADES.md);
serve de mapa entre o que eles têm, o que o Forja tem e o que fazer em seguida.

## 1. A regra do jogo

| Papel | Produto | O que copiamos | O que não copiamos |
|---|---|---|---|
| **Contrato** | PocketBase | Rotas, JSON, códigos de erro, filtros, regras, nomes de campo, semântica de auth e de arquivos. Se o SDK oficial do PocketBase funciona, a compatibilidade é real. | Hooks em JavaScript, SQLite como única opção de escala, single-node obrigatório. |
| **Cardápio** | Appwrite | Funcionalidades que valem a pena (equipes/papéis, buckets, mensageria, sessões, magic URL, OTP por SMS), sempre traduzidas para o modelo do Forja. | A arquitetura (MariaDB + Redis + workers + muitos containers), GraphQL como API principal, funções em 10 runtimes. |

Três consequências:

1. **Toda rota nasce no formato do PocketBase.** Uma funcionalidade só do Appwrite
   ganha rota própria, marcada como extensão, e nunca quebra o contrato.
2. **Nada de SDK antes da 0.9.** Até lá, o SDK do PocketBase é o teste de
   conformidade — não uma suíte escrita por nós.
3. **Escala é vertical.** O Forja é um processo por instância, um `data.db` por
   projeto. Replicação e multi-nó ficam fora (ARQUITETURA §5).

## 2. Onde estamos (2026-10-23)

| Área | PocketBase | Appwrite | Forja | Onda |
|---|---|---|---|---|
| Coleções base + campos | ✅ | ✅ | ✅ | feito |
| Coleções `auth` com senha | ✅ | ✅ | ✅ | feito |
| Regras por operação + `@request.auth.*` | ✅ | ✅ (permissões) | ✅ | feito |
| Filtros, ordenação, paginação | ✅ | ✅ | ✅ | feito |
| Superusuário, impersonate | ✅ | ✅ (console) | ✅ | feito |
| E-mail: verificação, senha, troca | ✅ | ✅ | ✅ (console; SMTP na 0.5) | feito |
| Relações + `expand` | ✅ | ✅ | 📋 | 0.2 |
| Views (SQL somente leitura) | ✅ | ⛔ | 📋 | 0.2 |
| Arquivos + miniaturas | ✅ | ✅ (buckets) | 📋 | 0.3 |
| Lote (`/api/batch`) | ✅ | ⛔ | 📋 | 0.3 |
| Tempo real | ✅ (SSE) | ✅ (WebSocket) | 📋 | 0.3 |
| Painel administrativo | ✅ | ✅ | 🔨 (login e CRUD de registros; editor de coleções na 0.4) | 0.4 |
| Logs, cron, rate limit, settings | ✅ | ✅ (parcial) | 📋 | 0.5 |
| Backups + S3 | ✅ | ✅ (Cloud) | 📋 | 0.5 |
| OAuth2, OTP, MFA | ✅ | ✅ | 📋 | 0.6 |
| Vários projetos | ⛔ (uma instância = um app) | ✅ | 📋 | 0.7 |
| Funções/extensões | ✅ (JS) | ✅ (10 runtimes) | 📋 (WASM) | 0.8 |
| MCP para agentes | ⛔ | ⛔ | 📋 (diferencial) | 0.8 |
| SDKs oficiais | ✅ (4) | ✅ (muitos) | 📋 (gerados do OpenAPI) | 0.9 |
| GraphQL | ⛔ | ✅ | ⛔ (decisão: só REST) | — |
| Hospedagem de sites estáticos | ⛔ | ✅ (Sites) | 📋 (avaliar na 0.7) | 0.7+ |

Legenda: ✅ pronto · 📋 planejado · ⛔ fora de escopo.

## 3. O que o Appwrite tem e o PocketBase não

Estas são as lacunas que **só o Appwrite cobre**. Cada uma vira uma extensão do
Forja, com nome e formato próprios, sem tocar nas rotas do PocketBase.

| Recurso do Appwrite | Por que vale | Como fica no Forja | Onda |
|---|---|---|---|
| **Equipes e papéis** (`teams`, `team:<id>/<papel>`) | Autorização por grupo é o que falta no modelo de "dono do registro" do PocketBase. | Coleção de sistema `_teams` + `_memberships`; macro `@request.auth.teams.*` nas regras. | 0.2 (macros) / 0.6 |
| **Buckets de arquivo** com permissões próprias | O PocketBase amarra arquivo à coleção; bucket separado simplifica compartilhamento. | `_buckets` opcional; o campo `file` continua igual. | 0.3 |
| **Magic URL** (link de login sem senha) | Menos atrito que senha; já temos token de e-mail. | `POST /api/collections/{c}/auth-with-magic-url` (extensão). | 0.6 |
| **OTP por e-mail/SMS** | Segundo fator simples; o PocketBase só tem e-mail. | `_otps` + provedor de SMS atrás de um trait, como o `Correio`. | 0.6 |
| **Sessões gerenciáveis** (listar e revogar dispositivos) | Segurança real para app móvel. | `GET/DELETE /api/collections/{c}/sessions`; o `tokenKey` já permite revogar em massa. | 0.6 |
| **Sessão anônima** | Comum em apps que pedem login depois. | Coleção `auth` com `authRule` que aceita visitante e um campo `anon`. | 0.6 |
| **Mensageria** (e-mail/SMS/push com tópicos) | Notificação é a próxima dor depois do CRUD. | Fila em SQLite + trait `Mensageiro`; sem worker externo. | 0.5 |
| **Sites estáticos** | Fecha o ciclo "backend + front". | Servir uma pasta por projeto, com `_redirects`. | 0.7+ |
| **Console com métricas** | Diagnóstico sem `sqlite3`. | Painel 0.4 + página de métricas na 0.5. | 0.4/0.5 |

O que **não** copiamos do Appwrite: GraphQL (dobra a superfície de teste sem
resolver nada que o REST não resolva), funções em vários runtimes (WASM cobre
o caso de uso com um contrato só) e a arquitetura de containers.

## 4. O que o PocketBase tem e ainda falta

| Recurso | Detalhe que importa | Onda |
|---|---|---|
| `relation` + `expand` | É o que transforma tabelas em API utilizável. Inclui cascata e relações reversas. | 0.2 |
| `@collection.*` nas regras | Permite regra do tipo "só quem é admin em `equipes`". | 0.2 |
| Operadores `?=`, `?~`, `:isset`, `:changed`, `:length`, `:each` | Filtros de lista JSON e regras de transição (`status:changed`). | 0.2/0.3 |
| `file` com miniaturas e URL assinada | Upload, recorte e proteção. | 0.3 |
| SSE com filtro por regra | O cliente só recebe o que pode ver. | 0.3 |
| `_settings`, logs, cron, rate limit | Operação de verdade, sem SSH. | 0.5 |
| Backup consistente + S3 | API de backup online do SQLite + manifesto. | 0.5 |
| OAuth2 (30 provedores), OTP, MFA | Login social é obrigatório para app de consumo. | 0.6 |
| `auth-methods`, `authAlert`, `_authOrigins` | UX de login e alerta de novo dispositivo. | 0.6 |
| Migrações versionadas | Sem isso, mudar schema em produção é manual. | 0.5/0.8 |
| `pb_data` import | Adoção: traz quem já usa PocketBase. | 0.7 |
| Hooks (antes/depois) | Automação sem alterar o núcleo. | 0.8 (WASM) |

## 5. A sequência que mais aproxima os dois

Ordem por **impacto no app real**, não por tamanho:

1. **0.2 — Relações, `expand` e views.** Sem isso, todo cliente faz N+1 na mão.
   Junto: `@collection.*` e os operadores de lista.
2. **0.3 — Arquivos, lote e tempo real.** Fecha o trio que faz um app parecer
   pronto: anexo, upload em lote e atualização ao vivo.
3. **0.4 — Painel.** Quem não programa precisa criar coleção e ver registro.
4. **0.5 — Operação.** Settings, logs, cron, rate limit, backup e SMTP. É o que
   permite rodar fora do notebook.
5. **0.6 — Login social e segundo fator.** OAuth2, OTP, MFA, sessões e magic URL;
   aqui entram as primeiras extensões inspiradas no Appwrite.
6. **0.7 — Vários projetos + importar `pb_data`.** Uma instância, vários apps,
   caminho de migração para quem vem do PocketBase.
7. **0.8 — WASM + MCP.** Extensões e agentes; é o diferencial que nenhum dos dois
   tem junto.
8. **0.9 — SDKs gerados + conformidade.** A paridade deixa de ser afirmação e
   vira teste: a mesma suíte roda contra o PocketBase e contra o Forja.
9. **1.0 — Estável.** Critérios do [PLANO](PLANO.md) §6.

## 6. Como medir a paridade (sem autoengano)

| Medida | Meta | Quando |
|---|---|---|
| Suíte do SDK oficial do PocketBase (JS/Dart/Kotlin/Swift) | verde nas operações que já existem | a cada onda |
| Rotas do PocketBase implementadas | 100% das usadas por um app CRUD+auth até a 0.9 | 0.9 |
| Testes de contrato (status, corpo, códigos de erro) | um por rota, comparando com o PocketBase real | contínuo |
| Funcionalidades do Appwrite adotadas | só as da §3, cada uma com teste próprio | por onda |
| Cobertura de testes do workspace | ≥ 90% nas linhas de `forja-core` | 0.9 |

O teste de contrato é o mais importante: sobe o PocketBase numa porta, o Forja
noutra, executa a mesma chamada e compara status e JSON (ignorando ids e datas).
Divergência vira issue, não "quase igual".

## 7. Decisões que não reabrimos

- **REST, não GraphQL.** O contrato é o do PocketBase; GraphQL seria uma segunda
  API para manter e testar.
- **SQLite, um arquivo por projeto.** Escala vertical; Turso e Postgres seguem
  compiláveis, mas fora do suporte.
- **Regras são filtros compilados para SQL.** Sem ACL paralela, sem linguagem de
  script nas regras.
- **Extensões em WASM.** Nada de runtime JS embutido: um contrato JSON, um
  sandbox, um jeito de instalar.
- **SDK gerado do OpenAPI.** O contrato é a fonte; SDK escrito à mão vira dívida.
