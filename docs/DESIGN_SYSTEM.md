# Design System Forjabase — Cyber Brutalism

> Versão 1.0 · Inspirado em estética **Cyber Brutalism**, **Terminal Industrial** e **Neo-Brutalist Dashboard UI**.

---

## 1. Filosofia e Princípios

O design system do **Forjabase** abandona o minimalismo asséptico e os tons pastéis genéricos em favor de uma identidade **técnica, crua, visceral e intransigentemente funcional**.

```
    ┌─────────────────────────────────────────────────────────────────┐
    │  // THE FUTURE ISN'T MINIMAL. IT'S SYSTEMATIC.                  │
    │  Raw, digital and unapologetically functional.                  │
    │  Visual language for the machine age.                           │
    └─────────────────────────────────────────────────────────────────┘
```

### 1.1 Princípios Centrais

1. **Function Over Form (Função sobre Forma)**:
   Construído para operar. Sem sombras falsas de 40px, sem curvaturas exageradas, sem elementos puramente ornamentais que roubam foco. Cada elemento comunica estado e ação.
2. **Systems Thinking (Pensamento Sistêmico)**:
   Grids modulares estritos, divisões nítidas de 1px, identificadores numéricos (`/01`, `/02`), dados brutos visíveis e telemetria em tempo real.
3. **Bold Contrast (Contraste Máximo)**:
   Preto abissal profundo (`#07080a`), superfícies de grafite escuro (`#0e1117`), texto branco puro (`#ffffff`) e acentos elétricos em **Neon Lime** (`#d4ff00`).
4. **Digital & Monospace Aesthetics (Estética Digital)**:
   Metadados, códigos, status e rótulos secundários utilizam fontes monospace com tracking largo. Indicadores direcionais (`↗`, `→`, `✕`, `●`, `//`) comunicam fluxo imediato.
5. **Zero-Radius / Crisp Geometry**:
   Bordas vivas com cantos retos (`border-radius: 0px` ou geométrico `2px`), grids modulares e caixas de alta precisão técnica.

---

## 2. Paleta de Cores e Tokens

### 2.1 Cores Base (Dark Obsidian)

| Token | Valor Hex | Descrição | Uso |
|---|---|---|---|
| `--void` | `#07080a` | Preto absoluto abissal | Fundo principal da página |
| `--fundo` | `#0c0e12` | Grafite profundo | Fundo do canvas |
| `--superficie` | `#11141c` | Grafite ardósia | Painéis, cards e menus |
| `--elevado` | `#161b26` | Superfície secundária | Cabeçalho de tabelas, inputs |
| `--linha` | `#1f2633` | Borda técnica sutil | Divisores de grade, tabelas |
| `--linha-forte` | `#2e384b` | Borda de contraste | Contorno de cards, inputs |
| `--linha-brilhante`| `#485772` | Borda de destaque | Estados hover, linhas ativas |

### 2.2 Cores de Acento (Neon Cyber)

| Token | Valor Hex | Descrição | Uso |
|---|---|---|---|
| `--marca` (Neon Lime) | `#d4ff00` | Verde-limão elétrico | Ações primárias, foco, ativos |
| `--marca-hover` | `#bce600` | Limão de alto brilho | Hover de botões primários |
| `--marca-fundo` | `#1b2102` | Brilho escurecido | Badges e pílulas ativas |
| `--marca-brilho` | `rgba(212, 255, 0, 0.4)` | Glow elétrico | Box-shadow em botões e gráficos |
| `--ciano` | `#00e5ff` | Azul ciano elétrico | Redirecionamentos 3xx e links |
| `--sucesso` | `#00ffaa` | Verde menta puro | Status 2xx, telemetria ativa |
| `--aviso` | `#ffb800` | Âmbar industrial | Status 4xx, alertas de atenção |
| `--perigo` | `#ff3355` | Vermelho laser | Status 5xx, exclusões e erros |

### 2.3 Tipografia e Escala de Texto

| Token | Valor | Uso |
|---|---|---|
| `--tinta` | `#f4f6fb` | Texto principal em alto contraste |
| `--fraco` | `#7e8b9f` | Texto secundário e rótulos de dados |
| `--dim` | `#444e60` | Placeholders e dados desativados |
| `--mono` | `ui-monospace, "JetBrains Mono", "SF Mono", Consolas, monospace` | Números, IDs, status, logs |
| `--sans` | `ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif` | Interface geral e leitura |

---

## 3. Escala Tipográfica e Hierarquia

- **Display / Hero**: `font-weight: 800; font-size: 26px; text-transform: uppercase; letter-spacing: -0.03em;`
- **Seção / H2**: `font-weight: 800; font-size: 16px; text-transform: uppercase; letter-spacing: -0.01em;`
- **Sub-cabeçalho / Rótulos**: `font-family: var(--mono); font-size: 11px; text-transform: uppercase; letter-spacing: 0.08em;`
- **Corpo / Tabela**: `font-family: var(--sans); font-size: 13px; line-height: 1.5;`
- **Código / IDs**: `font-family: var(--mono); font-size: 12px; background: rgba(255,255,255,0.05);`

---

## 4. Componentes

### 4.1 Barra de Comando Superior (`.topo`)

Barra escura com divisão horizontal fina, logotipo com bloco neon lime `▮ FORJA_`, badges de versão, navegação com rótulos mono e indicador de telemetria ativa:

