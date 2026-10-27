# Arquitetura do Forja

Este documento é o mapa de camadas e de ondas. As escolhas de stack (axum,
SQLite, maud + htmx, API do PocketBase, SSE, WASM no processo, núcleo versus
extensão) estão travadas no [ADR-001](arquitetura/ADR-001-um-binario.md). Onde
este arquivo, o [PLANO](PLANO.md) ou a [PARIDADE](PARIDADE.md) divergirem do
ADR, vale o ADR.

## 1. A decisão

O Forja é um **backend embarcado, de nó único, em SQLite, compatível com a API
do PocketBase**. O caminho até a paridade com PocketBase e Appwrite não passa
por trocar de banco: passa por construir a camada de plataforma que os dois têm
e o Forja ainda não tem.

| # | Decisão | Escolha |
|---|---|---|
| 1 | Banco | **SQLite**, um arquivo por projeto. Sem NoSQL. |
| 2 | Modelo de dados | Uma **tabela física por coleção**, como no PocketBase. |
| 3 | Contrato público | **API do PocketBase.** SDKs, tutoriais e migrações dele funcionam aqui. |
| 4 | Appwrite e Supabase | Fonte de **ideias de recurso** (auth, dados, arquivo, tempo real, função). Nunca de API nem da arquitetura de serviços deles. |
| 5 | Permissões | Um só sistema: **regras como filtros** + macros (`@request.auth.*`). Sem ACL paralela. |
| 6 | Tempo real | **In-process + SSE** (protocolo do PocketBase). Sem WebSocket. |
| 7 | Extensões | **WASM** (wasmtime) **no mesmo processo**, ABI JSON. Sem subprocesso e sem runtime JS. |
| 8 | Vários projetos | Um **arquivo SQLite por projeto** + banco de controle. |
| 9 | SDKs | **OpenAPI gerado** a partir dos handlers; até a 0.9 o SDK do PocketBase é o cliente de teste. |
| 10 | Painel | **maud + htmx** no mesmo processo. Cookie `httpOnly` só para o admin. Sem askama, sem SPA. |
| 11 | Backups | Snapshot pela **API de backup online** + S3 + manifesto. Litestream é integração externa. |
| 12 | Escala | **Nó único.** Escala vertical, não horizontal. |
| 13 | Postgres e Turso | Feature de compilação, **sem suporte** e sem CI. Postgres só se o operador já tem um Postgres e aceita sair do arquivo único; Turso só para abrir o mesmo arquivo com libSQL. Nenhum dos dois é caminho de compatibilidade com Supabase. Detalhe no ADR-001. |

## 2. Por que SQLite, e por que não NoSQL

A pergunta "existe um banco NoSQL que suporte tudo?" confunde duas camadas. O
"tudo" do PocketBase e do Appwrite não está no banco — está em autenticação,
arquivos, tempo real, painel, permissões, backups, migrações e SDKs. O Appwrite,
que tem a maior superfície do mercado, roda em **MariaDB** (`mariadb:10.11`) com
Redis e Traefik em volta. Banco é a parte barata do problema.

| Opção | Encaixa no binário único | Resolve o que falta | Custo real |
|---|---|---|---|
| **SQLite** | Sim, é um arquivo | Nada por si só, mas não atrapalha | Nenhum; já está feito |
| Postgres | Não (serviço) | Nada do que falta | Volta o ops que o produto elimina |
| Mongo | Não (serviço, replica set p/ change streams) | Só o schema flexível | Reescrever regras, expand e relações; perde o backup de um arquivo |
| CouchDB | Não | Sincronização | Consultas fracas, ecossistema parado |
| SurrealDB | Sim (embarcado, Rust, live queries) | Só o tempo real, que já resolvemos em processo | Reescrita do armazenamento, maturidade e licença a auditar |
| Firebase/Firestore | Não (serviço) | Nada que interesse | Dependência de nuvem, sem auto-hospedagem |

O argumento decisivo é o modelo de permissões. Uma regra do PocketBase é um
filtro que **compila para um `WHERE`**. É por isso que as regras são expressivas
e baratas. Num banco de documentos, esse compilador vira um interpretador de
consultas próprio — mais trabalho, não menos. O mesmo vale para `expand`,
relações, cascata e índices únicos.

E o que o SQLite entrega de graça e é central aqui: **um arquivo**. `data.db`
cabe num backup, num e-mail, num `git`, num Litestream. Isso é vantagem de
produto, não detalhe de implementação.

### Consequências aceitas

- **Um escritor por vez.** Suficiente para o alvo (apps pequenos e médios). O
  limite é explícito e documentado, não escondido.
- **Sem HA.** Réplica é DR (Litestream), não failover. "Escale para cima" é a
  resposta oficial.
- **Perde-se o discurso "Postgres/Supabase".** Em troca, some a obrigação de
  manter três dialetos, três baterias de conformidade e três respostas para
  cada recurso novo.
- Postgres e Turso continuam compilando atrás de `--features`, para não jogar
  fora o trabalho e manter a costura testável. Não são suportados.

## 3. Camadas

```
┌─────────────────────────────────────────────────────────────┐
│  painel (HTML+htmx)   CLI `forja`   SDKs gerados   MCP    │
└──────────────────────────────┬──────────────────────────────┘
                               │ HTTP/JSON + SSE
┌──────────────────────────────▼──────────────────────────────┐
│  forja-server (axum): rotas, auth, regras, erros, realtime   │
├─────────────────────────────────────────────────────────────┤
│  forja-core: coleções, registros, validação, filtros → SQL   │
├─────────────────────────────────────────────────────────────┤
│  Executor (SQLite) ── 1 escritor + N leitores (WAL)          │
├─────────────────────────────────────────────────────────────┤
│  data.db   storage/   extensões .wasm                        │
└─────────────────────────────────────────────────────────────┘
```

