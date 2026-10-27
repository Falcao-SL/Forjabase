# Status — Forja

> Atualizado em **2026-10-27**. Este arquivo é a fonte única de "em que pé está".

| Campo | Valor |
|---|---|
| **Estágio** | **Em desenvolvimento** — 0.1 e 0.2 entregues. Marco M1 entregue (barramento de escrita). Relações, `expand`, coleções `view` e `@collection.*` estão no núcleo |
| **Plataformas** | Servidor: macOS, Linux, Windows (Rust); painel: navegador, HTML no servidor + htmx (sem build de front-end) |
| **Stack** | Rust (axum; SQLite), maud + htmx, wasmtime in-process na 0.8. Decisão: [ADR-001](arquitetura/ADR-001-um-binario.md) |
| **Site** | Landing em `site/` (inglês na raiz, português em `index.pt.html`). O host previsto é https://forja.falcaosl.com, no Coolify, com `deploy/site/Dockerfile`. O DNS ainda não existe; o endereço público segue o repositório. |
| **Monetização** | Nenhuma: código aberto, MIT |
| **Próximo marco** | 0.3: campo `file`, lote (`/api/batch`) e SSE. Ainda na fila, sem fechar versão: operadores `?=`, `dry-run-view` e `PATCH` de coleção |
| **Data-alvo** | A definir; 1.0.0 só quando estável (PLANO §6) |

## O que já está pronto

- Workspace com 4 crates e 47 testes passando.
- Barramento in-process (M1): `criar`, `atualizar` e `apagar` — inclusive senha,
  verificação, superusuário e cada registro da cascata — publicam
  `Evento { colecao, acao, id }` em `tokio::broadcast` depois do commit.
  Assinante ausente ou lento não desfaz a escrita. Sem rota SSE (isso é a 0.3).
- Campo `relation` (um ou vários): o id tem de existir; visitante só liga o que
  a `viewRule` do alvo deixa ver. `cascadeDelete` apaga o registro que ficou
  sem referências; relação obrigatória sem cascata impede o apagamento.
  `?expand=` direto, aninhado (`autor.cidade`) e reverso (`comentarios_via_post`,
  sempre lista). O painel edita o campo como texto de ids.
- Coleção `view`: um `SELECT` (ou `WITH`) com coluna `id`. Os outros nomes
  viram campos. Listar e ver respeitam `listRule` e `viewRule`. Criar, atualizar
  e apagar registro respondem que a coleção não é gravável — no API e no painel.
  Apagar a coleção de origem é recusado enquanto alguma view a citar.
- Macro `@collection.colecao.campo` nas regras e, para o superusuário, no
  `filter`. Vira `EXISTS`: no mesmo `&&`, uma linha da outra coleção tem de
  cumprir todos os predicados. No `||`, cada lado é independente. Visitante que
  manda `@collection` ou `@request` no filtro recebe 403.
- SQLite é o único motor suportado (um arquivo por projeto, WAL). Turso e
  PostgreSQL continuam compilando atrás de feature flag, sem suporte e fora do CI.
- Superusuário (Argon2id, JWT), coleções base e auth, registros com filtro do
  PocketBase convertido em SQL com parâmetros, ordenação, paginação, regras e
  campos ocultos.
- Autenticação de usuários: login, refresh, verificação de e-mail, redefinição
  de senha, troca de e-mail, personificação e macros `@request.auth.*`.
- Painel em `/_/`: login por cookie `httpOnly`, lista de coleções, tabela com
  filtro e paginação via htmx, criar/editar/apagar registros com erros por campo.
- Cliente Python do PocketBase funciona contra o Forja (conferido em 2026-10-21).
- Documentação: [FUNCIONALIDADES](FUNCIONALIDADES.md), [PLANO](PLANO.md), [GUIA](GUIA.md),
  [ARQUITETURA](ARQUITETURA.md), [PARIDADE](PARIDADE.md),
  [ADR-001](arquitetura/ADR-001-um-binario.md) (um binário: o que entra no núcleo).

## O que falta

Tudo marcado 📋 em [FUNCIONALIDADES.md](FUNCIONALIDADES.md), na ordem do PLANO §5.