```html
<header class="topo">
  <a class="marca" href="/_/">
    Forja
    <span class="marca-tag">v0.6</span>
  </a>
  <nav>
    <a class="ativo" href="/_/">Coleções</a>
    <a href="/_/logs">Logs</a>
    <a href="/_/crons">Crons</a>
    <a href="/_/backups">Backups</a>
    <a href="/_/settings">Configurações</a>
  </nav>
  <div class="telemetria">
    <span class="pulso-verde"></span>
    SYS: ONLINE_
  </div>
  <form method="post" action="/_/logout">
    <button class="botao" type="submit">Sair ↗</button>
  </form>
</header>
```

### 4.2 Botões (`.botao`)

- **Primário (`.botao.solido`)**:
  - Fundo `#d4ff00`, texto preto `#000000`, cantos retos `border-radius: 0`.
  - Hover: elevação suave de 1px com glow `box-shadow: 0 0 16px rgba(212, 255, 0, 0.4)`.
  - Indicador terminal direcional `↗` no final do rótulo.
- **Secundário / Neutro (`.botao`)**:
  - Fundo escuro `#11141c`, borda `1px solid #2e384b`, texto `#f4f6fb`.
  - Hover: borda e texto neon lime `#d4ff00`, glow sutil.
- **Perigo (`.botao.perigo`)**:
  - Borda `1px solid #ff3355`, texto `#ff3355`.
  - Hover: fundo translúcido `rgba(255, 51, 85, 0.12)` com glow avermelhado.

### 4.3 Tabela de Dados Técnica (`table`, `th`, `td`)

- Divisões em grade de 1px (`#1f2633`).
- Cabeçalhos (`th`) em cinza técnico com fonte mono em caixa alta.
- Linhas com destaque no hover com leve iluminação âmbar/limão.
- Colunas numéricas e de ação alinhadas à direita.

### 4.4 Badges de Status HTTP (`.status-badge`)

Pílulas retangulares em fonte monospace de alta legibilidade:
- `2xx`: Borda e texto `#00ffaa`, fundo `rgba(0, 255, 170, 0.10)`.
- `3xx`: Borda e texto `#00e5ff`, fundo `rgba(0, 229, 255, 0.10)`.
- `4xx`: Borda e texto `#ffb800`, fundo `rgba(255, 184, 0, 0.10)`.
- `5xx`: Borda e texto `#ff3355`, fundo `rgba(255, 51, 85, 0.12)`.

### 4.5 Gráfico de Telemetria / Barras (`.grafico-barras`)

- Recipiente em preto abissal com borda técnica.
- Barras de volume horário que ganham destaque em Neon Lime (`#d4ff00`) ao passar o mouse.

### 4.6 Portal de Acesso e Instalação (`.cartao`)

- Painel centralizado simulando um terminal seguro de autenticação.
- Prefixo de cabeçalho: `// ACCESS_CONTROL // FORJABASE`.
- Faixa decorativa técnica no topo (`.faixa-marca`).
- Inputs escurecidos com anel de foco em Neon Lime.
- Botão de submissão ocupando 100% da largura.

### 4.7 Cartões KPI e Estatísticas (`.grid-kpi`, `.cartao-kpi`)

Inspirado no painel de comando do XENO (Brutalist Dashboard UI):
- Layout de grade responsiva com repetição automática (`repeat(auto-fit, minmax(200px, 1fr))`).
- Superfície obsidiana escura (`#11141c`) com mira/retículo `+` gravada no canto superior direito.
- Rótulo técnico em monospace caixa alta (`// TOTAL COLEÇÕES`, `// REQUISIÇÕES 2XX`).
- Métrica em destaque com tipografia mono arrojada de 28px (`#ffffff`).
- Indicador secundário / tendência em Neon Lime (`#d4ff00`) ou Verde Menta (`#00ffaa`).

### 4.8 Faixas Técnicas e Rodapé de Telemetria (`.faixa-marca`, `.faixa-perigo`, `.rodape`)

- **Faixa Neon (`.faixa-marca`)**: Listras diagonais repetidas em 45º alternando Neon Lime e Void para cabeçalhos e portais.
- **Faixa de Perigo (`.faixa-perigo`)**: Listras diagonais em 45º alternando Vermelho Laser e Void para zonas de risco e exclusão destrutiva.
- **Rodapé Telemetria (`.rodape`)**: Linha de base terminal exibindo status operacional do motor (`FORJABASE // RUST ENGINE // SYS: OPERATIONAL`) e o manifesto Cyber Brutalist (`THE FUTURE ISN'T MINIMAL. IT'S SYSTEMATIC. ▮`).

---

## 5. Micro-interações e Estados

- **Foco (`:focus-visible`)**:
  `outline: none; border-color: #d4ff00; box-shadow: 0 0 0 1px #d4ff00, 0 0 12px rgba(212, 255, 0, 0.25);`
- **Hover em Linhas de Tabela**:
  `background: rgba(212, 255, 0, 0.03);`
- **Pulsar do LED de Status**:
  Animação suave de 2 segundos com oscilação de opacidade entre 1.0 e 0.4.
- **Hover em Barras de Estatística**:
  Transição suave com iluminação Neon Lime e glow localizado.
