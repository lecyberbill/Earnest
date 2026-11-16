# Hook d'adoption de la base de connaissances (Claude Code).
#
# Rien n'oblige un agent à consulter la base : ce hook rend l'étape kb_check difficile à oublier.
#   - PostToolUse (Write/Edit/MultiEdit d'un fichier script) : rappelle d'appeler kb_check.
#   - Stop : bloque la fin du tour (code de sortie 2, message sur stderr renvoyé à l'agent)
#     si un script a été écrit pendant le tour sans appel ultérieur à un outil *kb_check.
#     Le champ stop_hook_active évite toute boucle de blocage.
# La liste des extensions et des outils est dans kb_adoption.json (à côté de ce script).
# Toute erreur du hook (configuration absente, JSON illisible) sort en code 1 : visible, jamais
# transformée en « tout va bien ».

$ErrorActionPreference = 'Stop'
[Console]::InputEncoding = [System.Text.Encoding]::UTF8

function Fail([string]$message) {
    [Console]::Error.WriteLine("kb_adoption: $message")
    exit 1
}

try {
    $configPath = Join-Path $PSScriptRoot 'kb_adoption.json'
    if (-not (Test-Path -LiteralPath $configPath)) { Fail "configuration absente: $configPath" }
    $config = Get-Content -LiteralPath $configPath -Raw -Encoding UTF8 | ConvertFrom-Json
    $extensions = @($config.script_extensions | ForEach-Object { $_.ToLowerInvariant() })
    $editTools = @($config.edit_tools)
    $checkSuffix = [string]$config.check_tool_suffix
    if ($extensions.Count -eq 0 -or $editTools.Count -eq 0 -or -not $checkSuffix) {
        Fail "configuration incomplète: $configPath"
    }

    $raw = [Console]::In.ReadToEnd()
    if (-not $raw.Trim()) { Fail 'aucune donnée d''événement reçue sur stdin' }
    $hookEvent = $raw | ConvertFrom-Json

    function Test-ScriptPath($path) {
        if (-not $path) { return $false }
        $extension = [System.IO.Path]::GetExtension([string]$path).ToLowerInvariant()
        return ($extensions -contains $extension)
    }

    switch ([string]$hookEvent.hook_event_name) {
        'PostToolUse' {
            $path = $hookEvent.tool_input.file_path
            if (($editTools -contains [string]$hookEvent.tool_name) -and (Test-ScriptPath $path)) {
                $reminder = "Script modifié ($path) : appelle l'outil MCP kb_check (script_text + script_kind) avant de conclure."
                $payload = @{
                    hookSpecificOutput = @{
                        hookEventName     = 'PostToolUse'
                        additionalContext = $reminder
                    }
                }
                Write-Output ($payload | ConvertTo-Json -Depth 5 -Compress)
            }
            exit 0
        }
        'Stop' {
            if ($hookEvent.stop_hook_active -eq $true) { exit 0 }
            $transcript = [string]$hookEvent.transcript_path
            if (-not $transcript -or -not (Test-Path -LiteralPath $transcript)) {
                Fail "transcript introuvable: $transcript"
            }
            $position = 0
            $lastScript = -1
            $lastCheck = -1
            $lastScriptPath = ''
            foreach ($line in [System.IO.File]::ReadLines($transcript, [System.Text.Encoding]::UTF8)) {
                if (-not $line.Trim()) { continue }
                $entry = $line | ConvertFrom-Json
                if ($entry.type -eq 'user') {
                    $content = $entry.message.content
                    $isPrompt = ($content -is [string])
                    if (-not $isPrompt -and $content) {
                        $isPrompt = (@($content | Where-Object { $_.type -eq 'text' }).Count -gt 0)
                    }
                    if ($isPrompt) { $lastScript = -1; $lastCheck = -1 }
                    continue
                }
                if ($entry.type -ne 'assistant') { continue }
                foreach ($block in @($entry.message.content)) {
                    if ($block.type -ne 'tool_use') { continue }
                    $position++
                    $name = [string]$block.name
                    if (($editTools -contains $name) -and (Test-ScriptPath $block.input.file_path)) {
                        $lastScript = $position
                        $lastScriptPath = [string]$block.input.file_path
                    }
                    if ($name.EndsWith($checkSuffix)) { $lastCheck = $position }
                }
            }
            if ($lastScript -ge 0 -and $lastCheck -lt $lastScript) {
                [Console]::Error.WriteLine("Un script a été écrit ou modifié ($lastScriptPath) sans appel ultérieur à kb_check. Appelle l'outil MCP kb_check sur ce script avant de conclure ; en cas d'écart à l'exécution, kb_report_incident.")
                exit 2
            }
            exit 0
        }
        default {
            Fail "événement non pris en charge: '$($hookEvent.hook_event_name)'"
        }
    }
}
catch {
    Fail $_.Exception.Message
}
