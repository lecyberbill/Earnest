# -*- coding: utf-8 -*-
import io

p = r"/path/to/Ernest/docs\PORTING_EARNEST_MODE3_MODE2_BCPE.html"

part3 = u"""
<h2 id="s5">5. Tableau comparatif synth&eacute;tique</h2>
<div class="tablewrap">
<table>
<thead>
<tr><th>Capacit&eacute;</th><th>Claude Code</th><th>VS Code Copilot</th><th>Cursor</th></tr>
</thead>
<tbody>
<tr>
  <td><strong>Serveurs MCP &mdash; fichier</strong></td>
  <td><code>.mcp.json</code> (projet), <code>~/.claude.json</code> (user)</td>
  <td><code>.vscode/mcp.json</code> (cl&eacute; <code>servers</code>)</td>
  <td><code>.cursor/mcp.json</code> (cl&eacute; <code>mcpServers</code>)</td>
</tr>
<tr>
  <td><strong>Transports MCP</strong></td>
  <td>stdio, http/streamable-http, sse, ws</td>
  <td>stdio, http, sse</td>
  <td>stdio, SSE, Streamable HTTP</td>
</tr>
<tr>
  <td><strong>Secrets MCP</strong></td>
  <td><code>${VAR}</code>, <code>${VAR:-def}</code>, <code>headersHelper</code></td>
  <td><code>inputs</code> + <code>${input:id}</code></td>
  <td><code>${env:VAR}</code>, <code>auth.CLIENT_ID</code></td>
</tr>
<tr>
  <td><strong>Hook pr&eacute;-outil</strong></td>
  <td><code>PreToolUse</code> (matcher + <code>if</code>)</td>
  <td><code>preToolUse</code> (camelCase)</td>
  <td><code>preToolUse</code> + <code>beforeShellExecution</code> / <code>beforeMCPExecution</code></td>
</tr>
<tr>
  <td><strong>Hook post-outil</strong></td>
  <td><code>PostToolUse</code>, <code>PostToolUseFailure</code>, <code>PostToolBatch</code></td>
  <td><code>postToolUse</code></td>
  <td><code>postToolUse</code>, <code>postToolUseFailure</code>, <code>afterFileEdit</code></td>
</tr>
<tr>
  <td><strong>Hook de session</strong></td>
  <td><code>SessionStart</code>, <code>SessionEnd</code>, <code>Setup</code></td>
  <td><code>sessionStart</code>, <code>sessionEnd</code></td>
  <td><code>sessionStart</code>, <code>sessionEnd</code>, <code>workspaceOpen</code></td>
</tr>
<tr>
  <td><strong>Hook de prompt</strong></td>
  <td><code>UserPromptSubmit</code>, <code>UserPromptExpansion</code></td>
  <td><code>userPromptSubmit</code></td>
  <td><code>beforeSubmitPrompt</code></td>
</tr>
<tr>
  <td><strong>Hook de fin</strong></td>
  <td><code>Stop</code>, <code>StopFailure</code>, <code>SubagentStop</code></td>
  <td><code>stop</code></td>
  <td><code>stop</code>, <code>subagentStop</code>, <code>afterAgentResponse</code></td>
</tr>
<tr>
  <td><strong>Hook de compaction</strong></td>
  <td><code>PreCompact</code>, <code>PostCompact</code></td>
  <td>&mdash; (non expos&eacute;)</td>
  <td><code>preCompact</code></td>
</tr>
<tr>
  <td><strong>Types de handler</strong></td>
  <td>command, http, mcp_tool, prompt, agent, async</td>
  <td>command</td>
  <td>command, prompt</td>
</tr>
<tr>
  <td><strong>D&eacute;cision de blocage</strong></td>
  <td><code>permissionDecision</code> allow/deny/ask + exit&nbsp;2</td>
  <td>JSON stdout (allow/deny) + exit code</td>
  <td>JSON stdout (allow/deny) + exit code</td>
</tr>
<tr>
  <td><strong>R&eacute;pertoire hooks</strong></td>
  <td><code>.claude/settings.json</code>, <code>settings.local.json</code>, plugins</td>
  <td><code>.github/hooks/*.json</code>, user folder</td>
  <td><code>.cursor/hooks.json</code>, <code>~/.cursor/hooks.json</code></td>
</tr>
<tr>
  <td><strong>Instructions projet</strong></td>
  <td><code>CLAUDE.md</code> + <code>.claude/rules/*.md</code></td>
  <td><code>.github/copilot-instructions.md</code> + <code>*.instructions.md</code> (<code>applyTo</code>)</td>
  <td><code>.cursor/rules/*.mdc</code> (<code>globs</code>/<code>alwaysApply</code>) + <code>AGENTS.md</code></td>
</tr>
<tr>
  <td><strong>Skills (standard ouvert)</strong></td>
  <td><code>.claude/skills/&lt;n&gt;/SKILL.md</code></td>
  <td><code>.github/skills/</code>, <code>.claude/skills/</code></td>
  <td><code>.cursor/skills/</code>, <code>.agents/skills/</code>, <code>.claude/skills/</code></td>
</tr>
<tr>
  <td><strong>Chargement progressif</strong></td>
  <td>Oui (frontmatter seul au repos)</td>
  <td>Oui</td>
  <td>Oui</td>
</tr>
<tr>
  <td><strong>Sous-agents</strong></td>
  <td><code>.claude/agents/*.md</code></td>
  <td><code>.github/agents/*.agent.md</code></td>
  <td>Subagents + Custom Modes</td>
</tr>
<tr>
  <td><strong>Plugins / marketplace</strong></td>
  <td>Oui (<code>/plugin install</code>)</td>
  <td>Oui (Agent Plugins)</td>
  <td>Oui (Marketplace + cursor.directory)</td>
</tr>
<tr>
  <td><strong>Compat. hooks Claude Code</strong></td>
  <td>&mdash; (r&eacute;f&eacute;rence)</td>
  <td>Harnais &laquo;&nbsp;Claude&nbsp;&raquo; s&eacute;lectionnable</td>
  <td>Oui (Third Party Hooks)</td>
</tr>
<tr>
  <td><strong>Maturit&eacute; hooks</strong></td>
  <td>GA, surface tr&egrave;s large</td>
  <td>Preview</td>
  <td>GA</td>
</tr>
</tbody>
</table>
</div>

<h2 id="s6">6. Recettes de portage du workflow Earnest</h2>

<h3>6.1 Mode 3 &mdash; Spec-First (geler les invariants)</h3>
<p><strong>Objectif Earnest</strong> : produire <code>.ide/invariants.json</code>, le sceller en SHA-256 dans
<code>.ide/invariants.lock</code> (lecture seule <code>0444</code>), puis interdire toute mutation sans
sign-off humain.</p>
<div class="grid">
  <div class="card">
    <h3>Claude Code</h3>
    <ul>
      <li>Skill <code>.claude/skills/mode3-spec/SKILL.md</code> : proc&eacute;dure de conception + format des probes.</li>
      <li>Hook <code>PreToolUse</code> sur <code>Write|Edit</code> avec <code>if</code> ciblant <code>invariants.lock</code> &rarr; <code>permissionDecision: "deny"</code>.</li>
      <li>Hook <code>Stop</code> : v&eacute;rifie que le lock existe et que son SHA-256 correspond.</li>
      <li>Hook <code>InstructionsLoaded</code> : injecte le rappel &laquo;&nbsp;invariants scell&eacute;s&nbsp;&raquo;.</li>
    </ul>
  </div>
  <div class="card">
    <h3>VS Code Copilot</h3>
    <ul>
      <li>Skill <code>.github/skills/mode3-spec/SKILL.md</code> (m&ecirc;me contenu, portable).</li>
      <li>Hook <code>preToolUse</code> dans <code>.github/hooks/seal.json</code> : refuse l'&eacute;criture sur <code>invariants.lock</code>.</li>
      <li>Instructions <code>*.instructions.md</code> avec <code>applyTo: ".ide/**"</code> pour le rappel de contrat.</li>
      <li>Custom agent <code>architect.agent.md</code> restreint aux outils de lecture + <code>add_invariant</code>.</li>
    </ul>
  </div>
  <div class="card">
    <h3>Cursor</h3>
    <ul>
      <li>Skill <code>.cursor/skills/mode3-spec/SKILL.md</code> (ou <code>.agents/skills/</code>).</li>
      <li>Hook <code>preToolUse</code> + <code>beforeReadFile</code> dans <code>.cursor/hooks.json</code>.</li>
      <li>Rule <code>.cursor/rules/invariants.mdc</code> avec <code>globs: .ide/**</code>, <code>alwaysApply: false</code>.</li>
      <li>Hook <code>stop</code> : contr&ocirc;le d'int&eacute;grit&eacute; du lock avant cl&ocirc;ture du tour.</li>
    </ul>
  </div>
</div>

<h3>6.2 Mode 2 &mdash; Shadow Worktree (impl&eacute;mentation contrainte)</h3>
<p><strong>Objectif Earnest</strong> : tout patch est appliqu&eacute; dans <code>.ide/shadow_worktree/</code>,
valid&eacute; par probes statiques (AST) et dynamiques (CLI), puis fusionn&eacute; atomiquement seulement si
100&nbsp;% PASS. Sinon : rejet, trace, arbre de travail intact.</p>
<div class="grid">
  <div class="card">
    <h3>Claude Code</h3>
    <ul>
      <li>Hook <code>PreToolUse</code> sur <code>Write|Edit</code> : r&eacute;&eacute;crit <code>updatedInput</code> pour rediriger le chemin vers <code>.ide/shadow_worktree/</code>.</li>
      <li>Hook <code>PostToolUse</code> : lance <code>cargo test</code> / <code>pytest</code> + inspection AST ; en cas d'&eacute;chec, <code>additionalContext</code> avec la trace.</li>
      <li>Hook <code>PostToolBatch</code> : v&eacute;rifie l'ensemble des probes avant le tour suivant.</li>
      <li>Hook <code>Stop</code> : bloque la fin de tour si des probes n'ont pas tourn&eacute;.</li>
      <li>Subagent <code>verifier</code> en contexte isol&eacute; pour l'arbitrage.</li>
    </ul>
  </div>
  <div class="card">
    <h3>VS Code Copilot</h3>
    <ul>
      <li>Hook <code>preToolUse</code> : validation/redirection du chemin d'&eacute;criture.</li>
      <li>Hook <code>postToolUse</code> : ex&eacute;cution des probes et retour du verdict.</li>
      <li>Hook <code>stop</code> : refus de cl&ocirc;ture si verdict incomplet.</li>
      <li>Custom agent <code>implementer.agent.md</code> avec outils limit&eacute;s au worktree.</li>
      <li>Limite : pas de <code>PostToolBatch</code> ni de <code>PreCompact</code> &rarr; arbitrage par lot &agrave; simuler dans le hook <code>stop</code>.</li>
    </ul>
  </div>
  <div class="card">
    <h3>Cursor</h3>
    <ul>
      <li>Hook <code>preToolUse</code> : redirection vers le worktree (Cursor ignore <code>updated_input</code> pour les outils MCP distants, mais l'accepte pour les outils locaux).</li>
      <li>Hook <code>afterFileEdit</code> : formate puis d&eacute;clenche les probes.</li>
      <li>Hook <code>beforeShellExecution</code> : n'autorise que les runners d&eacute;clar&eacute;s (<code>cargo test</code>, <code>pytest</code>).</li>
      <li>Hook <code>stop</code> : v&eacute;rifie le verdict global.</li>
      <li>Hook <code>subagentStop</code> : arbitrage du sous-agent de v&eacute;rification.</li>
    </ul>
  </div>
</div>

<h3>6.3 BCPE &mdash; Base de Connaissances d'Exp&eacute;rience (serveur MCP)</h3>
<p><strong>Objectif Earnest</strong> : exposer <code>kb.exe serve</code> comme serveur MCP stdio, avec les
outils <code>kb_search</code>, <code>kb_get</code>, <code>kb_check</code>, <code>kb_report_incident</code>,
<code>kb_propose</code>, <code>kb_record_outcome</code>, <code>kb_confirm</code>, <code>kb_attach_repro</code>.
Le texte d'une entr&eacute;e est une <em>donn&eacute;e</em>, jamais une instruction.</p>
<div class="grid">
  <div class="card">
    <h3>Claude Code &mdash; <code>.mcp.json</code></h3>
<pre><code>{
  "mcpServers": {
    "bcpe": {
      "type": "stdio",
      "command": "/path/to/Ernest/earnest-bcpe/target/release/kb.exe",
      "args": ["serve", "--db", "/path/to/Ernest/earnest-bcpe\\kb.db"]
    }
  }
}</code></pre>
    <p>Outils visibles sous <code>mcp__bcpe__kb_search</code>, etc. Hook <code>PreToolUse</code> avec
    <code>matcher: "mcp__bcpe__.*"</code> pour journaliser chaque requ&ecirc;te BCPE.</p>
  </div>
  <div class="card">
    <h3>VS Code Copilot &mdash; <code>.vscode/mcp.json</code></h3>
<pre><code>{
  "servers": {
    "bcpe": {
      "type": "stdio",
      "command": "/path/to/Ernest/earnest-bcpe/target/release/kb.exe",
      "args": ["serve", "--db", "/path/to/Ernest/earnest-bcpe\\kb.db"]
    }
  }
}</code></pre>
    <p>Appara&icirc;t dans la vue <em>MCP Servers</em> ; outils s&eacute;lectionnables via <code>#kb_search</code>.</p>
  </div>
  <div class="card">
    <h3>Cursor &mdash; <code>.cursor/mcp.json</code></h3>
<pre><code>{
  "mcpServers": {
    "bcpe": {
      "command": "/path/to/Ernest/earnest-bcpe/target/release/kb.exe",
      "args": ["serve", "--db", "/path/to/Ernest/earnest-bcpe\\kb.db"]
    }
  }
}</code></pre>
    <p>Hook <code>beforeMCPExecution</code> pour tracer les appels BCPE et bloquer les &eacute;critures non autoris&eacute;es.</p>
  </div>
</div>

<h3>6.4 C&acirc;blage des trois briques ensemble</h3>
<div class="tablewrap">
<table>
<thead><tr><th>&Eacute;tape Earnest</th><th>M&eacute;canisme Claude Code</th><th>M&eacute;canisme VS Code Copilot</th><th>M&eacute;canisme Cursor</th></tr></thead>
<tbody>
<tr><td>Rappel d'incidents avant conception</td><td>Skill + <code>kb_search</code> MCP</td><td>Skill + <code>#kb_search</code></td><td>Skill + <code>kb_search</code></td></tr>
<tr><td>Gel des invariants</td><td><code>PreToolUse</code> deny sur lock</td><td><code>preToolUse</code> deny sur lock</td><td><code>preToolUse</code> deny sur lock</td></tr>
<tr><td>Redirection shadow worktree</td><td><code>updatedInput</code></td><td><code>preToolUse</code> validation</td><td><code>preToolUse</code> validation</td></tr>
<tr><td>Ex&eacute;cution des probes</td><td><code>PostToolUse</code> + <code>PostToolBatch</code></td><td><code>postToolUse</code></td><td><code>afterFileEdit</code> + <code>beforeShellExecution</code></td></tr>
<tr><td>Arbitrage final</td><td><code>Stop</code> + subagent</td><td><code>stop</code> + custom agent</td><td><code>stop</code> + <code>subagentStop</code></td></tr>
<tr><td>Apprentissage (BCPE)</td><td><code>kb_record_outcome</code> / <code>kb_confirm</code></td><td><code>kb_record_outcome</code> / <code>kb_confirm</code></td><td><code>kb_record_outcome</code> / <code>kb_confirm</code></td></tr>
</tbody>
</table>
</div>

<h2 id="s7">7. Recommandations et limites</h2>
<div class="grid">
  <div class="card">
    <h3>Portabilit&eacute; maximale</h3>
    <ul>
      <li>Placer les skills dans <code>.claude/skills/</code> : les trois outils les lisent (Cursor et Copilot supportent explicitement ce r&eacute;pertoire).</li>
      <li>Utiliser le standard ouvert <em>Agent Skills</em> pour le contenu, et r&eacute;server les extensions propri&eacute;taires au strict n&eacute;cessaire.</li>
      <li>Garder les scripts de hooks en <strong>PowerShell</strong> (Windows) ou en Python portable, avec lecture JSON sur stdin.</li>
      <li>Centraliser la logique m&eacute;tier (scellement, probes, arbitrage) dans le binaire <code>kb</code> / <code>earnest-core</code> et laisser les hooks n'&ecirc;tre que de fins adaptateurs.</li>
    </ul>
  </div>
  <div class="card">
    <h3>Limites &agrave; anticiper</h3>
    <ul>
      <li><strong>VS Code Copilot</strong> : hooks en <em>preview</em>, surface d'&eacute;v&eacute;nements plus &eacute;troite (pas de <code>PostToolBatch</code>, <code>PreCompact</code>, <code>SubagentStop</code>).</li>
      <li><strong>Cursor</strong> : <code>updated_input</code> ignor&eacute; pour les outils MCP distants ; hooks utilisateur absents en cloud agents.</li>
      <li><strong>Claude Code</strong> : surface la plus riche mais la plus sp&eacute;cifique ; les hooks ne sont pas portables tels quels vers les deux autres sans adaptation du sch&eacute;ma.</li>
      <li><strong>Nommage</strong> : <code>PreToolUse</code> (PascalCase) vs <code>preToolUse</code> (camelCase) &mdash; source d'erreurs fr&eacute;quente lors du portage.</li>
      <li><strong>S&eacute;curit&eacute;</strong> : un hook s'ex&eacute;cute sans validation du mod&egrave;le ; revoir chaque script avant installation, surtout depuis une marketplace.</li>
    </ul>
  </div>
</div>

<h2 id="s8">8. Conclusion</h2>
<p>Les trois outils convergent vers un m&ecirc;me mod&egrave;le en trois couches&nbsp;:
<strong>MCP</strong> pour les capacit&eacute;s (BCPE), <strong>hooks</strong> pour le contr&ocirc;le
(scel des invariants, shadow worktree, arbitrage), et <strong>skills/rules</strong> pour la connaissance
(proc&eacute;dure Mode&nbsp;3, conventions Mode&nbsp;2). Le workflow Earnest est donc portable dans son
<em>architecture</em>, mais pas dans ses <em>fichiers</em>&nbsp;: chaque cible exige un adaptateur de
sch&eacute;ma de hooks, tandis que les skills et le serveur MCP BCPE restent, eux, directement r&eacute;utilisables.</p>
<p>La strat&eacute;gie la plus robuste consiste &agrave; faire de <code>earnest-bcpe</code> et
<code>earnest-core</code> les v&eacute;ritables d&eacute;tenteurs de la logique de gouvernance, expos&eacute;s
via MCP, et &agrave; r&eacute;duire les hooks de chaque IDE &agrave; de simples points d'entr&eacute;e
d&eacute;clenchant ces binaires. On obtient alors un comportement identique sur Claude Code, VS Code Copilot
et Cursor, ind&eacute;pendamment des &eacute;volutions de leurs API d'extension respectives.</p>

<footer>
  <p>Document g&eacute;n&eacute;r&eacute; pour le projet Hernest &mdash; Earnest Ecosystem.
  Sources&nbsp;: documentation officielle Claude Code (hooks reference, MCP, skills),
  VS Code (agent hooks, MCP, agent skills, custom instructions),
  Cursor (hooks, MCP, rules, agent skills).</p>
</footer>
</main>
</body>
</html>
"""

with io.open(p, "a", encoding="utf-8", newline="\n") as f:
    f.write(part3)

print("appended", len(part3))