Regras que valem para todo código novo:

1. **Identificador nunca é interpolado sem validação.** Nome de coleção e de
   campo passam por `nome_valido`/`coluna_valida` antes de virar SQL.
2. **Valor nunca entra no SQL.** Sempre parâmetro.
3. **Macro nunca é substituída por texto.** `@request.auth.*` vira parâmetro no
   compilador de filtros, com o tipo certo.
4. **Recurso novo nasce no SQLite.** Se precisar de `json_extract`, `FTS5` ou
   `RETURNING`, usa. O `Executor` é costura de teste, não camada de menor
   denominador.
5. **Escrita passa por um ponto.** É o mesmo ponto que publica evento de tempo
   real, dispara hook e invalida cache.
6. **Nada de estado global mutável.** Cada projeto tem seu `Banco` e seu canal.

## 4. Ondas até a 1.0

A ordem abaixo é por **o que destrava um app real**, não por facilidade. Cada
onda só começa quando a anterior tem teste de ponta a ponta.

| Onda | Versão | Entrega | Portão de saída | Tamanho |
|---|---|---|---|---|
| 1 | 0.1 | **Autenticação**: coleções `auth`, campos de sistema, senha/verificação/redefinição/troca de e-mail, personificação, macros `@request.auth.*`, envio de e-mail (console e SMTP) | SDK do PocketBase faz login, refresh e reset contra o Forja; regra `@request.auth.id = id` funciona | L |
| 2 | 0.2 | **Relações e views**: campo `relation` (um e vários), cascata, `expand`, relações reversas, coleções `view`, macro `@collection.*` | Esquema posts/comentários/usuários com `expand` em uma chamada | L |
| 3 | 0.3 | **Arquivos, lote e tempo real**: campo `file`, local/S3, miniaturas, arquivos protegidos, `POST /api/batch`, SSE com filtro por regra | Upload → servir → assinar evento, tudo testado | L |
| 4 | 0.4 | **Painel utilizável**: login, editor de coleções e campos, navegador de registros, configurações, logs | Criar coleção e fazer CRUD só pela interface | XL |
| 5 | 0.5 | **Operação**: `_settings`, logs com retenção, cron, limite de requisições, backups com S3 e restauração, encerramento gracioso | Restaurar um backup em CI e rodar `integrity_check` | L |
| 6 | 0.6 | **OAuth2, OTP e MFA** | Login Google e GitHub com o SDK do PocketBase | L |
| 7 | 0.7 | **Vários projetos**: banco de controle, roteamento `p/<projeto>`, limites por projeto, importar `pb_data` | Importar um `pb_data` e servir em `p/<projeto>` | L |
| 8 | 0.8 | **Extensões e MCP**: hooks, rotas e cron em WASM, migrações, servidor MCP com tokens de escopo | Extensão WASM intercepta criação de registro; MCP lê com token restrito | XL |
| 9 | 0.9 | **SDKs e conformidade**: OpenAPI publicado, SDKs gerados, bateria de compatibilidade contra o PocketBase, TLS automático, builds macOS/Linux/Windows | Suíte de conformidade verde nos três sistemas | L |
| 10 | 1.0 | Estável | Critérios do PLANO §6 | — |

Não se escreve SDK antes da 0.9. Até lá, o SDK oficial do PocketBase é o
cliente: se ele funciona, a compatibilidade é real e não autodeclarada.

**Estado:** a onda 1 está concluída (2026-10-23) com o envio de e-mail no
console; o SMTP entra na onda 5, atrás do mesmo trait `Correio`.

## 5. O que fica de fora, por decisão

Copiar a nuvem do Appwrite ou a fatia de serviços do Supabase destruiria a tese
do produto. A lista fechada, com o corte núcleo/extensão, está no
[ADR-001](arquitetura/ADR-001-um-binario.md). Ficam fora, sem arrependimento:

- **Hospedagem de sites, domínios e firewall** (Sites, Domains, Firewall).
- **Infraestrutura de mensageria** (SMS, push, filas). E-mail transacional fica,
  porque autenticação depende dele.
- **GraphQL.** O PocketBase não tem; o custo de manter dois contratos é alto.
- **Funções em linguagem arbitrária.** Extensão é WASM.
- **ACL por papel/equipe do Appwrite.** Regras por filtro cobrem o mesmo com um
  sistema só.
- **Cluster, multi-região, eleição de líder.** Nó único.

E duas coisas que o Appwrite tem e valem copiar: **transformação de imagens** e
**MCP com skills para agentes** (já planejado). São diferenciais baratos.

## 6. Riscos

| Risco | Mitigação |
|---|---|
| O painel (0.4) virar o projeto inteiro | Editor de coleções e CRUD primeiro; o resto do painel é incremental |
| A ABI de extensões endurecer cedo demais | Congelar só os 4 hooks essenciais na 0.8; versionar a ABI desde o início |
| Compatibilidade com PocketBase quebrar sem ninguém notar | CI roda o SDK Python e o JS contra o Forja a cada release |
| `data.db` crescer e o snapshot ficar caro | Snapshot incremental por página via API de backup; medir a partir de 100 MB |
| Tentação de reintroduzir Postgres no meio do caminho | A decisão 13 só se revisita com um cliente pagante que exija; não por conveniência |
