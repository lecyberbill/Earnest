# -*- coding: utf-8 -*-
import io

P = r"/path/to/Ernest/docs\PORTING_EARNEST_MODE3_MODE2_BCPE.html"

PART2 = u"""
<h2 id="s3">3. VS Code Copilot &mdash; m&eacute;canismes d'extension</h2>
<div class="grid">
  <div class="card">
    <h3>MCP servers</h3>
    <ul>
      <li>Configuration dans <code>.vscode/mcp.json</code> (workspace) ou dans les settings utilisateur ; cl&eacute; racine <code>servers</code>.</li>
      <li>Transports : <code>stdio</code>, <code>http</code>, <code>sse</code>.</li>
      <li>Secrets via <code>inputs</code> + <code>${input:id}</code> : jamais de token en clair dans le fichier versionn&eacute;.</li>
      <li>D&eacute;couverte via le <em>MCP registry</em> et la commande <code>MCP: Add Server</code>.</li>
      <li>Outils expos&eacute;s dans le picker d'outils et r&eacute;f&eacute;renc&eacute;s par <code>#tool</code> dans le chat.</li>
      <li>Approbations par outil et sandboxing de l'agent (<em>Approvals and Permissions</em>).</li>
    </ul>
  </div>
  <div class="card">
    <h3>Agent hooks (preview)</h3>
    <ul>
      <li>Fichiers <code>.github/hooks/*.json</code> (workspace) ou dossier utilisateur ; &eacute;dition via l'<em>Agent Customizations editor</em>.</li>
      <li>Format : <code>{"version":1,"hooks":{"preToolUse":[{"type":"command","command":"..."}]}}</code>.</li>
      <li>&Eacute;v&eacute;nements en camelCase : <code>preToolUse</code>, <code>postToolUse</code>, <code>sessionStart</code>, <code>sessionEnd</code>, <code>userPromptSubmit</code>, <code>stop</code>.</li>
      <li>Payload JSON sur stdin, r&eacute;ponse JSON sur stdout (allow/deny, contexte additionnel).</li>
      <li>Harnais multiples : <em>Copilot</em> (partag&eacute; avec Copilot CLI et l'app GitHub Copilot), <em>Claude</em>, <em>Codex</em> &mdash; chacun avec son propre sch&eacute;ma.</li>
      <li>Harnais <em>Local</em> d&eacute;pr&eacute;ci&eacute; (retrait annonc&eacute; f&eacute;vrier 2027).</li>
      <li>Hooks <em>agent-scoped</em> d&eacute;clar&eacute;s dans les fichiers d'agents personnalis&eacute;s.</li>
      <li>Restrictions possibles via les managed settings d'entreprise.</li>
    </ul>
  </div>
  <div class="card">
    <h3>Instructions / Skills / Agents</h3>
    <ul>
      <li><code>.github/copilot-instructions.md</code> : instructions projet toujours charg&eacute;es.</li>
      <li><code>*.instructions.md</code> avec frontmatter <code>applyTo</code> (globs) : application cibl&eacute;e par fichiers.</li>
      <li><code>AGENTS.md</code> support&eacute;.</li>
      <li><strong>Agent Skills</strong> : dossiers <code>SKILL.md</code>, standard ouvert ; emplacements <code>.github/skills/</code>, <code>.claude/skills/</code>, <code>~/.copilot/skills/</code>.</li>
      <li>Prompt files <code>.prompt.md</code> invoqu&eacute;s par <code>/nom</code>.</li>
      <li>Custom agents <code>.github/agents/*.agent.md</code> (outils, mod&egrave;le, instructions).</li>
      <li>Agent Plugins (marketplace) pouvant embarquer skills, hooks et serveurs MCP.</li>
    </ul>
  </div>
</div>

<h2 id="s4">4. Cursor &mdash; m&eacute;canismes d'extension</h2>
<div class="grid">
  <div class="card">
    <h3>MCP servers</h3>
    <ul>
      <li>Configuration dans <code>.cursor/mcp.json</code> (projet) ou <code>~/.cursor/mcp.json</code> (global) ; cl&eacute; <code>mcpServers</code>.</li>
      <li>Transports : <code>stdio</code>, <code>SSE</code>, <code>Streamable HTTP</code>.</li>
      <li>Capacit&eacute;s : tools, prompts, resources, roots, elicitation, <strong>MCP Apps</strong> (UI interactive retourn&eacute;e par un outil).</li>
      <li>OAuth statique (<code>auth.CLIENT_ID</code> / <code>CLIENT_SECRET</code>) ou enregistrement dynamique ; redirects fixes <code>http://localhost:8787/callback</code> et <code>https://www.cursor.com/agents/mcp/oauth/callback</code>.</li>
      <li>Interpolation <code>${env:VAR}</code> pour les secrets.</li>
      <li>Marketplace Cursor + <code>cursor.directory</code> ; distribution team/enterprise.</li>
      <li>En cloud, les outils MCP apparaissent sous <code>MCP:&lt;tool_name&gt;</code>.</li>
    </ul>
  </div>
  <div class="card">
    <h3>Hooks</h3>
    <ul>
      <li>Fichiers <code>.cursor/hooks.json</code> (projet) ou <code>~/.cursor/hooks.json</code> (utilisateur) ; rechargement automatique.</li>
      <li>Processus spawn&eacute;s, JSON bidirectionnel sur stdio.</li>
      <li>Hooks agent : <code>sessionStart</code>, <code>sessionEnd</code>, <code>preToolUse</code>, <code>postToolUse</code>, <code>postToolUseFailure</code>, <code>subagentStart</code>, <code>subagentStop</code>, <code>beforeShellExecution</code>, <code>afterShellExecution</code>, <code>beforeMCPExecution</code>, <code>afterMCPExecution</code>, <code>beforeReadFile</code>, <code>afterFileEdit</code>, <code>beforeSubmitPrompt</code>, <code>preCompact</code>, <code>stop</code>, <code>afterAgentResponse</code>, <code>afterAgentThought</code>.</li>
      <li>Hooks Tab : <code>beforeTabFileRead</code>, <code>afterTabFileEdit</code>. Hook applicatif : <code>workspaceOpen</code>.</li>
      <li>Types : <code>command</code> (d&eacute;faut) et <code>prompt</code> (&eacute;valu&eacute; par un LLM).</li>
      <li>Windows : ex&eacute;cution via PowerShell, JSON lu depuis <code>$input</code>.</li>
      <li><strong>Compatibilit&eacute; hooks Claude Code</strong> (Third Party Hooks) : r&eacute;utilisation directe possible.</li>
      <li>Cloud agents : hooks projet/team/enterprise uniquement (pas de hooks utilisateur).</li>
    </ul>
  </div>
  <div class="card">
    <h3>Rules / Skills / Subagents</h3>
    <ul>
      <li><strong>Project Rules</strong> : <code>.cursor/rules/*.mdc</code> avec frontmatter <code>description</code>, <code>globs</code>, <code>alwaysApply</code>.</li>
      <li>4 modes : Always Apply / Apply Intelligently / Apply to Specific Files / Apply Manually (<code>@rule</code>).</li>
      <li><strong>User Rules</strong> (globales), <strong>Team Rules</strong> (dashboard, Team/Enterprise), <code>AGENTS.md</code>.</li>
      <li><strong>Agent Skills</strong> : <code>.cursor/skills/</code>, <code>.agents/skills/</code>, <code>~/.cursor/skills/</code>, <code>~/.agents/skills/</code> &mdash; et compatibilit&eacute; <code>.claude/skills/</code>, <code>.codex/skills/</code>.</li>
      <li>Skills imbriqu&eacute;s par sous-dossier, scop&eacute;s automatiquement au r&eacute;pertoire.</li>
      <li>Subagents (<code>/create-subagent</code>), Custom Modes, Plugins.</li>
      <li>Skills int&eacute;gr&eacute;s : <code>/create-rule</code>, <code>/create-hook</code>, <code>/create-skill</code>, <code>/migrate-to-skills</code>.</li>
    </ul>
  </div>
</div>
"""

with io.open(P, "a", encoding="utf-8", newline="\n") as f:
    f.write(PART2)

print("appended", len(PART2))
