# Status — Forja

> Atualizado em **2026-10-27**. Este arquivo é a fonte única de "em que pé está".

| Campo | Valor |
|---|---|
| **Estágio** | **Em desenvolvimento** — 0.1 entregue. Marco M1 entregue (barramento de escrita). 0.2 em andamento: `relation`, cascata e `expand` prontos; faltam `view` e `@collection.*` |
| **Plataformas** | Servidor: macOS, Linux, Windows (Rust); painel: navegador, HTML no servidor + htmx (sem build de front-end) |
| **Stack** | Rust (axum; SQLite), maud + htmx, wasmtime in-process na 0.8. Decisão: [ADR-001](arquitetura/ADR-001-um-binario.md) |
| **Site** | Landing em `site/` (inglês na raiz, português em `index.pt.html`). O host previsto é https://forja.falcaosl.com, no Coolify, com `deploy/site/Dockerfile`. O DNS ainda não existe; o endereço público segue o repositório. |
| **Monetização** | Nenhuma: código aberto, MIT |
| **Próximo marco** | Fechar a 0.2: coleções `view` e a macro `@collection.*`. O barramento (M1), o campo `relation`, a cascata e o `expand` já estão no núcleo |
| **Data-alvo** | A definir; 1.0.0 só quando estável (PLANO §6) |

## O que já está pronto

- Workspace com 4 crates e 37 testes passando.
- Barramento in-process (M1): `criar`, `atualizar` e `apagar` — inclusive senha,
  verificação, superusuário e cada registro da cascata — publicam
  `Evento { colecao, acao, id }` em `tokio::broadcast` depois do commit.
  Assinante ausente ou lento não desfaz a escrita. Sem rota SSE (isso é a 0.3).
- Campo `relation` (um ou vários): o id tem de existir; visitante só liga o que
  a `viewRule` do alvo deixa ver. `cascadeDelete` apaga o registro que ficou
  sem referências; relação obrigatória sem cascata impede o apagamento.
  `?expand=` direto, aninhado (`autor.cidade`) e reverso (`comentarios_via_post`,
  sempre lista). O painel edita o campo como texto de ids.
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
